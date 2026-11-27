#!/usr/bin/env python3
"""Bounded tests for the offline P024 development manifest tool."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

TOOL = Path(__file__).with_name("sign-wine-runtime-manifest.py")
SPEC = importlib.util.spec_from_file_location("manifest_tool", TOOL)
tool = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tool)


class ManifestToolTests(unittest.TestCase):
    def test_generation_binds_artifact_profile_and_transformation_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / tool.ARTIFACT_NAME
            artifact.write_bytes(b"disposable fixture candidate")
            source = root / "source.json"
            source.write_text(json.dumps({"entries": []}))
            transformed = root / "transformed.json"
            transformed.write_text("[]")
            source_digest = tool.json_digest([])
            transformed_digest = tool.json_digest([])
            packaging = {
                "artifact_sha256": tool.file_sha256(artifact),
                "artifact_size": artifact.stat().st_size,
                "source_inventory_sha256": source_digest,
                "transformed_inventory_sha256": transformed_digest,
                "source_evidence_sha256": "a" * 64,
                "source_packages": [{"sha256": "b" * 64}],
            }
            profile = {
                "source_inventory_sha256": source_digest,
                "derived_artifact_sha256": tool.file_sha256(artifact),
            }
            profile_path = root / "profile.json"
            profile_path.write_text(json.dumps(profile))
            packaging_path = root / "packaging.json"
            packaging_path.write_text(json.dumps(packaging))
            source_inventory_path = root / "source-inventory.json"
            source_inventory_path.write_text(json.dumps({"entries": []}))
            transformed_inventory_path = root / "transformed-inventory.json"
            transformed_inventory_path.write_text("[]")
            evidence_path = root / "revalidation.json"
            evidence_path.write_text(json.dumps({"packaging": packaging}))
            private_key = root / "dev-key.pem"
            subprocess.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(private_key)], check=True, capture_output=True)
            private_key.chmod(0o600)
            manifest_path = root / "manifest.json"
            signature_path = root / "manifest.sig"
            receipt_path = root / "receipt.json"
            public_key_path = root / "dev-key.bin"
            args = type("Args", (), {
                "artifact": artifact,
                "profile": profile_path,
                "packaging": packaging_path,
                "source_inventory": source_inventory_path,
                "transformed_inventory": transformed_inventory_path,
                "revalidation_evidence": evidence_path,
                "private_key": private_key,
                "key_id": "dev-fixture-key",
                "manifest_id": "fixture-development-manifest",
                "manifest_version": 1,
                "issued_at": "2026-11-25T00:00:00Z",
                "expires_at": "2026-12-25T00:00:00Z",
                "manifest": manifest_path,
                "signature": signature_path,
                "receipt": receipt_path,
                "public_key": public_key_path,
            })()
            tool.create(args)
            manifest_bytes = manifest_path.read_bytes()
            manifest = json.loads(manifest_bytes)
            self.assertEqual(manifest["channel"], "development")
            self.assertEqual(manifest["provider_id"], "endfield-unix-development")
            self.assertEqual(manifest["runtime_build"], "wine-10.0")
            self.assertEqual(manifest["artifacts"][0]["sha256"], tool.file_sha256(artifact))
            self.assertEqual(manifest["artifacts"][0]["wine_qualification_profile_sha256"], hashlib.sha256(profile_path.read_bytes()).hexdigest())
            receipt = json.loads(receipt_path.read_text())
            self.assertEqual(public_key_path.stat().st_size, 32)
            self.assertEqual(receipt["manifest_sha256"], hashlib.sha256(manifest_bytes).hexdigest())
            public_key = root / "dev-key.pub"
            subprocess.run(["openssl", "pkey", "-in", str(private_key), "-pubout", "-out", str(public_key)], check=True, capture_output=True)
            message = root / "message"
            message.write_bytes(tool.DOMAIN + manifest_bytes)
            subprocess.run(["openssl", "pkeyutl", "-verify", "-rawin", "-pubin", "-inkey", str(public_key), "-in", str(message), "-sigfile", str(signature_path)], check=True, capture_output=True)
            with self.assertRaises(ValueError):
                tool.create(args)

            # A detached transformation receipt cannot be silently replaced
            # while keeping the package record that the manifest was built from.
            for path in (manifest_path, signature_path, receipt_path, public_key_path):
                path.unlink()
            evidence_path.write_text(json.dumps({"packaging": {"artifact_sha256": "0" * 64}}))
            with self.assertRaises(ValueError):
                tool.create(args)

    def test_non_development_key_id_is_refused_before_signing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            private_key = root / "dev-key.pem"
            subprocess.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(private_key)], check=True, capture_output=True)
            private_key.chmod(0o600)
            args = type("Args", (), {"private_key": private_key, "key_id": "publisher-key",
                                     "manifest": root / "manifest.json", "signature": root / "manifest.sig",
                                     "receipt": root / "receipt.json", "public_key": root / "dev-key.bin"})()
            with self.assertRaises(ValueError):
                tool.create(args)


if __name__ == "__main__":
    unittest.main()
