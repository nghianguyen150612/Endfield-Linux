#!/usr/bin/env python3
"""Mount and inspect a SquashFS Wine image in a private child namespace; never execute Wine."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


def hash_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def inspect(image_path, evidence_path, squashfuse, output_path):
    image = Path(image_path)
    evidence_file = Path(evidence_path)
    evidence = json.loads(evidence_file.read_text(encoding="utf-8"))
    if (image.is_symlink() or not image.is_file()
            or image.stat().st_size != evidence.get("image_size")
            or hash_file(image) != evidence.get("image_sha256")
            or evidence.get("format") != "squashfs"
            or evidence.get("logical_inventory_sha256") != evidence.get("transformed_inventory_sha256")):
        raise ValueError("image bytes or P025 scope do not match image evidence")
    if Path(output_path).exists():
        raise ValueError("refusing to overwrite mount evidence")
    version_result = subprocess.run([str(squashfuse), "-V"], capture_output=True, text=True)
    provider_version = (version_result.stdout + version_result.stderr).strip().splitlines()[0]
    with tempfile.TemporaryDirectory(prefix="p034-wine-image-view-") as temporary:
        base = Path(temporary)
        mountpoint = base / "mnt"
        mountpoint.mkdir()
        backing = os.memfd_create("p034-wine-squashfs-image", os.MFD_ALLOW_SEALING)
        digest = hashlib.sha256()
        with image.open("rb") as source:
            while block := source.read(1024 * 1024):
                digest.update(block)
                view = memoryview(block)
                while view:
                    written = os.write(backing, view)
                    view = view[written:]
        if digest.hexdigest() != evidence["image_sha256"]:
            os.close(backing)
            raise ValueError("image changed while copying to sealed backing")
        seals = (getattr(fcntl, "F_SEAL_WRITE", 0x0008)
                 | getattr(fcntl, "F_SEAL_GROW", 0x0004)
                 | getattr(fcntl, "F_SEAL_SHRINK", 0x0002)
                 | getattr(fcntl, "F_SEAL_SEAL", 0x0001))
        fcntl.fcntl(backing, getattr(fcntl, "F_ADD_SEALS", 1033), seals)
        os.set_inheritable(backing, True)
        try:
            try:
                os.pwrite(backing, b"mutate", 0)
                raise ValueError("sealed image backing accepted a write")
            except PermissionError:
                pass
            wrapper = base / "mount-inspect.sh"
            wrapper.write_text(r'''#!/bin/sh
set -eu
mountpoint="$P034_MOUNTPOINT"
cleanup() { /usr/bin/fusermount3 -u "$mountpoint" >/dev/null 2>&1 || true; }
trap cleanup EXIT
"$P034_SQUASHFUSE" -o ro,nosuid,nodev "/proc/self/fd/$P034_BACKING_FD" "$mountpoint"
record=$(awk -v p="$mountpoint" '$5 == p { print $6; found=1 } END { if (!found) exit 1 }' /proc/self/mountinfo)
case ",$record," in *,ro,*nosuid,*nodev,*) ;; *) echo "unexpected mount options: $record" >&2; exit 51 ;; esac
echo "MOUNT_OPTIONS=$record"
python3 "$P034_VERIFY_SCRIPT" "$mountpoint" "$P034_IMAGE_EVIDENCE"
for entry in "$mountpoint/opt/wine-stable/bin/wine64" \
             "$mountpoint/opt/wine-stable/bin/wine64-preloader" \
             "$mountpoint/opt/wine-stable/bin/wineserver"; do
  if printf x >> "$entry" 2>/dev/null; then exit 52; fi
done
if mv "$mountpoint/opt/wine-stable/bin/wine64" "$mountpoint/opt/wine-stable/bin/replaced" 2>/dev/null; then exit 53; fi
if ln -s /tmp/escape "$mountpoint/opt/wine-stable/bin/wine64" 2>/dev/null; then exit 54; fi
echo "RUNTIME_WRITES_REJECTED"
cleanup
trap - EXIT
''')
            wrapper.chmod(0o700)
            before = Path("/proc/self/mountinfo").read_text()
            env = dict(os.environ)
            env.update({
                "P034_MOUNTPOINT": str(mountpoint),
                "P034_SQUASHFUSE": str(squashfuse),
                "P034_BACKING_FD": str(backing),
                "P034_VERIFY_SCRIPT": str(Path(__file__).with_name("verify-squashfs-runtime-inventory.py")),
                "P034_IMAGE_EVIDENCE": str(evidence_file),
            })
            result = subprocess.run([
                "/usr/bin/unshare", "--user", "--map-root-user", "--mount", "--propagation", "private",
                "/bin/sh", str(wrapper),
            ], env=env, pass_fds=(backing,), capture_output=True, text=True, timeout=120)
            if result.returncode != 0:
                raise RuntimeError(f"private SquashFS view failed ({result.returncode}): {result.stderr[-2000:]}")
            after = Path("/proc/self/mountinfo").read_text()
            if before != after or str(mountpoint) in after:
                raise RuntimeError("parent mount namespace changed or mount cleanup was incomplete")
            mounted_record = json.loads(next(line for line in result.stdout.splitlines() if line.startswith("{")))
            report = {
                "schema_version": 1,
                "state": "ACTUAL_WINE_IMAGE_VIEW_QUALIFIED",
                "image_sha256": evidence["image_sha256"],
                "image_size": evidence["image_size"],
                "provider": provider_version,
                "backing": "sealed_memfd",
                "mount_options": next(line.split("=", 1)[1] for line in result.stdout.splitlines() if line.startswith("MOUNT_OPTIONS=")),
                "mounted_inventory_sha256": mounted_record["mounted_inventory_sha256"],
                "mounted_entries": mounted_record["mounted_entry_count"],
                "runtime_file_write_replace_symlink_attempts": "rejected",
                "parent_namespace_unchanged": True,
                "mount_cleanup": "unmounted_before_namespace_owner_exit",
                "wine_spawned": False,
                "authentic_execution": "NOT_RUN",
                "p025_installation_id": evidence["p025_installation_id"],
                "p025_installed_generation": evidence["p025_installed_generation"],
                "p026_baseline_sha256": evidence["p026_baseline_sha256"],
                "p027_pin_id": evidence["p027_pin_id"],
                "p027_pin_registry_revision": evidence["p027_pin_registry_revision"],
            }
            Path(output_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        finally:
            os.close(backing)
    print(json.dumps({"state": "ACTUAL_WINE_IMAGE_VIEW_QUALIFIED", "image_sha256": report["image_sha256"],
                      "mounted_entries": report["mounted_entries"], "mount_options": report["mount_options"],
                      "wine_spawned": False}, sort_keys=True))


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--squashfuse", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    try:
        inspect(args.image, args.evidence, args.squashfuse, args.output)
    except (OSError, ValueError, KeyError, json.JSONDecodeError, subprocess.SubprocessError, RuntimeError) as error:
        raise SystemExit(f"actual Wine image inspection failed: {error}")
