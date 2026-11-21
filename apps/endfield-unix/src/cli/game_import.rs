//! Headless, explicit game-directory import commands.
use clap::Subcommand;
use endfield_unix_game_import::{
    GameImportError, GameImportPlan, GameInstallationId, GameInstallationRecord,
    GameInstallationRegistry, GameInstallationSnapshot, GameInstallationState,
    GameInstallationValidation, GameUnregisterPlan, REGISTRY_SCHEMA_VERSION,
};
use serde::Serialize;
use std::{
    io::{self, Write},
    path::PathBuf,
};

const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_HUMAN_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum Operation {
    /// Build a read-only, explicitly selected launch plan.
    Launch {
        #[command(subcommand)]
        command: super::game_launch::Operation,
    },
    /// List reviewed official distribution sources, without network access.
    Sources {
        #[arg(long)]
        json: bool,
    },
    /// Official handoff and bounded source observation (no download).
    Source {
        #[command(subcommand)]
        command: super::game_sources::Operation,
    },
    /// Plan or apply registration of one explicitly selected directory.
    Import {
        #[command(subcommand)]
        command: ImportOperation,
    },
    /// List registered locations without disclosing their absolute paths.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Inspect one local registration, including its selected path.
    Show {
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Revalidate the exact registered path without mutation or relocation search.
    Validate {
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Preview metadata-only removal of one registration.
    RemovePlan {
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove only one application's registration record.
    Remove {
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        confirm: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum ImportOperation {
    /// Inspect one selected directory and produce an approval token.
    Plan {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        id: String,
        /// Explicitly declare that the selected directory contains Endfield.
        #[arg(long)]
        declare_endfield: bool,
        #[arg(long)]
        json: bool,
    },
    /// Register the selected location after revalidation and exact approval.
    Apply {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        id: String,
        /// Repeat the explicit user declaration shown in the import plan.
        #[arg(long)]
        declare_endfield: bool,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        confirm: String,
        #[arg(long)]
        json: bool,
    },
}

impl Operation {
    fn json(&self) -> bool {
        match self {
            Self::Launch { command } => command.json(),
            Self::Source { .. } => false,
            Self::Import { command } => match command {
                ImportOperation::Plan { json, .. } | ImportOperation::Apply { json, .. } => *json,
            },
            Self::Sources { json }
            | Self::List { json }
            | Self::Show { json, .. }
            | Self::Validate { json, .. }
            | Self::RemovePlan { json, .. }
            | Self::Remove { json, .. } => *json,
        }
    }
}

#[derive(Serialize)]
struct GameListReport<'a> {
    schema_version: u32,
    registry_revision: u64,
    installations: Vec<GameListItem<'a>>,
}

#[derive(Serialize)]
struct GameListItem<'a> {
    id: &'a str,
    scope: &'static str,
    state: &'static str,
    identity: &'static str,
    structural_evidence: &'static str,
    publisher_authenticity_verified: bool,
    game_build: Option<&'a str>,
}

#[derive(Serialize)]
struct RegistrationResult<'a> {
    schema_version: u32,
    registry_revision: u64,
    installation: &'a GameInstallationRecord,
    compatibility: &'static str,
    game_execution: &'static str,
}

#[derive(Serialize)]
struct RemovalResult<'a> {
    schema_version: u32,
    id: &'a str,
    registry_revision: u64,
    registration: &'static str,
    game_directory: &'static str,
}

#[derive(Serialize)]
struct ErrorResult<'a> {
    schema_version: u32,
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

pub fn run(operation: Operation, output: &mut impl Write) -> io::Result<u8> {
    let operation = match operation {
        Operation::Launch { command } => return super::game_launch::run(command, output),
        operation => operation,
    };
    let operation = match operation {
        Operation::Sources { json } => return super::game_sources::list(json, output),
        Operation::Source { command } => return super::game_sources::run(command, output),
        operation => operation,
    };
    let json = operation.json();
    let mut rendered = Vec::new();
    let result = run_inner(operation, &mut rendered, json);
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            if json {
                rendered.clear();
                write_json(
                    &mut rendered,
                    &ErrorResult {
                        schema_version: REGISTRY_SCHEMA_VERSION,
                        error: ErrorDetail {
                            code: error.code(),
                            message: &error.to_string(),
                        },
                    },
                )?;
            } else {
                writeln!(rendered, "Game import failed [{}]: {error}", error.code())?;
            }
            error.exit_status()
        }
    };
    let limit = if json {
        MAX_JSON_BYTES
    } else {
        MAX_HUMAN_BYTES
    };
    if rendered.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "game import output exceeded its configured bound",
        ));
    }
    output.write_all(&rendered)?;
    Ok(code)
}

fn run_inner(
    operation: Operation,
    output: &mut impl Write,
    json: bool,
) -> Result<u8, GameImportError> {
    let paths = endfield_unix_storage::AppPaths::discover()
        .map_err(|_| GameImportError::StorageUnavailable)?;
    let registry = GameInstallationRegistry::for_app(&paths);
    match operation {
        Operation::Launch { command } => {
            return super::game_launch::run(command, output).map_err(GameImportError::Io);
        }
        Operation::Sources { .. } | Operation::Source { .. } => {
            unreachable!("source commands dispatch before storage discovery")
        }
        Operation::Import { command } => match command {
            ImportOperation::Plan {
                path,
                id,
                declare_endfield,
                ..
            } => {
                let id = GameInstallationId::new(id)?;
                let plan = registry.plan_import(id, &path, declare_endfield)?;
                if json {
                    write_json(output, &plan).map_err(GameImportError::Io)?;
                } else {
                    write_plan(output, &plan).map_err(GameImportError::Io)?;
                }
                if !plan.can_register {
                    return Ok(if plan.existing_id_collision {
                        GameImportError::DuplicateId
                    } else if plan.existing_location_collision.is_some() {
                        GameImportError::DuplicateLocation
                    } else if plan.evidence.root_entries.symlinks > 0
                        || plan.evidence.root_entries.special_files > 0
                    {
                        GameImportError::UnsupportedEntries
                    } else if !plan.filesystem.mount_id_observed
                        || !plan.filesystem.object_identity_observed
                    {
                        GameImportError::FilesystemIdentityUnavailable
                    } else if plan.filesystem.filesystem_type == "unknown" {
                        GameImportError::UnsupportedFilesystem
                    } else {
                        GameImportError::DeclarationRequired
                    }
                    .exit_status());
                }
            }
            ImportOperation::Apply {
                path,
                id,
                declare_endfield,
                expected_revision,
                confirm,
                ..
            } => {
                let id = GameInstallationId::new(id)?;
                let (record, revision) = registry.apply_import(
                    id,
                    &path,
                    declare_endfield,
                    expected_revision,
                    &confirm,
                )?;
                if json {
                    write_json(
                        output,
                        &RegistrationResult {
                            schema_version: REGISTRY_SCHEMA_VERSION,
                            registry_revision: revision,
                            installation: &record,
                            compatibility: "not_tested",
                            game_execution: "not_performed",
                        },
                    )
                    .map_err(GameImportError::Io)?;
                } else {
                    writeln!(
                        output,
                        "Game location registered\n\nID: {}\nPath: {}\nScope: user\nRegistry revision: {}\nGame identity: user-declared, unverified\nStructure: no authoritative Endfield layout is configured\nPublisher authenticity: not verified\nCompatibility: not tested\nGame execution: not performed\nGame files modified: no",
                        record.id.as_str(),
                        record.selected_path,
                        revision
                    )
                    .map_err(GameImportError::Io)?;
                }
            }
        },
        Operation::List { .. } => {
            let snapshot = registry.load_read_only()?;
            let report = list_report(&snapshot);
            if json {
                write_json(output, &report).map_err(GameImportError::Io)?;
            } else {
                write_list(output, &snapshot).map_err(GameImportError::Io)?;
            }
        }
        Operation::Show { id, .. } => {
            let id = GameInstallationId::new(id)?;
            let snapshot = registry.load_read_only()?;
            let record = snapshot
                .installations
                .iter()
                .find(|record| record.id == id)
                .ok_or(GameImportError::NotRegistered)?;
            if json {
                write_json(output, record).map_err(GameImportError::Io)?;
            } else {
                write_record(output, record, snapshot.revision).map_err(GameImportError::Io)?;
            }
        }
        Operation::Validate { id, .. } => {
            let id = GameInstallationId::new(id)?;
            let validation = registry.validate(&id)?;
            if json {
                write_json(output, &validation).map_err(GameImportError::Io)?;
            } else {
                write_validation(output, &validation).map_err(GameImportError::Io)?;
            }
            if matches!(
                validation.state,
                GameInstallationState::Changed
                    | GameInstallationState::Missing
                    | GameInstallationState::Blocked
                    | GameInstallationState::Unsupported
            ) {
                return Err(match validation.state {
                    GameInstallationState::Missing => GameImportError::RegisteredLocationMissing,
                    GameInstallationState::Changed => GameImportError::ChangedSincePlan,
                    GameInstallationState::Unsupported => GameImportError::RevalidationBlocked,
                    _ => GameImportError::RevalidationBlocked,
                });
            }
        }
        Operation::RemovePlan { id, .. } => {
            let id = GameInstallationId::new(id)?;
            let plan = registry.plan_unregister(&id)?;
            if json {
                write_json(output, &plan).map_err(GameImportError::Io)?;
            } else {
                write_unregister_plan(output, &plan).map_err(GameImportError::Io)?;
            }
        }
        Operation::Remove {
            id,
            expected_revision,
            confirm,
            ..
        } => {
            let id = GameInstallationId::new(id)?;
            let revision = registry.unregister(&id, expected_revision, &confirm)?;
            if json {
                write_json(
                    output,
                    &RemovalResult {
                        schema_version: REGISTRY_SCHEMA_VERSION,
                        id: id.as_str(),
                        registry_revision: revision,
                        registration: "removed",
                        game_directory: "preserved",
                    },
                )
                .map_err(GameImportError::Io)?;
            } else {
                writeln!(
                    output,
                    "Game registration removed\n\nID: {}\nRegistry revision: {}\nGame directory: preserved\nGame files modified: no",
                    id.as_str(), revision
                )
                .map_err(GameImportError::Io)?;
            }
        }
    }
    Ok(0)
}

fn list_report(snapshot: &GameInstallationSnapshot) -> GameListReport<'_> {
    GameListReport {
        schema_version: REGISTRY_SCHEMA_VERSION,
        registry_revision: snapshot.revision,
        installations: snapshot
            .installations
            .iter()
            .map(|record| GameListItem {
                id: record.id.as_str(),
                scope: "user",
                state: "registered",
                identity: "user_declared_unverified",
                structural_evidence: "no_authoritative_format_known",
                publisher_authenticity_verified: false,
                game_build: None,
            })
            .collect(),
    }
}

fn write_plan(output: &mut impl Write, plan: &GameImportPlan) -> io::Result<()> {
    writeln!(output, "Game import plan")?;
    writeln!(output, "\nID: {}", plan.proposed_id.as_str())?;
    writeln!(output, "Selected path: {}", plan.selected_path)?;
    writeln!(output, "Scope: user")?;
    writeln!(output, "Registry revision: {}", plan.registry_revision)?;
    writeln!(
        output,
        "Root object: device {} / inode {} / mount {}",
        plan.root_identity.device,
        plan.root_identity.inode,
        plan.root_identity
            .mount_id
            .map_or_else(|| "unavailable".into(), |value| value.to_string())
    )?;
    writeln!(
        output,
        "Filesystem: {} (mount identity observed: {})",
        plan.filesystem.filesystem_type, plan.filesystem.mount_id_observed
    )?;
    writeln!(
        output,
        "Root entries: {} total ({} directories, {} regular files, {} links, {} special)",
        plan.evidence.root_entries.total,
        plan.evidence.root_entries.directories,
        plan.evidence.root_entries.regular_files,
        plan.evidence.root_entries.symlinks,
        plan.evidence.root_entries.special_files
    )?;
    writeln!(
        output,
        "Game identity: {}",
        if plan.evidence.user_declared_endfield {
            "user-declared, unverified"
        } else {
            "not established"
        }
    )?;
    writeln!(
        output,
        "Structural evidence: no authoritative Endfield format is configured"
    )?;
    writeln!(
        output,
        "Game build: unknown\nPublisher authenticity: not verified\nCompatibility: not evaluated"
    )?;
    writeln!(
        output,
        "Existing ID collision: {}",
        plan.existing_id_collision
    )?;
    writeln!(
        output,
        "Existing location collision: {}",
        plan.existing_location_collision
            .as_ref()
            .map_or("none", |id| id.as_str())
    )?;
    writeln!(output, "Registration available: {}", plan.can_register)?;
    if let Some(confirmation) = &plan.required_confirmation {
        writeln!(output, "Required confirmation: {confirmation}")?;
    }
    writeln!(
        output,
        "Expected storage effects: {}",
        plan.expected_storage_effects
    )?;
    for warning in &plan.warnings {
        writeln!(output, "Warning: {warning}")?;
    }
    writeln!(
        output,
        "Selected-directory writes, hashes, and execution: none"
    )
}

fn write_list(output: &mut impl Write, snapshot: &GameInstallationSnapshot) -> io::Result<()> {
    writeln!(
        output,
        "Game installations\n\nRegistry revision: {}\nInstallations: {}",
        snapshot.revision,
        snapshot.installations.len()
    )?;
    for record in &snapshot.installations {
        writeln!(
            output,
            "{}: registered / user-declared identity unverified / structure unverified",
            record.id.as_str()
        )?;
    }
    writeln!(
        output,
        "Absolute paths omitted. Use `game show --id ID` for local inspection."
    )
}

fn write_record(
    output: &mut impl Write,
    record: &GameInstallationRecord,
    revision: u64,
) -> io::Result<()> {
    writeln!(
        output,
        "Game installation\n\nID: {}\nState: registered\nScope: user\nPath: {}\nRegistry revision: {}",
        record.id.as_str(),
        record.selected_path,
        revision
    )?;
    writeln!(
        output,
        "Root object: device {} / inode {} / mount {}",
        record.root_identity.device,
        record.root_identity.inode,
        record
            .root_identity
            .mount_id
            .map_or_else(|| "unavailable".into(), |value| value.to_string())
    )?;
    writeln!(
        output,
        "Filesystem: {}\nIdentity: user-declared, unverified\nStructure: no authoritative Endfield format known\nPublisher authenticity: not verified\nBuild: unknown\nCompatibility: not evaluated",
        record.filesystem.filesystem_type
    )
}

fn write_validation(
    output: &mut impl Write,
    validation: &GameInstallationValidation,
) -> io::Result<()> {
    writeln!(
        output,
        "Game installation validation\n\nID: {}\nState: {:?}\nPath: {}\nRegistry revision: {}\nIdentity: user-declared, unverified\nExplanation: {}\nCompatibility: not evaluated",
        validation.id.as_str(),
        validation.state,
        validation.registered_path,
        validation.registry_revision,
        validation.explanation
    )
}

fn write_unregister_plan(output: &mut impl Write, plan: &GameUnregisterPlan) -> io::Result<()> {
    writeln!(
        output,
        "Game unregister plan\n\nID: {}\nSelected path: {}\nRegistry revision: {}\nRequired confirmation: {}\nEffects: {}",
        plan.id.as_str(),
        plan.selected_path,
        plan.registry_revision,
        plan.required_confirmation,
        plan.effects
    )
}

fn write_json(output: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSON output exceeded its configured bound",
        ));
    }
    output.write_all(&bytes)?;
    output.write_all(b"\n")
}
