#!/bin/sh
# Offline synthetic fixture; public test key, no publisher authenticity claim.
set -eu
binary=${1:-./target/debug/endfield-unix}
fixture=$(pwd)/crates/runtime-verification/tests/fixtures
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
verify() {
  "$binary" diagnose runtime-integrity --manifest "$1" --signature "$fixture/manifest.sig" \
    --artifact "$fixture/runtime.tar.gz" --trust-key "$fixture/public-key.bin" \
    --provider synthetic-provider --project synthetic-project --channel test \
    --family proton --lineage ge-proton --build synthetic-build \
    --artifact-id synthetic-artifact --key-id synthetic-key
}
set +e
verify "$fixture/manifest.json" > "$scratch/report"
result=$?
set -e
cat "$scratch/report"
test "$result" -eq 3
grep -Fq 'Source-integrity status: local match' "$scratch/report"
grep -Fq 'Official publisher authenticity: UNVERIFIED' "$scratch/report"
cp "$fixture/manifest.json" "$scratch/tampered.json"
printf ' ' >> "$scratch/tampered.json"
set +e
verify "$scratch/tampered.json" > "$scratch/tampered-report"
result=$?
set -e
cat "$scratch/tampered-report"
test "$result" -eq 4
