# Runtime manifest authority enrollment

P024 authenticates the exact manifest bytes with Ed25519 over
`ENDFIELD-UNIX-RUNTIME-MANIFEST-V1\0` followed by those bytes. The verifier
does not infer authority from a valid signature. It requires a caller-supplied
`TrustAnchor` with an exact key ID, provider/project/channel/family/lineage
scope, provenance, revocation status, and optional validity window. Expiry,
trusted time, and rollback checkpoints remain separate checks.

WineHQ's OpenPGP repository signer authenticates signed repository metadata and
package digests. It is not the Wine source signer, an Endfield-Unix P024 signer,
a production manifest authority, an installation owner, or execution approval.
The local package transformation and its evidence remain distinct from all of
those identities.

Synthetic contract fixtures use a separate `SyntheticFixtureOnly` provenance,
`fixture-` key IDs, and the `endfield-unix-fixture/synthetic-contracts/fixture`
namespace. That provenance cannot create checkpoints or P025 installations.

## Disposable development authority

The P034 development namespace is reserved by the P024 verifier:

| Field | Required value |
| --- | --- |
| Key ID | Prefix `dev-` |
| Provider | `endfield-unix-development` |
| Project | `p034-disposable` |
| Channel | `development` |
| Runtime family / lineage | `wine` / `upstream-wine` |

A `DevelopmentOnly` anchor outside that scope is rejected. A `dev-` key or a
manifest in the reserved namespace is rejected if an anchor labels it as a
publisher or user-pinned key. A development verification cannot produce a
publisher checkpoint. P025 installation approval is available only through
the explicit `approve_development` method and stores `development_only` in the
installation record and P026 baseline. P027 preserves that provenance. Existing
initialization and launch gates still require independently authenticated
publisher provenance.

`scripts/sign-wine-runtime-manifest.py` makes a schema-v2 manifest from the
actual candidate, profile, packaging record, source inventory, transformed
inventory, and revalidation evidence. It checks their digest relationships,
uses OpenSSL's Ed25519 implementation, emits the 32-byte public key, detached
64-byte signature, and a versioned receipt bound to the exact manifest digest.
The tool requires an explicitly supplied private PEM with owner-only
permissions. It never creates or enrolls keys. Keep all private material in
private disposable state, outside Git, CI artifacts, issue/PR text, and release
systems; discard it after the isolated verification run.

The development anchor is constructed directly by the opt-in test and never
written to normal application trust settings. The success state means only
that a development key signed the exact real candidate manifest and the P024
path verified it under development scope. It is not publisher authentication.

## Production enrollment procedure

No production authority is enrolled by this repository change. Production
enrollment is an operator-controlled action and requires all of the following:

1. Approve the authority identity and raw 32-byte Ed25519 public key through an
   independent, authenticated channel. A signature alone is not enrollment.
2. Record the exact permitted provider, project, channel, family, and lineage.
   Do not use a development key ID or the reserved development namespace.
3. Establish key validity start/end times, trusted-time handling, revocation
   ownership, and a durable rollback checkpoint for that exact scope.
4. Verify the key fingerprint, manifest schema, signature, expiry, artifact
   digest/size, profile digest, provenance receipt, and source/transformation
   evidence independently before updating the production trust store.
5. Retain the enrollment decision, public-key fingerprint, scope, validity,
   checkpoint, and revocation path in the operator's authenticated records.

Enrollment must not be inferred from WineHQ's OpenPGP signature, a user pin, a
development test, or a self-signed Endfield-Unix receipt. Production authority
availability remains `false` until an operator performs and records this
procedure in the production trust store.
