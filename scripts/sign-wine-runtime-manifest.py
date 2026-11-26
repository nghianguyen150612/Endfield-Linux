#!/usr/bin/env python3
"""Create a P024 Wine manifest with an explicitly supplied Ed25519 signing key.

This offline development tool never enrolls its key. Its development namespace
and channel are fixed so output cannot be mistaken for a production manifest.
OpenSSL performs the Ed25519 operation; P024 remains the verification authority.
"""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile

DOMAIN = b"ENDFIELD-UNIX-RUNTIME-MANIFEST-V1\0"
ARTIFACT_NAME = "winehq-noble-10.0-dualarch-candidate.tar.gz"


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def json_digest(value):
    encoded = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(encoded).hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def check_inputs(artifact, profile_path, packaging_path, source_inventory_path,
                 transformed_inventory_path, evidence_path):
    profile = read_json(profile_path)
    packaging = read_json(packaging_path)
    source = read_json(source_inventory_path)
    transformed = read_json(transformed_inventory_path)
    evidence = read_json(evidence_path)
    artifact_hash = file_sha256(artifact)
    if artifact.name != ARTIFACT_NAME:
        raise ValueError("unexpected candidate filename")
    if artifact.stat().st_size != packaging.get("artifact_size") or artifact_hash != packaging.get("artifact_sha256"):
        raise ValueError("candidate bytes do not match packaging evidence")
    if evidence.get("packaging") != packaging:
        raise ValueError("revalidation evidence does not bind the packaging record")
    source_hash = json_digest(source.get("entries"))
    transformed_hash = json_digest(transformed)
    if (source_hash != packaging.get("source_inventory_sha256")
            or source_hash != profile.get("source_inventory_sha256")
            or transformed_hash != packaging.get("transformed_inventory_sha256")
            or profile.get("derived_artifact_sha256") != artifact_hash):
        raise ValueError("source, transformed inventory, profile, or packaging identity mismatch")
    return artifact_hash, profile, packaging, source, transformed, evidence


def timestamp(value):
    try:
        parsed = dt.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)
    except ValueError as error:
        raise ValueError("timestamps must use YYYY-MM-DDTHH:MM:SSZ") from error
    return parsed


def create(args):
    if not args.key_id.startswith("dev-"):
        raise ValueError("development signer key IDs must use the reserved dev- prefix")
    key_info = args.private_key.lstat()
    if (not stat.S_ISREG(key_info.st_mode) or key_info.st_uid != os.getuid()
            or key_info.st_mode & 0o077):
        raise ValueError("private key must be an owner-controlled regular file with mode 0600 or stricter")
    for output in (args.manifest, args.signature, args.receipt, args.public_key):
        if output.exists():
            raise ValueError("refusing to overwrite an output file")
    issued = timestamp(args.issued_at)
    expires = timestamp(args.expires_at)
    if issued >= expires:
        raise ValueError("manifest expiry must be later than issue time")

    artifact_hash, profile, packaging, source, transformed, evidence = check_inputs(
        args.artifact, args.profile, args.packaging, args.source_inventory,
        args.transformed_inventory, args.revalidation_evidence)
    profile_bytes = args.profile.read_bytes()
    profile_hash = hashlib.sha256(profile_bytes).hexdigest()
    manifest = {
        "schema_version": 2,
        "manifest_id": args.manifest_id,
        "manifest_version": args.manifest_version,
        "provider_id": "endfield-unix-development",
        "project_id": "p034-disposable",
        "channel": "development",
        "runtime_family": "wine",
        "runtime_lineage": "upstream-wine",
        "runtime_build": "wine-10.0",
        "key_id": args.key_id,
        "signature_algorithm": "ed25519",
        "issued_at": args.issued_at,
        "expires_at": args.expires_at,
        "artifacts": [{
            "artifact_id": "winehq-dualarch-candidate",
            "filename": ARTIFACT_NAME,
            "size_bytes": args.artifact.stat().st_size,
            "sha256": artifact_hash,
            "wine_qualification_profile_sha256": profile_hash,
        }],
    }
    manifest_bytes = (json.dumps(manifest, separators=(",", ":"), ensure_ascii=False) + "\n").encode()
    if len(manifest_bytes) > 262144:
        raise ValueError("manifest exceeds the P024 size bound")
    with tempfile.TemporaryDirectory(prefix="endfield-manifest-sign-") as temporary:
        message = Path(temporary) / "signing-input"
        signature = Path(temporary) / "signature"
        public_pem = Path(temporary) / "public.pem"
        public_der = Path(temporary) / "public.der"
        message.write_bytes(DOMAIN + manifest_bytes)
        subprocess.run([
            "openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(args.private_key),
            "-in", str(message), "-out", str(signature),
        ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        subprocess.run([
            "openssl", "pkey", "-in", str(args.private_key), "-pubout", "-out", str(public_pem),
        ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        subprocess.run([
            "openssl", "pkey", "-pubin", "-in", str(public_pem), "-outform", "DER", "-out", str(public_der),
        ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        signature_bytes = signature.read_bytes()
        der = public_der.read_bytes()
    if len(signature_bytes) != 64:
        raise ValueError("signer did not return an Ed25519 signature")
    ed25519_spki_prefix = bytes.fromhex("302a300506032b6570032100")
    if len(der) != len(ed25519_spki_prefix) + 32 or not der.startswith(ed25519_spki_prefix):
        raise ValueError("signing key is not Ed25519")
    public_key_bytes = der[len(ed25519_spki_prefix):]

    receipt = {
        "schema_version": 1,
        "authority_scope": "development_only_disposable",
        "manifest_id": args.manifest_id,
        "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "signature_sha256": hashlib.sha256(signature_bytes).hexdigest(),
        "public_key_sha256": hashlib.sha256(public_key_bytes).hexdigest(),
        "artifact_sha256": artifact_hash,
        "artifact_size": args.artifact.stat().st_size,
        "profile_sha256": profile_hash,
        "packaging_record_sha256": file_sha256(args.packaging),
        "source_inventory_sha256": packaging["source_inventory_sha256"],
        "transformed_inventory_sha256": packaging["transformed_inventory_sha256"],
        "source_evidence_sha256": packaging["source_evidence_sha256"],
        "source_package_sha256": [item["sha256"] for item in packaging["source_packages"]],
        "revalidation_evidence_sha256": file_sha256(args.revalidation_evidence),
    }
    args.manifest.write_bytes(manifest_bytes)
    args.signature.write_bytes(signature_bytes)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    args.public_key.write_bytes(public_key_bytes)
    print(json.dumps({
        "manifest": str(args.manifest),
        "manifest_sha256": receipt["manifest_sha256"],
        "signature": str(args.signature),
        "public_key": str(args.public_key),
        "receipt": str(args.receipt),
        "authority_scope": receipt["authority_scope"],
        "artifact_sha256": artifact_hash,
    }, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--packaging", type=Path, required=True)
    parser.add_argument("--source-inventory", type=Path, required=True)
    parser.add_argument("--transformed-inventory", type=Path, required=True)
    parser.add_argument("--revalidation-evidence", type=Path, required=True)
    parser.add_argument("--private-key", type=Path, required=True)
    parser.add_argument("--key-id", required=True)
    parser.add_argument("--manifest-id", required=True)
    parser.add_argument("--manifest-version", type=int, required=True)
    parser.add_argument("--issued-at", required=True)
    parser.add_argument("--expires-at", required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--signature", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--public-key", type=Path, required=True)
    args = parser.parse_args()
    create(args)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"manifest generation failed: {error}")
