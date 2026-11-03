//! P032 read-only source reports, sharing the canonical game command tree.
use clap::Subcommand;
use endfield_unix_game_import::{
    GameImportError, GameInstallationId, GameInstallationRegistry, GameInstallationState,
};
use endfield_unix_game_sources::{
    self as sources, InstalledSourceEvidence, SourceError, SourceVerificationStatus,
};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum Operation {
    /// Show verified official URL, capabilities and manual installation guidance.
    Show {
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Recheck built-in evidence policy offline; does not probe live availability.
    CheckCatalog {
        #[arg(long)]
        json: bool,
    },
    /// Inspect only an explicit Steam library root or appmanifest_4732690.acf.
    Inspect {
        #[arg(long)]
        id: String,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Reopen the exact prior Steam location and compare its observation token.
    Revalidate {
        #[arg(long)]
        id: String,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        expected_observation: String,
        #[arg(long)]
        json: bool,
    },
    /// Preview a source label or local Steam observation for an exact P031 ID; never persist.
    PlanAssociation {
        #[arg(long)]
        game_id: String,
        #[arg(long)]
        source_id: String,
        /// Optional explicit Steam library/manifest; no automatic search.
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Revalidate one P031 location and report that no source association is stored.
    Validate {
        #[arg(long)]
        game_id: String,
        #[arg(long)]
        json: bool,
    },
}
impl Operation {
    fn json(&self) -> bool {
        match self {
            Self::Show { json, .. }
            | Self::CheckCatalog { json }
            | Self::Inspect { json, .. }
            | Self::Revalidate { json, .. }
            | Self::PlanAssociation { json, .. }
            | Self::Validate { json, .. } => *json,
        }
    }
}
fn source_observation(
    id: &str,
    path: &std::path::Path,
    expected: Option<&str>,
) -> Result<InstalledSourceEvidence, SourceError> {
    sources::source(id)?;
    if id != "steam" {
        return Err(SourceError::Unsupported);
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(token) = expected {
            sources::revalidate_steam(path, token)
        } else {
            sources::inspect_steam(path)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, expected);
        Err(SourceError::Unsupported)
    }
}
fn registry() -> Result<GameInstallationRegistry, GameImportError> {
    let paths = endfield_unix_storage::AppPaths::discover()
        .map_err(|_| GameImportError::StorageUnavailable)?;
    Ok(GameInstallationRegistry::for_app(&paths))
}
fn import_error(error: GameImportError) -> (u8, Value) {
    (
        error.exit_status(),
        json!({"schema_version":1,"error":{"code":error.code(),"message":error.to_string()}}),
    )
}
fn source_error(error: SourceError) -> (u8, Value) {
    (
        error.exit_status(),
        json!({"schema_version":1,"error":{"code":error.code(),"message":error.to_string()}}),
    )
}
fn result(operation: Operation) -> Result<(u8, Value), (u8, Value)> {
    match operation {
        Operation::Show { id, .. } => Ok((
            0,
            json!({"schema_version":1,"source":sources::source(&id).map_err(source_error)?}),
        )),
        Operation::CheckCatalog { .. } => {
            let catalog = sources::catalog();
            sources::validate_catalog(&catalog).map_err(source_error)?;
            Ok((
                0,
                json!({"schema_version":1,"catalog_revision":catalog.revision,"built_in_evidence":"verified","live_availability":"unknown","network_performed":false}),
            ))
        }
        Operation::Inspect { id, path, .. } => Ok((
            0,
            json!(source_observation(&id, &path, None).map_err(source_error)?),
        )),
        Operation::Revalidate {
            id,
            path,
            expected_observation,
            ..
        } => {
            let value = source_observation(&id, &path, Some(&expected_observation))
                .map_err(source_error)?;
            Ok((
                if value.status == SourceVerificationStatus::Changed {
                    4
                } else {
                    0
                },
                json!(value),
            ))
        }
        Operation::PlanAssociation {
            game_id,
            source_id,
            path,
            ..
        } => {
            let source = sources::source(&source_id).map_err(source_error)?;
            let id = GameInstallationId::new(game_id).map_err(import_error)?;
            let store = registry().map_err(import_error)?;
            let snapshot = store.load_read_only().map_err(import_error)?;
            let record = snapshot
                .installations
                .iter()
                .find(|record| record.id == id)
                .ok_or_else(|| import_error(GameImportError::NotRegistered))?;
            let validation = store.validate(&id).map_err(import_error)?;
            if validation.registry_revision != snapshot.revision {
                return Err(import_error(GameImportError::RevisionConflict));
            }
            let observation = path
                .as_deref()
                .map(|path| source_observation(source.id, path, None))
                .transpose()
                .map_err(source_error)?;
            let matches = observation.as_ref().is_some_and(|value| {
                value.installation_path == record.selected_path && value.installation_present
            }) && validation.state == GameInstallationState::Registered;
            Ok((
                0,
                json!({"schema_version":1,"game_id":id,"source_id":source.id,"registry_revision":snapshot.revision,"source_evidence_type":if matches {SourceVerificationStatus::ObservedLocalMetadata} else {SourceVerificationStatus::UserDeclared},"local_observation_matches_registered_location":matches,"local_observation":observation,"game_validation":validation,"persistence":"unsupported_read_only_plan","association_stored":false,"publisher_authenticity":"unverifiable","artifact_integrity":"unverifiable","game_compatibility":"not_tested","execution_readiness":"unknown","guidance":source.guidance,"limitations":"Source label is user-declared unless the local Steam observation matches the revalidated registered location. Snapshot revision is observational, not a reservation or cross-registry transaction. Re-run after any change; no apply command exists."}),
            ))
        }
        Operation::Validate { game_id, .. } => {
            let id = GameInstallationId::new(game_id).map_err(import_error)?;
            let validation = registry()
                .map_err(import_error)?
                .validate(&id)
                .map_err(import_error)?;
            let code = if validation.state == GameInstallationState::Registered {
                0
            } else {
                4
            };
            Ok((
                code,
                json!({"schema_version":1,"game_id":id,"source_association":null,"source_status":"unknown","game_validation":validation,"game_compatibility":"not_tested","guidance":"No source association is persisted in P031 schema v1. Use source plan-association for an explicit read-only preview; source revalidate requires the prior exact path and observation token."}),
            ))
        }
    }
}
/// JSON is the machine contract; human reports use bounded, escaped JSON values for safe local display.
fn render(output: &mut impl Write, json_output: bool, code: u8, value: Value) -> io::Result<u8> {
    let bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
    if bytes.len() > 64 * 1024 {
        return Err(io::Error::other("source report exceeds 64 KiB bound"));
    }
    if !json_output {
        writeln!(
            output,
            "Official game sources (read-only)\nURLs and guidance below are manual handoff only. No download or execution.\n"
        )?;
    }
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    Ok(code)
}
pub fn run(operation: Operation, output: &mut impl Write) -> io::Result<u8> {
    let json_output = operation.json();
    let (code, value) = result(operation).unwrap_or_else(|error| error);
    render(output, json_output, code, value)
}
pub fn list(json_output: bool, output: &mut impl Write) -> io::Result<u8> {
    render(output, json_output, 0, json!(sources::catalog()))
}
