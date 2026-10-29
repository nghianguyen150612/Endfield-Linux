//! Offline, single-key runtime manifest verification. No publisher keys are bundled.
#![forbid(unsafe_code)]
pub mod artifact;
pub mod manifest;
pub mod trust;
pub mod verification;
pub use artifact::*;
pub use manifest::*;
pub use trust::*;
pub use verification::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Schema,
    UnsupportedAlgorithm,
    Signature,
    UnknownKey,
    RevokedKey,
    UntrustedKey,
    Scope,
    TimeUnknown,
    Expired,
    FutureIssued,
    Rollback,
    CheckpointConflict,
    Identity,
    MissingDigest,
    DigestMismatch,
    SizeMismatch,
    TooLarge,
    FileUnavailable,
    PermissionDenied,
    FileType,
    ChangedDuringRead,
    ReadFailure,
    Contract,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<endfield_unix_runtime::ContractError> for Error {
    fn from(_: endfield_unix_runtime::ContractError) -> Self {
        Self::Contract
    }
}
pub(crate) fn io_error(e: std::io::Error) -> Error {
    match e.kind() {
        std::io::ErrorKind::NotFound => Error::FileUnavailable,
        std::io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        _ => Error::ReadFailure,
    }
}
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
