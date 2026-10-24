use std::error::Error;
use std::fmt;

/// Bounded failure categories exposed by the injectable Linux source boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSourceErrorKind {
    NotFound,
    PermissionDenied,
    InvalidUtf8,
    MalformedData,
    TooLarge,
    UnexpectedType,
    Unsupported,
    Other,
}

/// Source failure without paths or operating-system error text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataSourceError {
    pub kind: DataSourceErrorKind,
}

impl DataSourceError {
    pub const fn new(kind: DataSourceErrorKind) -> Self {
        Self { kind }
    }
}

impl fmt::Display for DataSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            DataSourceErrorKind::NotFound => "source was not found",
            DataSourceErrorKind::PermissionDenied => "source access was denied",
            DataSourceErrorKind::InvalidUtf8 => "source was not valid UTF-8",
            DataSourceErrorKind::MalformedData => "source data was malformed",
            DataSourceErrorKind::TooLarge => "source exceeded its size limit",
            DataSourceErrorKind::UnexpectedType => "source had an unexpected file type",
            DataSourceErrorKind::Unsupported => "operation is unsupported on this platform",
            DataSourceErrorKind::Other => "source could not be read",
        })
    }
}

impl Error for DataSourceError {}

/// Stable parser failure categories. The parser never includes source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsReleaseParseError {
    TooLarge,
    InvalidUtf8,
    LineTooLong,
    MalformedAssignment,
    InvalidKey,
    InvalidValue,
    InvalidEscape,
    UnterminatedQuote,
    TrailingCharacters,
    ValueTooLong,
    InvalidSelectedField,
    TooManyIdLikeValues,
}

impl fmt::Display for OsReleaseParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "os-release file exceeded its size limit",
            Self::InvalidUtf8 => "os-release file was not valid UTF-8",
            Self::LineTooLong => "os-release line exceeded its size limit",
            Self::MalformedAssignment => "os-release contained a malformed assignment",
            Self::InvalidKey => "os-release contained an invalid key",
            Self::InvalidValue => "os-release contained an invalid value",
            Self::InvalidEscape => "os-release contained an unsupported escape",
            Self::UnterminatedQuote => "os-release contained an unterminated quoted value",
            Self::TrailingCharacters => "os-release contained characters after a quoted value",
            Self::ValueTooLong => "os-release value exceeded its size limit",
            Self::InvalidSelectedField => "os-release contained an invalid distribution field",
            Self::TooManyIdLikeValues => "os-release contained too many ID_LIKE values",
        })
    }
}

impl Error for OsReleaseParseError {}
