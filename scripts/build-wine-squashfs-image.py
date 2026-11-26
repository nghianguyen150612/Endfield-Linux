#!/usr/bin/env python3
"""Build two byte-identical SquashFS images from a P025-validated Wine tree."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def digest_json(value):
    raw = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(raw).hexdigest()


def inventory_tree(root):
    root_info = root.lstat()
    if not stat.S_ISDIR(root_info.st_mode) or stat.S_ISLNK(root_info.st_mode):
        raise ValueError("runtime root must be a real directory")
    output = []
    stack = [(root, "")]
    while stack:
        directory, prefix = stack.pop()
        with os.scandir(directory) as iterator:
            children = sorted(iterator, key=lambda entry: entry.name, reverse=True)
        for child in children:
            name = child.name
            if not name or name in (".", "..") or "/" in name or "\\" in name:
                raise ValueError("unsafe image path")
            if not prefix and name in (".endfield-installation.json", ".endfield-runtime-baseline.json"):
                continue
            relative = f"{prefix}/{name}" if prefix else name
            path = directory / name
            info = path.lstat()
            mode = stat.S_IMODE(info.st_mode)
            if stat.S_ISDIR(info.st_mode):
                output.append({"path": relative, "kind": {"type": "directory", "mode": mode}})
                stack.append((path, relative))
            elif stat.S_ISREG(info.st_mode):
                if info.st_mode & (stat.S_ISUID | stat.S_ISGID):
                    raise ValueError("set-id file in runtime tree")
                opened = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
                try:
                    current = os.fstat(opened)
                    if (current.st_dev, current.st_ino, current.st_size) != (info.st_dev, info.st_ino, info.st_size):
                        raise ValueError("file changed while inventory was read")
                    digest = hashlib.sha256()
                    while block := os.read(opened, 1024 * 1024):
                        digest.update(block)
                finally:
                    os.close(opened)
                output.append({"path": relative, "kind": {
                    "type": "file", "size": info.st_size, "sha256": digest.hexdigest(),
                    "executable": bool(info.st_mode & 0o111), "setid": False,
                }})
            elif stat.S_ISLNK(info.st_mode):
                target = os.readlink(path)
                if target.startswith("/") or "\\" in target or any(part in ("", ".", "..") for part in target.split("/")):
                    raise ValueError("unsafe symlink in runtime tree")
                output.append({"path": relative, "kind": {"type": "symlink", "target": target}})
            else:
                raise ValueError("unsupported runtime entry")
    return sorted(output, key=lambda entry: entry["path"])


def validate_managed_identity(root, expected_inventory, artifact_hash, packaging, manifest_sha256, pin_registry_path):
    record_path = root / ".endfield-installation.json"
    baseline_path = root / ".endfield-runtime-baseline.json"
    for path in (record_path, baseline_path, pin_registry_path):
        if path.is_symlink() or not path.is_file():
            raise ValueError("P025/P026/P027 evidence must be regular files")
    record = json.loads(record_path.read_text(encoding="utf-8"))
    plan = record.get("plan", {})
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if (record.get("schema_version") != 1 or record.get("state") != "Published"
            or record.get("trust") != "development_only" or plan.get("trust") != "development_only"
            or plan.get("family") != "wine" or plan.get("lineage") != "upstream-wine"
            or plan.get("build") != "wine-10.0" or plan.get("provider") != "endfield-unix-development"
            or plan.get("project") != "p034-disposable" or plan.get("channel") != "development"
            or plan.get("artifact_sha256") != artifact_hash or plan.get("manifest_sha256") != manifest_sha256
            or plan.get("archive_size") != packaging.get("artifact_size") or plan.get("generation") != packaging.get("generation")):
        raise ValueError("P025 ownership record does not match development manifest/candidate scope")
    if (baseline.get("schema_version") != 1 or baseline.get("installation_id") != plan.get("installation_id")
            or baseline.get("generation") != plan.get("generation") or baseline.get("family") != "wine"
            or baseline.get("build") != "wine-10.0" or baseline.get("artifact_sha256") != artifact_hash
            or baseline.get("manifest_sha256") != manifest_sha256 or baseline.get("trust") != "development_only"
            or baseline.get("origin") != "verified_installation"
            or baseline.get("inventory_sha256") != packaging.get("transformed_inventory_sha256")
            or baseline.get("entries") != expected_inventory):
        raise ValueError("P026 installed baseline does not match the transformed candidate")
    registry = json.loads(pin_registry_path.read_text(encoding="utf-8"))
    owners_hash = sha256_file(record_path)
    matches = []
    for pin in registry.get("pins", []):
        runtime = pin.get("runtime", {})
        identity = runtime.get("identity", {})
        if runtime.get("kind") == "managed" and identity.get("installation_id") == plan.get("installation_id"):
            matches.append((pin, identity))
    if len(matches) != 1:
        raise ValueError("P027 registry must contain one exact managed installation pin")
    pin, identity = matches[0]
    if (registry.get("schema_version") != 2 or registry.get("revision", 0) < 1
            or identity.get("family") != "wine" or identity.get("lineage") != "upstream-wine"
            or identity.get("provider") != "endfield-unix-development"
            or identity.get("project") != "p034-disposable" or identity.get("build") != "wine-10.0"
            or identity.get("artifact_sha256") != artifact_hash or identity.get("manifest_sha256") != manifest_sha256
            or identity.get("generation") != plan.get("generation")
            or identity.get("ownership_record_sha256") != owners_hash
            or identity.get("trust") != "development_only"):
        raise ValueError("P027 pin identity or development trust does not match P025/P026")
    return record, baseline, registry, pin, owners_hash


def run_builder(builder, root, destination):
    command = [str(builder), str(root), str(destination), "-noappend", "-reproducible",
               "-all-root", "-no-xattrs", "-no-exports", "-no-progress", "-no-duplicates",
               "-mkfs-time", "0", "-all-time", "0", "-processors", "1",
               "-comp", "zstd", "-Xcompression-level", "15"]
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def materialize_payload(source_root, destination, inventory):
    destination.mkdir(mode=0o700)
    directories = [entry for entry in inventory if entry["kind"]["type"] == "directory"]
    for entry in sorted(directories, key=lambda item: (item["path"].count("/"), item["path"])):
        target = destination / entry["path"]
        target.mkdir(mode=entry["kind"]["mode"])
    for entry in inventory:
        relative = entry["path"]
        kind = entry["kind"]
        source = source_root / relative
        target = destination / relative
        if kind["type"] == "directory":
            continue
        if kind["type"] == "symlink":
            if not source.is_symlink() or os.readlink(source) != kind["target"]:
                raise ValueError("symlink changed while creating the image source tree")
            os.symlink(kind["target"], target)
            continue
        before = source.lstat()
        if not stat.S_ISREG(before.st_mode) or before.st_size != kind["size"]:
            raise ValueError("file changed while creating the image source tree")
        source_fd = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        target_fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        digest = hashlib.sha256()
        size = 0
        try:
            current = os.fstat(source_fd)
            if (current.st_dev, current.st_ino, current.st_size) != (before.st_dev, before.st_ino, before.st_size):
                raise ValueError("file changed before image materialization")
            while block := os.read(source_fd, 1024 * 1024):
                digest.update(block)
                size += len(block)
                view = memoryview(block)
                while view:
                    written = os.write(target_fd, view)
                    view = view[written:]
            after = os.fstat(source_fd)
            if ((after.st_dev, after.st_ino, after.st_size) != (current.st_dev, current.st_ino, current.st_size)
                    or size != kind["size"] or digest.hexdigest() != kind["sha256"]):
                raise ValueError("file bytes changed during image materialization")
            os.fchmod(target_fd, 0o700 if kind["executable"] else 0o600)
        finally:
            os.close(target_fd)
            os.close(source_fd)
    for entry in directories:
        (destination / entry["path"]).chmod(entry["kind"]["mode"])


def build(args):
    for path in (args.artifact, args.profile, args.packaging, args.transformed_inventory):
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"required evidence is not a regular file: {path}")
    if args.output.exists() or args.evidence.exists():
        raise ValueError("refusing to overwrite image or evidence")
    parent = args.output.parent.resolve(strict=True)
    if args.output.parent != parent or parent.stat().st_uid != os.geteuid() or parent.stat().st_mode & 0o077:
        raise ValueError("output directory must be a canonical private user-owned directory")
    packaging = json.loads(args.packaging.read_text(encoding="utf-8"))
    expected = json.loads(args.transformed_inventory.read_text(encoding="utf-8"))
    profile_bytes = args.profile.read_bytes()
    artifact_hash = sha256_file(args.artifact)
    if (args.artifact.name != "winehq-noble-10.0-dualarch-candidate.tar.gz"
            or args.artifact.stat().st_size != packaging.get("artifact_size")
            or artifact_hash != packaging.get("artifact_sha256")
            or digest_json(expected) != packaging.get("transformed_inventory_sha256")):
        raise ValueError("artifact or transformed inventory does not match packaging evidence")
    profile = json.loads(profile_bytes)
    if (profile.get("derived_artifact_sha256") != artifact_hash
            or profile.get("inventory_sha256") != packaging.get("transformed_inventory_sha256")
            or profile.get("source_inventory_sha256") != packaging.get("source_inventory_sha256")):
        raise ValueError("profile does not bind the candidate and package inventories")
    actual = inventory_tree(args.root)
    if actual != expected:
        raise ValueError("P025 tree inventory does not match the transformed inventory")
    if digest_json(actual) != packaging.get("transformed_inventory_sha256"):
        raise ValueError("logical image inventory digest mismatch")
    manifest_sha256 = args.manifest_sha256
    record, baseline, pin_registry, pin, ownership_record_sha256 = validate_managed_identity(
        args.root, expected, artifact_hash, packaging, manifest_sha256, args.pin_registry)
    builder = args.mksquashfs.resolve(strict=True)
    if builder.is_symlink() or not builder.is_file():
        raise ValueError("mksquashfs must be a resolved regular file")
    version = subprocess.run([str(builder), "-version"], check=True, capture_output=True, text=True).stdout.strip()
    with tempfile.TemporaryDirectory(prefix="p034-squashfs-build-", dir=parent) as workspace:
        image_source = Path(workspace) / "runtime-root"
        first = Path(workspace) / "first.squashfs"
        second = Path(workspace) / "second.squashfs"
        materialize_payload(args.root, image_source, actual)
        if inventory_tree(image_source) != actual:
            raise ValueError("materialized image root inventory changed")
        run_builder(builder, image_source, first)
        run_builder(builder, image_source, second)
        first_hash = sha256_file(first)
        second_hash = sha256_file(second)
        if first.stat().st_size != second.stat().st_size or first_hash != second_hash:
            raise ValueError("SquashFS output was not byte-reproducible")
        os.replace(second, args.output)
    image_size = args.output.stat().st_size
    evidence = {
        "schema_version": 1,
        "state": "IMAGE_BUILT_AND_BYTE_REPRODUCIBLE",
        "format": "squashfs",
        "provider_target": "squashfuse_read_only_userspace",
        "candidate_archive_sha256": artifact_hash,
        "candidate_archive_size": args.artifact.stat().st_size,
        "source_package_sha256": [item["sha256"] for item in packaging["source_packages"]],
        "source_inventory_sha256": packaging["source_inventory_sha256"],
        "transformed_inventory_sha256": packaging["transformed_inventory_sha256"],
        "profile_sha256": hashlib.sha256(profile_bytes).hexdigest(),
        "candidate_generation": packaging["generation"],
        "candidate_generation_identity": packaging["generation_identity"],
        "p025_installation_id": record["plan"]["installation_id"],
        "p025_installed_generation": record["plan"]["generation"],
        "p025_ownership_record_sha256": ownership_record_sha256,
        "p026_baseline_sha256": sha256_file(args.root / ".endfield-runtime-baseline.json"),
        "p027_pin_id": pin["pin_id"],
        "p027_pin_registry_revision": pin_registry["revision"],
        "p027_pin_registry_sha256": sha256_file(args.pin_registry),
        "image_sha256": first_hash,
        "image_size": image_size,
        "image_builder_version": version,
        "image_builder_sha256": sha256_file(builder),
        "builder_configuration": ["-noappend", "-reproducible", "-all-root", "-no-xattrs", "-no-exports", "-no-progress", "-no-duplicates", "-mkfs-time=0", "-all-time=0", "-processors=1", "-comp=zstd", "-Xcompression-level=15"],
        "reproduction_comparison": {"performed": True, "byte_equal": True, "first_sha256": first_hash, "second_sha256": second_hash},
        "logical_inventory_entry_count": len(actual),
        "logical_inventory_sha256": digest_json(actual),
        "logical_inventory": actual,
        "mounted_runtime_view": "NOT_RUN",
        "authentic_execution": "NOT_RUN",
    }
    args.evidence.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"image": str(args.output), "image_size": image_size, "image_sha256": first_hash,
                      "logical_inventory_entries": len(actual), "reproducibility": "BYTE_EQUAL"}, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True, help="P025-installed runtime tree")
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--packaging", type=Path, required=True)
    parser.add_argument("--transformed-inventory", type=Path, required=True)
    parser.add_argument("--mksquashfs", type=Path, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--p027-pin-registry", dest="pin_registry", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    build(args)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"SquashFS image construction failed: {error}")
