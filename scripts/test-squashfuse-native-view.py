#!/usr/bin/env python3
"""Explicit host test for sealed SquashFS FUSE mounting and native handoff."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import fcntl
import json


class SquashfuseNativeViewTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("P034_SQUASHFUSE") and os.environ.get("P034_MKSQUASHFS"),
                         "explicit SquashFS mount tools are unavailable")
    def test_sealed_read_only_view_preserves_native_launcher_preloader_helper_chain(self):
        squashfuse = Path(os.environ["P034_SQUASHFUSE"])
        mksquashfs = Path(os.environ["P034_MKSQUASHFS"])
        with tempfile.TemporaryDirectory(prefix="p034-native-view-") as temporary:
            base = Path(temporary)
            root = base / "tree"
            bin_dir = root / "opt/wine-stable/bin"
            helper_dir = bin_dir / "helpers"
            library_dir = root / "opt/wine-stable/lib"
            helper_dir.mkdir(parents=True)
            library_dir.mkdir(parents=True)
            (library_dir / "module.dat").write_text("fixture-module-v1\n")
            sources = {
                "wine.c": r'''#define _GNU_SOURCE
#include <libgen.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  (void)argc; (void)argv;
  char self[PATH_MAX]; ssize_t n = readlink("/proc/self/exe", self, sizeof(self)-1);
  if (n < 0) return 10;
  self[n] = 0;
  char *base = strrchr(self, '/'); if (!base || strcmp(base+1, "wine")) return 11;
  *base = 0;
  char *preloader = NULL; char *main_path = NULL;
  if (asprintf(&preloader, "%s/wine-preloader", self) < 0) return 12;
  if (asprintf(&main_path, "%s/wine-main", self) < 0) return 13;
  char *next[] = {"wine", main_path, NULL};
  execv(preloader, next); return 14;
}''',
                "preloader.c": r'''#define _GNU_SOURCE
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc < 2) return 20;
  int fd = open(argv[1], O_RDONLY);
  if (fd < 0) return 21;
  struct stat st;
  if (fstat(fd, &st) || !S_ISREG(st.st_mode)) return 22;
  close(fd);
  execv(argv[1], &argv[1]); return 23;
}''',
                "main.c": r'''#define _GNU_SOURCE
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int main(void) {
  char self[PATH_MAX]; ssize_t n = readlink("/proc/self/exe", self, sizeof(self)-1);
  if (n < 0) return 30;
  self[n] = 0;
  char *slash = strrchr(self, '/');
  if (!slash) return 31;
  *slash = 0;
  char *module = NULL;
  if (asprintf(&module, "%s/../lib/module.dat", self) < 0) return 32;
  FILE *f = fopen(module, "r");
  if (!f) return 33;
  char data[64];
  if (!fgets(data, sizeof(data), f)) return 34;
  fclose(f);
  char *helper = NULL;
  if (asprintf(&helper, "%s/helpers/worker", self) < 0) return 35;
  char *args[] = {helper, module, data, NULL}; execv(helper, args); return 34;
}''',
                "helper.c": r'''#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
  if (argc != 3) return 40;
  FILE *f = fopen(argv[1], "r");
  if (!f) return 41;
  char data[64];
  if (!fgets(data, sizeof(data), f)) return 42;
  fclose(f);
  if (strcmp(data, argv[2])) return 43;
  printf("HANDOFF_OK module=%s data=%s", argv[1], data); return 0;
}''',
            }
            gcc = os.environ.get("CC", "gcc")
            for name, source in sources.items():
                path = base / name
                path.write_text(source)
                output = bin_dir / {"wine.c": "wine", "preloader.c": "wine-preloader",
                                    "main.c": "wine-main", "helper.c": "helpers/worker"}[name]
                subprocess.run([gcc, "-std=c17", "-O2", "-Wall", "-Wextra", "-Werror", str(path), "-o", str(output)],
                               check=True, capture_output=True)
                output.chmod(0o700)
            descriptor_source = base / "descriptor-runner.c"
            descriptor_source.write_text(r'''#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <unistd.h>
extern char **environ;
int main(int argc, char **argv) {
  if (argc != 2) return 60;
  int fd = open(argv[1], O_PATH | O_CLOEXEC);
  if (fd < 0) return 61;
  char *child_argv[] = {"wine", NULL};
  syscall(SYS_execveat, fd, "", child_argv, environ, AT_EMPTY_PATH);
  perror("execveat"); return errno == 0 ? 62 : 63;
}''')
            descriptor_runner = base / "descriptor-runner"
            subprocess.run([gcc, "-std=c17", "-O2", "-Wall", "-Wextra", "-Werror",
                            str(descriptor_source), "-o", str(descriptor_runner)], check=True, capture_output=True)
            descriptor_runner.chmod(0o700)
            image = base / "fixture.squashfs"
            subprocess.run([str(mksquashfs), str(root), str(image), "-noappend", "-reproducible",
                            "-all-root", "-no-xattrs", "-no-exports", "-no-progress", "-processors", "1",
                            "-comp", "zstd", "-Xcompression-level", "15", "-mkfs-time", "0", "-all-time", "0"],
                           check=True, capture_output=True)
            image_bytes = image.read_bytes()
            backing = os.memfd_create("p034-fixture-image", os.MFD_ALLOW_SEALING)
            os.write(backing, image_bytes)
            del image_bytes
            seals = (getattr(fcntl, "F_SEAL_WRITE", 0x0008)
                     | getattr(fcntl, "F_SEAL_GROW", 0x0004)
                     | getattr(fcntl, "F_SEAL_SHRINK", 0x0002)
                     | getattr(fcntl, "F_SEAL_SEAL", 0x0001))
            fcntl.fcntl(backing, getattr(fcntl, "F_ADD_SEALS", 1033), seals)
            os.set_inheritable(backing, True)
            try:
                with self.assertRaises(PermissionError):
                    os.pwrite(backing, b"changed", 0)
                alias = os.dup(backing)
                try:
                    with self.assertRaises(PermissionError):
                        os.pwrite(alias, b"changed", 0)
                finally:
                    os.close(alias)
                mountpoint = base / "mnt"
                mountpoint.mkdir()
                script = base / "mount-and-run.sh"
                script.write_text(r'''#!/bin/sh
set -eu
mountpoint="$P034_MOUNTPOINT"
cleanup() { /usr/bin/fusermount3 -u "$mountpoint" >/dev/null 2>&1 || true; }
trap cleanup EXIT
"$P034_SQUASHFUSE" -o ro,nosuid,nodev "/proc/self/fd/$P034_BACKING_FD" "$mountpoint"
record=$(awk -v p="$mountpoint" '$5 == p { print $6; found=1 } END { if (!found) exit 1 }' /proc/self/mountinfo)
case ",$record," in *,ro,*nosuid,*nodev,*) ;; *) echo "unexpected mount options: $record" >&2; exit 51 ;; esac
echo "MOUNT_OPTIONS=$record"
for entry in "$mountpoint/opt/wine-stable/bin/wine" \
             "$mountpoint/opt/wine-stable/bin/wine-preloader" \
             "$mountpoint/opt/wine-stable/bin/wine-main" \
             "$mountpoint/opt/wine-stable/bin/helpers/worker" \
             "$mountpoint/opt/wine-stable/lib/module.dat"; do
  if printf x >> "$entry" 2>/dev/null; then exit 52; fi
done
for entry in "$mountpoint/opt/wine-stable/bin/wine" \
             "$mountpoint/opt/wine-stable/bin/wine-preloader" \
             "$mountpoint/opt/wine-stable/bin/wine-main" \
             "$mountpoint/opt/wine-stable/bin/helpers/worker" \
             "$mountpoint/opt/wine-stable/lib/module.dat"; do
  if mv "$entry" "$entry.replaced" 2>/dev/null; then exit 53; fi
  if ln -sfn /tmp/redirect "$entry" 2>/dev/null; then exit 54; fi
done
if mv "$mountpoint" "${mountpoint}.replaced" 2>/dev/null; then exit 55; fi
"$P034_DESCRIPTOR_RUNNER" "$mountpoint/opt/wine-stable/bin/wine"
cleanup
trap - EXIT
''')
                script.chmod(0o700)
                before = Path("/proc/self/mountinfo").read_text()
                env = dict(os.environ)
                env.update({"P034_MOUNTPOINT": str(mountpoint), "P034_SQUASHFUSE": str(squashfuse),
                            "P034_BACKING_FD": str(backing),
                            "P034_DESCRIPTOR_RUNNER": str(descriptor_runner)})
                command = ["/usr/bin/unshare", "--user", "--map-root-user", "--mount", "--propagation", "private",
                           "/bin/sh", str(script)]
                result = subprocess.run(command, env=env, pass_fds=(backing,), capture_output=True,
                                        text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("HANDOFF_OK", result.stdout)
                self.assertIn("MOUNT_OPTIONS=", result.stdout)
                after = Path("/proc/self/mountinfo").read_text()
                self.assertEqual(before, after)
                self.assertNotIn(str(mountpoint), after)
                evidence_path = os.environ.get("P034_NATIVE_VIEW_EVIDENCE")
                if evidence_path:
                    version_result = subprocess.run([str(squashfuse), "-V"], capture_output=True, text=True)
                    version = (version_result.stdout + version_result.stderr).strip().splitlines()[0]
                    report = {
                        "schema_version": 1,
                        "state": "NATIVE_FIXTURE_VIEW_QUALIFIED",
                        "provider": version,
                        "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
                        "image_size": image.stat().st_size,
                        "backing": "sealed_memfd",
                        "backing_write_attempt": "rejected_by_kernel_seals",
                        "mount_options": result.stdout.split("MOUNT_OPTIONS=", 1)[1].splitlines()[0],
                        "path_handoff": ["descriptor-bound launcher", "/proc/self/exe discovery", "sibling preloader", "preloader opened main executable by pathname", "relative module lookup", "helper exec", "process completion"],
                        "runtime_replacement_and_write_attempts": "rejected_for_launcher_preloader_main_helper_module_and_root",
                        "parent_namespace_unchanged": True,
                        "mount_cleanup": "unmounted_before_namespace_owner_exit",
                        "wine_executed": False,
                    }
                    Path(evidence_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
            finally:
                os.close(backing)


if __name__ == "__main__":
    unittest.main()
