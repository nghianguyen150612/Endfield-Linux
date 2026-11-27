#!/usr/bin/env python3
"""Small deterministic SquashFS image construction and inventory contract."""
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("squashfs_builder", Path(__file__).with_name("build-wine-squashfs-image.py"))
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class SquashfsBuilderTests(unittest.TestCase):
    def test_inventory_rejects_unsafe_symlinks_and_special_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "root"
            root.mkdir()
            (root / "escape").symlink_to("../../etc/passwd")
            with self.assertRaises(ValueError):
                builder.inventory_tree(root)
            (root / "escape").unlink()
            os.mkfifo(root / "pipe")
            with self.assertRaises(ValueError):
                builder.inventory_tree(root)

    @unittest.skipUnless(os.environ.get("P034_MKSQUASHFS"), "mksquashfs opt-in fixture tool is unavailable")
    def test_image_bytes_and_logical_inventory_are_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output_dir = base / "private"
            output_dir.mkdir(mode=0o700)
            root = base / "root"
            root.mkdir(mode=0o700)
            (root / "opt").mkdir(mode=0o700)
            (root / "opt" / "wine-stable").mkdir(mode=0o700)
            (root / "opt" / "wine-stable" / "bin").mkdir(mode=0o700)
            executable = root / "opt" / "wine-stable" / "bin" / "wine"
            executable.write_bytes(b"harmless native fixture")
            executable.chmod(0o700)
            (root / "opt" / "wine-stable" / "bin" / "wine64").symlink_to("wine")
            inventory = builder.inventory_tree(root)
            inventory_digest = builder.digest_json(inventory)
            artifact = base / builder.ARTIFACT_NAME if hasattr(builder, "ARTIFACT_NAME") else base / "winehq-noble-10.0-dualarch-candidate.tar.gz"
            artifact.write_bytes(b"verified fixture archive")
            artifact_hash = builder.sha256_file(artifact)
            profile = {
                "derived_artifact_sha256": artifact_hash,
                "inventory_sha256": inventory_digest,
                "source_inventory_sha256": "a" * 64,
            }
            profile_path = base / "profile.json"
            profile_path.write_text(json.dumps(profile))
            packaging = {
                "artifact_sha256": artifact_hash,
                "artifact_size": artifact.stat().st_size,
                "source_inventory_sha256": "a" * 64,
                "transformed_inventory_sha256": inventory_digest,
                "source_packages": [{"sha256": "b" * 64}],
                "generation": 1,
                "generation_identity": "c" * 64,
            }
            packaging_path = base / "packaging.json"
            packaging_path.write_text(json.dumps(packaging))
            inventory_path = base / "transformed.json"
            inventory_path.write_text(json.dumps(inventory))
            manifest_sha = "d" * 64
            installation_id = "wine-test-1"
            plan = {
                "installation_id": installation_id,
                "family": "wine",
                "lineage": "upstream-wine",
                "build": "wine-10.0",
                "provider": "endfield-unix-development",
                "project": "p034-disposable",
                "channel": "development",
                "artifact_sha256": artifact_hash,
                "archive_size": artifact.stat().st_size,
                "manifest_sha256": manifest_sha,
                "generation": 1,
                "trust": "development_only",
            }
            record = {"schema_version": 1, "state": "Published", "trust": "development_only", "plan": plan}
            record_path = root / ".endfield-installation.json"
            record_bytes = json.dumps(record).encode()
            record_path.write_bytes(record_bytes)
            baseline = {
                "schema_version": 1,
                "installation_id": installation_id,
                "generation": 1,
                "family": "wine",
                "build": "wine-10.0",
                "artifact_sha256": artifact_hash,
                "manifest_sha256": manifest_sha,
                "trust": "development_only",
                "origin": "verified_installation",
                "inventory_sha256": inventory_digest,
                "entries": inventory,
            }
            (root / ".endfield-runtime-baseline.json").write_text(json.dumps(baseline))
            ownership_digest = hashlib.sha256(record_bytes).hexdigest()
            registry = {
                "schema_version": 2,
                "revision": 1,
                "pins": [{"pin_id": "pin-test", "runtime": {"kind": "managed", "identity": {
                    "installation_id": installation_id,
                    "family": "wine",
                    "lineage": "upstream-wine",
                    "provider": "endfield-unix-development",
                    "project": "p034-disposable",
                    "build": "wine-10.0",
                    "artifact_sha256": artifact_hash,
                    "manifest_sha256": manifest_sha,
                    "generation": 1,
                    "ownership_record_sha256": ownership_digest,
                    "trust": "development_only",
                }}}],
            }
            pin_registry = base / "runtime-pins.json"
            pin_registry.write_text(json.dumps(registry))
            args = type("Args", (), {
                "root": root,
                "artifact": artifact,
                "profile": profile_path,
                "packaging": packaging_path,
                "transformed_inventory": inventory_path,
                "mksquashfs": Path(os.environ["P034_MKSQUASHFS"]),
                "manifest_sha256": manifest_sha,
                "pin_registry": pin_registry,
                "output": output_dir / "runtime.squashfs",
                "evidence": output_dir / "runtime-evidence.json",
            })()
            builder.build(args)
            first_hash = builder.sha256_file(args.output)
            first_bytes = args.output.read_bytes()
            args.output.unlink()
            args.evidence.unlink()
            builder.build(args)
            self.assertEqual(builder.sha256_file(args.output), first_hash)
            self.assertEqual(args.output.read_bytes(), first_bytes)
            evidence = json.loads(args.evidence.read_text())
            self.assertTrue(evidence["reproduction_comparison"]["byte_equal"])
            self.assertEqual(evidence["logical_inventory_sha256"], inventory_digest)
            self.assertEqual(evidence["image_sha256"], first_hash)
            self.assertEqual(evidence["p025_installation_id"], "wine-test-1")
            self.assertEqual(evidence["p025_installed_generation"], 1)
            self.assertEqual(evidence["p027_pin_id"], "pin-test")


if __name__ == "__main__":
    unittest.main()
