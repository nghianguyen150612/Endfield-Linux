//! Explicit allowlisted UMU environment preparation.

use endfield_unix_runtime::{
    EnvironmentChange, EnvironmentName, SensitiveValueKind, ValueSensitivity,
};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

pub const MAX_GAME_ID_BYTES: usize = 128;
pub const MAX_PREFIX_PATH_BYTES: usize = 4096;
pub const NEUTRAL_GAME_ID: &str = "umu-default";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Egs,
    Gog,
    BattleNet,
    Amazon,
    Humble,
    Itchio,
    Ubisoft,
}
impl Store {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Egs => "egs",
            Self::Gog => "gog",
            Self::BattleNet => "battlenet",
            Self::Amazon => "amazon",
            Self::Humble => "humble",
            Self::Itchio => "itchio",
            Self::Ubisoft => "ubisoft",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrefixSelection {
    path: PathBuf,
    ownership: endfield_unix_runtime::PrefixOwnership,
}
impl std::fmt::Debug for PrefixSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrefixSelection")
            .field("path", &"[redacted]")
            .field("ownership", &self.ownership)
            .finish()
    }
}
impl PrefixSelection {
    pub fn new(
        path: impl Into<PathBuf>,
        ownership: endfield_unix_runtime::PrefixOwnership,
    ) -> Result<Self, EnvironmentError> {
        let path = path.into();
        validate_absolute(&path, MAX_PREFIX_PATH_BYTES)?;
        if !std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(EnvironmentError::InvalidPath);
        }
        if ownership == endfield_unix_runtime::PrefixOwnership::Unknown {
            return Err(EnvironmentError::UnknownOwnership);
        }
        Ok(Self { path, ownership })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub const fn ownership(&self) -> endfield_unix_runtime::PrefixOwnership {
        self.ownership
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentError {
    MissingProtonPath,
    InvalidPath,
    MissingPrefix,
    UnknownOwnership,
    InvalidGameId,
}
impl std::fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingProtonPath => "an explicit Proton directory is required",
            Self::InvalidPath => "path must be absolute, bounded, and contain no parent traversal",
            Self::MissingPrefix => "an explicit prefix selection is required",
            Self::UnknownOwnership => "prefix ownership must be classified",
            Self::InvalidGameId => "game identifier is invalid",
        })
    }
}
impl std::error::Error for EnvironmentError {}

pub fn validate_absolute(path: &Path, maximum: usize) -> Result<(), EnvironmentError> {
    if !path.is_absolute()
        || path.as_os_str().len() > maximum
        || path.components().any(|c| c == Component::ParentDir)
    {
        return Err(EnvironmentError::InvalidPath);
    }
    Ok(())
}

pub fn validate_game_id(value: &str) -> Result<(), EnvironmentError> {
    if value.is_empty()
        || value.len() > MAX_GAME_ID_BYTES
        || value
            .bytes()
            .any(|b| b == 0 || b.is_ascii_control() || b == b'/' || b == b'\\')
    {
        return Err(EnvironmentError::InvalidGameId);
    }
    Ok(())
}

/// Values are sorted by P021's `LaunchPlan` constructor. Paths are marked private
/// so its dry-run renderer redacts them. No inherited UMU control is preserved.
pub fn build_environment(
    proton: Option<&Path>,
    prefix: Option<&PrefixSelection>,
    game_id: Option<&str>,
    store: Option<Store>,
) -> Result<Vec<EnvironmentChange>, EnvironmentError> {
    let proton = proton.ok_or(EnvironmentError::MissingProtonPath)?;
    validate_absolute(proton, MAX_PREFIX_PATH_BYTES)?;
    let prefix = prefix.ok_or(EnvironmentError::MissingPrefix)?;
    let game_id = game_id.unwrap_or(NEUTRAL_GAME_ID);
    validate_game_id(game_id)?;
    let sensitive = ValueSensitivity::Sensitive(SensitiveValueKind::SensitivePath);
    let mut changes = vec![
        set("PROTONPATH", proton.as_os_str().to_owned(), sensitive),
        set("WINEPREFIX", prefix.path.as_os_str().to_owned(), sensitive),
        set("GAMEID", OsString::from(game_id), ValueSensitivity::Public),
        set(
            "PROTON_VERB",
            OsString::from("waitforexitandrun"),
            ValueSensitivity::Public,
        ),
        set(
            "UMU_RUNTIME_UPDATE",
            OsString::from("0"),
            ValueSensitivity::Public,
        ),
        set(
            "PROTONFIXES_DISABLE",
            OsString::from("1"),
            ValueSensitivity::Public,
        ),
    ];
    if let Some(store) = store {
        changes.push(set(
            "STORE",
            OsString::from(store.as_str()),
            ValueSensitivity::Public,
        ));
    } else {
        changes.push(EnvironmentChange::remove(name("STORE")));
    }
    // Remove known behavior controls from inherited ambient environment. UMU_ID
    // is not an upstream variable and is deliberately neither set nor forwarded.
    for key in [
        "UMU_NO_PROTON",
        "UMU_FOLDERS_PATH",
        "UMU_LOG",
        "PROTON_LOG",
        "UMU_HTTP_TIMEOUT",
        "UMU_HTTP_RETRIES",
        "STEAM_COMPAT_LIBRARY_PATHS",
        "PRESSURE_VESSEL_FILESYSTEMS_RO",
        "PRESSURE_VESSEL_FILESYSTEMS_RW",
        "SteamGameId",
    ] {
        changes.push(EnvironmentChange::remove(name(key)));
    }
    changes.sort_by(|left, right| left.name().cmp(right.name()));
    if changes
        .windows(2)
        .any(|pair| pair[0].name() == pair[1].name())
    {
        return Err(EnvironmentError::InvalidPath);
    }
    Ok(changes)
}
fn name(value: &str) -> EnvironmentName {
    EnvironmentName::new(value).expect("fixed environment name")
}
fn set(name: &str, value: OsString, sensitivity: ValueSensitivity) -> EnvironmentChange {
    EnvironmentChange::set(
        EnvironmentName::new(name).expect("fixed environment name"),
        value,
        sensitivity,
    )
    .expect("validated environment value")
}
