use clap::{Subcommand, ValueEnum};
use endfield_unix_prefix_management as prefixes;
use endfield_unix_runtime_selection as pins;
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum Operation {
    /// List registered prefixes and revalidate their stored runtime associations.
    List,
    /// Inspect one registered prefix.
    Show {
        #[arg(long)]
        id: String,
    },
    /// Validate one prefix and its associated runtime without changing files.
    Validate {
        #[arg(long)]
        id: String,
    },
    /// Preview managed storage and the exact P027-selected runtime association.
    Plan {
        #[arg(long)]
        id: String,
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        expected_revision: Option<u64>,
    },
    /// Create a managed directory and metadata; this does not initialize Wine.
    Create {
        #[arg(long)]
        id: String,
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Request Wine initialization; currently returns an explicit blocked result.
    Init {
        #[arg(long)]
        id: String,
    },
    /// Reconcile an interrupted directory-only create operation.
    Recover {
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Remove only the registry entry and preserve prefix data.
    Remove {
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Delete data after confirming the exact path reported by plan or show.
    Delete {
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long, value_name = "EXACT_PATH")]
        confirm_path: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ScopeArg {
    Machine,
    Profile,
}

#[derive(Debug)]
enum Failure {
    Prefix(prefixes::PrefixError),
    Runtime(String),
    Output(io::Error),
}

impl From<prefixes::PrefixError> for Failure {
    fn from(value: prefixes::PrefixError) -> Self {
        Self::Prefix(value)
    }
}
impl From<io::Error> for Failure {
    fn from(value: io::Error) -> Self {
        Self::Output(value)
    }
}

pub fn run(operation: Operation, out: &mut impl Write) -> io::Result<u8> {
    match run_inner(operation, out) {
        Ok(code) => Ok(code),
        Err(Failure::Output(error)) => Err(error),
        Err(failure) => {
            let (category, message, code) = match failure {
                Failure::Runtime(reason) => ("runtime_selection_blocked", reason, 4),
                Failure::Prefix(error) => {
                    let code = match &error {
                        prefixes::PrefixError::InitializationUnsupported => 5,
                        prefixes::PrefixError::NotRegistered | prefixes::PrefixError::Missing => 3,
                        _ => 4,
                    };
                    (prefix_error_category(&error), error.to_string(), code)
                }
                Failure::Output(_) => unreachable!("output failures are returned above"),
            };
            writeln!(out, "Prefix operation failed [{category}]: {message}")?;
            Ok(code)
        }
    }
}

fn run_inner(operation: Operation, out: &mut impl Write) -> Result<u8, Failure> {
    match operation {
        Operation::List => {
            let store = store()?;
            let mut runtime_validations =
                Vec::<(pins::PinnedRuntimeReference, Result<(), String>)>::new();
            let (registry, items) = store.list(|reference| {
                if let Some((_, result)) = runtime_validations
                    .iter()
                    .find(|(cached, _)| cached == reference)
                {
                    return result.clone();
                }
                let result = super::runtime_pin::validate_reference(reference);
                runtime_validations.push((reference.clone(), result.clone()));
                result
            })?;
            writeln!(
                out,
                "Managed prefixes\n\nRegistry revision: {}\nPrefixes: {}",
                registry.revision,
                items.len()
            )?;
            let mut blocked = false;
            let mut pin_relations = Vec::<(
                prefixes::PrefixScope,
                prefixes::PrefixRuntimeAssociation,
                (String, bool),
            )>::new();
            for item in &items {
                let relation = pin_relations
                    .iter()
                    .find(|(scope, association, _)| {
                        scope == &item.record.scope && association == &item.record.runtime
                    })
                    .map(|(_, _, relation)| relation.clone())
                    .unwrap_or_else(|| {
                        current_pin_relation(&item.record.scope, &item.record.runtime)
                    });
                blocked |= inspection_blocked(item, relation.1);
                write_inspection(out, item, &relation.0)?;
                pin_relations.push((
                    item.record.scope.clone(),
                    item.record.runtime.clone(),
                    relation,
                ));
            }
            writeln!(
                out,
                "\nRuntime execution: not performed\nGame execution: not performed"
            )?;
            Ok(if blocked { 4 } else { 0 })
        }
        Operation::Show { id } => {
            let id = prefixes::PrefixId::new(id)?;
            let item = store()?.show(&id, super::runtime_pin::validate_reference)?;
            let (pin_relation, pin_blocked) =
                current_pin_relation(&item.record.scope, &item.record.runtime);
            write_inspection(out, &item, &pin_relation)?;
            Ok(if inspection_blocked(&item, pin_blocked) {
                4
            } else {
                0
            })
        }
        Operation::Validate { id } => {
            let id = prefixes::PrefixId::new(id)?;
            let report = store()?.validate(&id, super::runtime_pin::validate_reference)?;
            let (pin_relation, pin_blocked) =
                current_pin_relation(&report.record.scope, &report.record.runtime);
            writeln!(
                out,
                "Prefix validation\n\nID: {}\nLifecycle: {}\nRuntime association: {}\nRuntime validation: {}\nCurrent P027 pin: {pin_relation}\nAdapter preference: {}\nRegistry revision: {}\nPath ownership: {}\nWine initialization: {}\nInitialization evidence: {}\nCompatibility: not verified",
                report.record.id.as_str(),
                lifecycle_label(&report.observed_lifecycle),
                runtime_label(&report.runtime),
                runtime_validation_status_label(&report.record.runtime.runtime, &report.runtime),
                adapter_label(report.record.runtime.adapter_preference),
                report.registry_revision,
                ownership_label(&report.record.ownership, &report.observed_lifecycle),
                initialization_label(&report.observed_lifecycle),
                initialization_evidence_label(&report.record)
            )?;
            if let prefixes::RuntimeValidation::Stale(reason) = &report.runtime {
                writeln!(out, "Runtime diagnostic: {reason}")?;
            }
            writeln!(
                out,
                "Prefix bytes changed: no\nRuntime execution: not performed\nGame execution: not performed"
            )?;
            Ok(
                if matches!(
                    report.observed_lifecycle,
                    prefixes::PrefixLifecycleState::Created
                        | prefixes::PrefixLifecycleState::Initialized
                ) && report.runtime == prefixes::RuntimeValidation::Valid
                    && !pin_blocked
                {
                    0
                } else {
                    4
                },
            )
        }
        Operation::Plan {
            id,
            scope,
            profile,
            expected_revision,
        } => {
            let (scope, profile) = scope_and_profile(scope, profile)?;
            let id = prefixes::PrefixId::new(id)?;
            let runtime = resolve_runtime(profile.as_deref())?;
            let plan = store()?.plan(id, scope, runtime, expected_revision)?;
            write_plan(out, &plan)?;
            Ok(0)
        }
        Operation::Create {
            id,
            scope,
            profile,
            expected_revision,
        } => {
            let (scope, profile) = scope_and_profile(scope, profile)?;
            let id = prefixes::PrefixId::new(id)?;
            let store = store()?;
            let planned_runtime = resolve_runtime(profile.as_deref())?;
            let plan = store.plan(
                id.clone(),
                scope.clone(),
                planned_runtime.clone(),
                Some(expected_revision),
            )?;
            if plan.already_created {
                writeln!(
                    out,
                    "Prefix already exists\n\nID: {}\nLifecycle: created\nRegistry revision: {}\nCreation: idempotent; no files changed",
                    id.as_str(),
                    plan.expected_registry_revision
                )?;
                return Ok(0);
            }
            let current_runtime = resolve_runtime(profile.as_deref())?;
            if current_runtime != planned_runtime {
                return Err(Failure::Runtime(
                    "P027 selection changed during preflight; run prefix plan again".into(),
                ));
            }
            let (record, revision) = store.create(id, scope, current_runtime, expected_revision)?;
            writeln!(
                out,
                "Managed prefix directory created\n\nID: {}\nLifecycle: created (uninitialized)\nPath: {}\nRegistry revision: {}\nRuntime association: {}\nRuntime validation: {}\nAdapter preference: {}\nCompatibility: not verified\nWine initialization: not performed\nRuntime execution: not performed\nGame execution: not performed",
                record.id.as_str(),
                store.target_path(&record.id).display(),
                revision,
                runtime_record_label(&record.runtime),
                runtime_validation_basis_label(&record.runtime.runtime),
                adapter_label(record.runtime.adapter_preference)
            )?;
            Ok(0)
        }
        Operation::Init { id } => {
            let id = prefixes::PrefixId::new(id)?;
            let store = store()?;
            let item = store.show(&id, super::runtime_pin::validate_reference)?;
            if item.observed_lifecycle == prefixes::PrefixLifecycleState::Initialized {
                writeln!(
                    out,
                    "Initialization: already completed and verified\nEvidence: {}\nProcess execution: not performed\nRuntime execution: not performed\nGame execution: not performed",
                    initialization_evidence_label(&item.record)
                )?;
                return Ok(0);
            }
            if item.observed_lifecycle != prefixes::PrefixLifecycleState::Created {
                writeln!(
                    out,
                    "Initialization: BLOCKED\nError category: invalid_lifecycle_state\nLifecycle: {}\nProcess execution: not performed\nRuntime execution: not performed\nGame execution: not performed",
                    lifecycle_label(&item.observed_lifecycle)
                )?;
                return Ok(4);
            }
            if item.runtime != prefixes::RuntimeValidation::Valid {
                writeln!(
                    out,
                    "Initialization: BLOCKED\nError category: runtime_association_stale\nRuntime association: stale or unavailable\nProcess execution: not performed\nRuntime execution: not performed\nGame execution: not performed"
                )?;
                return Ok(4);
            }
            if current_pin_relation(&item.record.scope, &item.record.runtime).1 {
                writeln!(
                    out,
                    "Initialization: BLOCKED\nError category: runtime_selection_blocked\nAutomatic fallback: not performed\nProcess execution: not performed\nRuntime execution: not performed\nGame execution: not performed"
                )?;
                return Ok(4);
            }
            match store.initialize(&id) {
                Ok(()) => Ok(0),
                Err(prefixes::PrefixError::InitializationUnsupported) => {
                    writeln!(
                        out,
                        "Initialization: BLOCKED\nError category: initialization_unsupported\nReason: no validated Wine or Proton initialization adapter is available\nPrefix lifecycle: created (uninitialized)\nProcess execution: not performed\nRuntime execution: not performed\nGame execution: not performed"
                    )?;
                    Ok(5)
                }
                Err(error) => Err(error.into()),
            }
        }
        Operation::Recover {
            id,
            expected_revision,
        } => {
            let id = prefixes::PrefixId::new(id)?;
            let (record, revision) = store()?.recover(&id, expected_revision)?;
            writeln!(
                out,
                "Prefix recovery completed\n\nID: {}\nLifecycle: created (uninitialized)\nRegistry revision: {}\nWine execution: not performed",
                record.id.as_str(),
                revision
            )?;
            Ok(0)
        }
        Operation::Remove {
            id,
            expected_revision,
        } => {
            let id = prefixes::PrefixId::new(id)?;
            let store = store()?;
            let preserved_path = store.target_path(&id);
            let revision = store.unregister(&id, expected_revision)?;
            writeln!(
                out,
                "Prefix registry entry removed\n\nID: {}\nRegistry revision: {}\nPrefix bytes: preserved\nPreserved data path: {}\nData deletion: not performed",
                id.as_str(),
                revision,
                preserved_path.display()
            )?;
            Ok(0)
        }
        Operation::Delete {
            id,
            expected_revision,
            confirm_path,
        } => {
            let id = prefixes::PrefixId::new(id)?;
            let store = store()?;
            let target = store.target_path(&id);
            let revision = store.delete_data(&id, expected_revision, &confirm_path)?;
            writeln!(
                out,
                "Managed prefix data deleted\n\nID: {}\nDeleted target: {}\nRegistry revision: {}\nRuntime installation: unchanged\nRuntime pin: unchanged\nGame execution: not performed",
                id.as_str(),
                target.display(),
                revision
            )?;
            Ok(0)
        }
    }
}

fn store() -> Result<prefixes::PrefixStore, Failure> {
    let paths = endfield_unix_storage::AppPaths::discover()
        .map_err(|error| Failure::Prefix(prefixes::PrefixError::Storage(error.to_string())))?;
    Ok(prefixes::PrefixStore::for_app(&paths))
}

fn scope_and_profile(
    scope: ScopeArg,
    profile: Option<String>,
) -> Result<(prefixes::PrefixScope, Option<String>), Failure> {
    let machine =
        pins::current_machine_id().map_err(|error| Failure::Runtime(error.to_string()))?;
    match (scope, profile) {
        (ScopeArg::Machine, None) => Ok((prefixes::PrefixScope::machine(machine), None)),
        (ScopeArg::Profile, Some(profile)) => {
            let key = pins::ProfileKey::new(profile.clone())
                .map_err(|_| Failure::Prefix(prefixes::PrefixError::InvalidScope))?;
            Ok((prefixes::PrefixScope::profile(machine, key), Some(profile)))
        }
        (ScopeArg::Machine, Some(_)) | (ScopeArg::Profile, None) => {
            Err(Failure::Prefix(prefixes::PrefixError::InvalidScope))
        }
    }
}

fn resolve_runtime(profile: Option<&str>) -> Result<prefixes::PrefixRuntimeAssociation, Failure> {
    let selected = super::runtime_pin::resolve_for_prefix(profile).map_err(Failure::Runtime)?;
    Ok(prefixes::PrefixRuntimeAssociation {
        runtime: selected.runtime,
        selection_source: match selected.source {
            RuntimeScope::Machine => prefixes::RuntimeSelectionSource::MachinePin,
            RuntimeScope::Profile => prefixes::RuntimeSelectionSource::ProfilePin,
        },
        pin_id: selected.pin_id,
        pin_revision: selected.revision,
        adapter_preference: selected.adapter_preference,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeScope {
    Machine,
    Profile,
}

fn write_plan(out: &mut impl Write, plan: &prefixes::PrefixOperationPlan) -> io::Result<()> {
    writeln!(
        out,
        "Prefix creation plan\n\nID: {}\nScope: {}\nDestination: {}\nPrefix registry revision: {}\nAvailable bytes: {}\nRuntime family: {}\nRuntime build: {}\nRuntime identity: {}\nRuntime validation: {}\nAdapter preference: {}\nRuntime pin ID: {}\nRuntime pin revision: {}\nSelection source: {:?}\nCompatibility: not verified\nExisting prefix: {}\nWine execution required: {}\nFilesystem mutation: no\nRuntime execution: not performed\nGame execution: not performed",
        plan.id.as_str(),
        scope_label(&plan.scope),
        plan.destination.display(),
        plan.expected_registry_revision,
        plan.free_bytes
            .map(|bytes| bytes.to_string())
            .unwrap_or_else(|| "unknown".into()),
        plan.runtime.runtime.family(),
        plan.runtime.runtime.build().unwrap_or("unknown"),
        runtime_record_label(&plan.runtime),
        runtime_validation_basis_label(&plan.runtime.runtime),
        adapter_label(plan.runtime.adapter_preference),
        plan.runtime.pin_id,
        plan.runtime.pin_revision,
        plan.runtime.selection_source,
        if plan.already_created { "yes" } else { "no" },
        plan.requires_wine_execution
    )
}

fn write_inspection(
    out: &mut impl Write,
    item: &prefixes::PrefixInspection,
    pin_relation: &str,
) -> io::Result<()> {
    let adapter = item
        .record
        .runtime
        .adapter_preference
        .map(|preference| format!("{preference:?}"))
        .unwrap_or_else(|| "not selected".into());
    writeln!(
        out,
        "\nPrefix {}\n  Scope: {}\n  Path: {}\n  Lifecycle: {}\n  Ownership: {}\n  Runtime association: {}\n  Runtime validation: {}\n  Current P027 pin: {pin_relation}\n  Adapter preference: {}\n  Wine initialization: {}\n  Initialization evidence: {}\n  Compatibility: not verified",
        item.record.id.as_str(),
        scope_label(&item.record.scope),
        item.path.display(),
        lifecycle_label(&item.observed_lifecycle),
        ownership_label(&item.record.ownership, &item.observed_lifecycle),
        runtime_label(&item.runtime),
        runtime_validation_status_label(&item.record.runtime.runtime, &item.runtime),
        adapter,
        initialization_label(&item.observed_lifecycle),
        initialization_evidence_label(&item.record)
    )?;
    if let prefixes::RuntimeValidation::Stale(reason) = &item.runtime {
        writeln!(out, "  Runtime diagnostic: {reason}")?;
    }
    Ok(())
}

fn current_pin_relation(
    scope: &prefixes::PrefixScope,
    association: &prefixes::PrefixRuntimeAssociation,
) -> (String, bool) {
    let recorded_machine = match scope {
        prefixes::PrefixScope::Machine { machine }
        | prefixes::PrefixScope::Profile { machine, .. } => machine,
    };
    match pins::current_machine_id() {
        Ok(machine) if &machine == recorded_machine => {}
        Ok(_) => {
            return (
                "blocked; recorded machine identity does not match this host".into(),
                true,
            );
        }
        Err(_) => {
            return (
                "blocked; current machine identity could not be established".into(),
                true,
            );
        }
    }
    let profile = scope.profile_id().map(pins::ProfileKey::as_str);
    match super::runtime_pin::resolve_for_prefix(profile) {
        Ok(current) => {
            let source_matches = matches!(
                (association.selection_source, current.source),
                (
                    prefixes::RuntimeSelectionSource::MachinePin,
                    RuntimeScope::Machine
                ) | (
                    prefixes::RuntimeSelectionSource::ProfilePin,
                    RuntimeScope::Profile
                )
            );
            if association.runtime == current.runtime
                && association.pin_id == current.pin_id
                && association.adapter_preference == current.adapter_preference
                && source_matches
            {
                if association.pin_revision == current.revision {
                    ("matches the runtime pin selected at creation".into(), false)
                } else {
                    (
                        format!(
                            "same runtime identity; pin registry revision changed from {} to {}",
                            association.pin_revision, current.revision
                        ),
                        false,
                    )
                }
            } else {
                (
                    "changed; existing prefix association is preserved".into(),
                    false,
                )
            }
        }
        Err(reason) => (
            format!("blocked or stale; automatic fallback was not performed ({reason})"),
            true,
        ),
    }
}

fn runtime_record_label(runtime: &prefixes::PrefixRuntimeAssociation) -> String {
    let identity = match &runtime.runtime {
        pins::PinnedRuntimeReference::Managed(runtime) => runtime.installation_id.as_str(),
        pins::PinnedRuntimeReference::External(_) => "external runtime (path redacted)",
    };
    format!(
        "{} {} ({identity})",
        runtime.runtime.family(),
        runtime.runtime.build().unwrap_or("unknown build")
    )
}

fn runtime_validation_basis_label(runtime: &pins::PinnedRuntimeReference) -> &'static str {
    match runtime {
        pins::PinnedRuntimeReference::Managed(_) => "P025 ownership and P026 baseline validated",
        pins::PinnedRuntimeReference::External(_) => {
            "P022 structure revalidated; integrity not assessed"
        }
    }
}

fn runtime_validation_status_label(
    runtime: &pins::PinnedRuntimeReference,
    validation: &prefixes::RuntimeValidation,
) -> &'static str {
    match validation {
        prefixes::RuntimeValidation::Valid => runtime_validation_basis_label(runtime),
        prefixes::RuntimeValidation::Stale(_) => match runtime {
            pins::PinnedRuntimeReference::Managed(_) => {
                "stale or unavailable; P025/P026 validation failed"
            }
            pins::PinnedRuntimeReference::External(_) => {
                "stale or unavailable; P022 structural identity changed"
            }
        },
        prefixes::RuntimeValidation::NotAssessed => "not assessed",
    }
}

fn initialization_evidence_label(record: &prefixes::PrefixRecord) -> String {
    let Some(evidence) = &record.initialization_evidence else {
        return "none".into();
    };
    let outcome = match evidence.outcome {
        prefixes::InitializationEvidenceOutcome::CompletedAndVerified => "completed and verified",
        prefixes::InitializationEvidenceOutcome::Partial => "partial",
        prefixes::InitializationEvidenceOutcome::Failed => "failed",
    };
    format!(
        "{outcome}; adapter {}; observed at {}",
        evidence.adapter_id, evidence.observed_at_unix_seconds
    )
}

fn adapter_label(adapter: Option<pins::AdapterPreference>) -> &'static str {
    match adapter {
        Some(pins::AdapterPreference::UmuProton) => "UmuProton",
        Some(pins::AdapterPreference::ManagedWine) => "ManagedWine",
        Some(pins::AdapterPreference::ExternalWine) => "ExternalWine",
        None => "not selected",
    }
}

fn runtime_label(validation: &prefixes::RuntimeValidation) -> &'static str {
    match validation {
        prefixes::RuntimeValidation::Valid => "available; identity revalidated",
        prefixes::RuntimeValidation::Stale(_) => "stale or unavailable",
        prefixes::RuntimeValidation::NotAssessed => "not assessed",
    }
}

fn lifecycle_label(state: &prefixes::PrefixLifecycleState) -> &'static str {
    match state {
        prefixes::PrefixLifecycleState::Planned => "planned (no mutation)",
        prefixes::PrefixLifecycleState::Created => "created (uninitialized)",
        prefixes::PrefixLifecycleState::Initialized => "initialized",
        prefixes::PrefixLifecycleState::Incomplete => "incomplete",
        prefixes::PrefixLifecycleState::RecoveryRequired => "recovery required",
        prefixes::PrefixLifecycleState::Missing => "missing",
        prefixes::PrefixLifecycleState::Blocked => "blocked",
    }
}

fn initialization_label(state: &prefixes::PrefixLifecycleState) -> &'static str {
    match state {
        prefixes::PrefixLifecycleState::Initialized => "initialized (evidence recorded)",
        prefixes::PrefixLifecycleState::Created => "not initialized",
        prefixes::PrefixLifecycleState::Planned => "not initialized (plan only)",
        prefixes::PrefixLifecycleState::Incomplete
        | prefixes::PrefixLifecycleState::RecoveryRequired => "not verified (recovery required)",
        prefixes::PrefixLifecycleState::Missing => "not verified (directory missing)",
        prefixes::PrefixLifecycleState::Blocked => "not verified (state blocked)",
    }
}

fn ownership_label(
    ownership: &prefixes::PrefixOwnership,
    state: &prefixes::PrefixLifecycleState,
) -> &'static str {
    if *ownership != prefixes::PrefixOwnership::EndfieldUnixManaged
        || *state == prefixes::PrefixLifecycleState::Blocked
    {
        "unverified; operation blocked"
    } else {
        "Endfield-Unix managed"
    }
}

fn scope_label(scope: &prefixes::PrefixScope) -> String {
    match scope {
        prefixes::PrefixScope::Machine { .. } => "machine".into(),
        prefixes::PrefixScope::Profile { profile, .. } => format!("profile ({})", profile.as_str()),
    }
}

fn inspection_blocked(item: &prefixes::PrefixInspection, pin_blocked: bool) -> bool {
    !matches!(
        item.observed_lifecycle,
        prefixes::PrefixLifecycleState::Created | prefixes::PrefixLifecycleState::Initialized
    ) || item.runtime != prefixes::RuntimeValidation::Valid
        || pin_blocked
}

fn prefix_error_category(error: &prefixes::PrefixError) -> &'static str {
    match error {
        prefixes::PrefixError::InvalidId => "prefix_id_invalid",
        prefixes::PrefixError::InvalidScope => "prefix_scope_invalid",
        prefixes::PrefixError::InvalidRuntime => "runtime_identity_invalid",
        prefixes::PrefixError::RegistryCorrupt => "registry_corrupt_or_unsupported",
        prefixes::PrefixError::UnsupportedSchemaVersion(_) => "registry_schema_unsupported",
        prefixes::PrefixError::RevisionConflict => "revision_conflict",
        prefixes::PrefixError::IdentityConflict => "identity_conflict",
        prefixes::PrefixError::DestinationCollision => "destination_collision",
        prefixes::PrefixError::RecoveryRequired => "recovery_required",
        prefixes::PrefixError::RecoveryBlocked => "recovery_blocked",
        prefixes::PrefixError::Missing => "prefix_missing",
        prefixes::PrefixError::NotRegistered => "prefix_not_registered",
        prefixes::PrefixError::ConfirmationMismatch => "confirmation_mismatch",
        prefixes::PrefixError::ActiveUse => "prefix_in_use",
        prefixes::PrefixError::UsageUnknown => "prefix_use_unknown",
        prefixes::PrefixError::InitializationUnsupported => "initialization_unsupported",
        prefixes::PrefixError::Blocked => "unsafe_or_unverifiable_state",
        prefixes::PrefixError::InvalidState => "invalid_lifecycle_state",
        prefixes::PrefixError::Io(_) => "filesystem_error",
        prefixes::PrefixError::Storage(_) => "storage_paths_unavailable",
    }
}
