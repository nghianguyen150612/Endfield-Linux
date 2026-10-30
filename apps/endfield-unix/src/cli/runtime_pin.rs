use clap::{Subcommand, ValueEnum};
use endfield_unix_runtime_selection as pins;
use std::{
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum Operation {
    /// List stored machine and profile pins without hashing runtime files.
    List,
    /// Inspect one pin for the current machine.
    Show {
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Explicitly pin one validated Endfield-managed installation.
    Set {
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        installation_id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        replace: bool,
        #[arg(long, value_enum)]
        adapter: Option<AdapterArg>,
    },
    /// Remove one pin record; runtime files are preserved.
    Remove {
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Resolve the applicable pin and revalidate managed identity and P026 baseline.
    Resolve {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ScopeArg {
    Machine,
    Profile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AdapterArg {
    UmuProton,
    ManagedWine,
}

fn scope(
    kind: ScopeArg,
    profile: Option<String>,
    machine: pins::MachineId,
) -> Result<pins::RuntimePinScope, String> {
    match kind {
        ScopeArg::Machine if profile.is_none() => Ok(pins::RuntimePinScope::Machine { machine }),
        ScopeArg::Profile => Ok(pins::RuntimePinScope::Profile {
            machine,
            profile: pins::ProfileKey::new(
                profile.ok_or("--profile is required for profile scope")?,
            )
            .map_err(|_| "profile identifier is invalid")?,
        }),
        ScopeArg::Machine => Err("--profile cannot be used with machine scope".into()),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn store() -> Result<pins::PinStore, String> {
    let paths = endfield_unix_storage::AppPaths::discover()
        .map_err(|_| "XDG configuration paths are unavailable".to_owned())?;
    Ok(pins::PinStore::for_app(&paths))
}

pub fn run(operation: Operation, out: &mut impl Write) -> io::Result<u8> {
    let result = run_inner(operation, out);
    match result {
        Ok(code) => Ok(code),
        Err(message) => {
            writeln!(out, "Runtime pin operation failed: {message}")?;
            Ok(4)
        }
    }
}

fn run_inner(operation: Operation, out: &mut impl Write) -> Result<u8, String> {
    let store = store()?;
    match operation {
        Operation::List => {
            let registry = store.load().map_err(|e| e.to_string())?;
            writeln!(
                out,
                "Runtime pins\n\nRegistry revision: {}\nPins: {}",
                registry.revision,
                registry.pins.len()
            )
            .map_err(|e| e.to_string())?;
            for pin in registry.pins {
                let (kind, profile) = match &pin.scope {
                    pins::RuntimePinScope::Machine { .. } => ("machine", ""),
                    pins::RuntimePinScope::Profile { profile, .. } => ("profile", profile.as_str()),
                };
                writeln!(
                    out,
                    "{kind}{}: {} / {} / {} [{}]",
                    if profile.is_empty() {
                        String::new()
                    } else {
                        format!(" ({profile})")
                    },
                    pin.runtime.family,
                    pin.runtime.build,
                    pin.runtime.installation_id,
                    pin.pin_id
                )
                .map_err(|e| e.to_string())?;
            }
            writeln!(out, "Compatibility: not verified\nRuntime execution: not performed\nGame execution: not performed").map_err(|e| e.to_string())?;
            Ok(0)
        }
        Operation::Show {
            scope: kind,
            profile,
        } => {
            let machine = pins::current_machine_id().map_err(|e| e.to_string())?;
            let target = scope(kind, profile, machine)?;
            let registry = store.load().map_err(|e| e.to_string())?;
            let Some(pin) = registry.pin(&target) else {
                writeln!(
                    out,
                    "No runtime pin configured for this scope.\nAutomatic selection: none"
                )
                .map_err(|e| e.to_string())?;
                return Ok(3);
            };
            print_pin(out, pin, registry.revision).map_err(|e| e.to_string())?;
            Ok(0)
        }
        Operation::Set {
            scope: kind,
            profile,
            installation_id,
            expected_revision,
            replace,
            adapter,
        } => {
            let machine = pins::current_machine_id().map_err(|e| e.to_string())?;
            let target = scope(kind, profile, machine)?;
            let paths = endfield_unix_storage::AppPaths::discover()
                .map_err(|_| "XDG paths are unavailable".to_owned())?;
            let identity = endfield_unix_runtime_install::pin_identity(&paths, &installation_id)
                .map_err(|_| {
                    "managed installation ownership identity could not be validated".to_owned()
                })?;
            let runtime = pins::ManagedRuntimeReference {
                installation_id: identity.installation_id,
                family: identity.family,
                lineage: identity.lineage,
                provider: identity.provider,
                project: identity.project,
                build: identity.build,
                artifact_sha256: identity.artifact_sha256,
                manifest_sha256: identity.manifest_sha256,
                generation: identity.generation,
                ownership_record_sha256: identity.ownership_record_sha256,
                trust: match identity.trust {
                    endfield_unix_runtime_install::TrustClassification::IndependentlyAuthenticatedPublisher => pins::Provenance::IndependentlyAuthenticatedPublisher,
                    endfield_unix_runtime_install::TrustClassification::UserPinnedLimitedTrust => pins::Provenance::UserPinnedLimitedTrust,
                },
            };
            let adapter = adapter.map(|a| match a {
                AdapterArg::UmuProton => pins::AdapterPreference::UmuProton,
                AdapterArg::ManagedWine => pins::AdapterPreference::ManagedWine,
            });
            let pin_id = pins::stable_pin_id(&target, &runtime);
            let pin = pins::RuntimePin::new(pin_id, target, runtime, adapter, now())
                .map_err(|e| e.to_string())?;
            let saved = store
                .set(pin, expected_revision, replace)
                .map_err(|e| e.to_string())?;
            writeln!(out, "Runtime pin saved\nRegistry revision: {}\nAutomatic updates: disabled by policy\nRuntime execution: not performed\nGame execution: not performed", saved.revision).map_err(|e| e.to_string())?;
            Ok(0)
        }
        Operation::Remove {
            scope: kind,
            profile,
            expected_revision,
        } => {
            let machine = pins::current_machine_id().map_err(|e| e.to_string())?;
            let target = scope(kind, profile, machine)?;
            let registry = store
                .remove(&target, expected_revision)
                .map_err(|e| e.to_string())?;
            writeln!(
                out,
                "Runtime pin removed\nRegistry revision: {}\nRuntime installation was not changed.",
                registry.revision
            )
            .map_err(|e| e.to_string())?;
            Ok(0)
        }
        Operation::Resolve { profile } => {
            let machine = pins::current_machine_id().map_err(|e| e.to_string())?;
            let profile = profile
                .map(pins::ProfileKey::new)
                .transpose()
                .map_err(|_| "profile identifier is invalid")?;
            let registry = store.load().map_err(|e| e.to_string())?;
            let report = pins::resolve(&registry, &machine, profile.as_ref(), None, |reference| {
                let paths = endfield_unix_storage::AppPaths::discover()
                    .map_err(|_| "XDG paths unavailable".to_owned())?;
                let identity = endfield_unix_runtime_install::pin_identity(
                    &paths,
                    &reference.installation_id,
                )
                .map_err(|_| {
                    "managed installation unavailable, stale, or integrity audit not complete"
                        .to_owned()
                })?;
                if identity.family != reference.family
                    || identity.lineage != reference.lineage
                    || identity.provider != reference.provider
                    || identity.project != reference.project
                    || identity.build != reference.build
                    || identity.artifact_sha256 != reference.artifact_sha256
                    || identity.manifest_sha256 != reference.manifest_sha256
                    || identity.generation != reference.generation
                    || identity.ownership_record_sha256 != reference.ownership_record_sha256
                {
                    return Err("managed installation identity changed".into());
                }
                use endfield_unix_runtime_audit::{Completeness, FindingKind};
                match identity.audit.completeness {
                    Completeness::Complete
                        if identity
                            .audit
                            .findings
                            .iter()
                            .all(|finding| finding.kind == FindingKind::Match) => {}
                    Completeness::Complete => {
                        return Err("P026 detected a local baseline mismatch".into());
                    }
                    Completeness::Unverifiable => return Err(
                        "P026 integrity is unverifiable because a baseline is missing or legacy"
                            .into(),
                    ),
                    Completeness::Partial => {
                        return Err("P026 audit was partial; integrity is not established".into());
                    }
                    Completeness::Failed => {
                        return Err("P026 audit failed; integrity is not established".into());
                    }
                }
                Ok(())
            });
            match report.state {
                pins::ResolutionState::NoPin => {
                    writeln!(out, "No runtime pin configured.\nSelection: none\nAutomatic fallback: not performed").map_err(|e| e.to_string())?;
                    Ok(3)
                }
                pins::ResolutionState::Stale(reason) => {
                    writeln!(out, "Runtime pin\n\nPin status: stale\nReason: {reason}\nAutomatic fallback: not performed\nStored pin: preserved\nRuntime execution: not performed").map_err(|e| e.to_string())?;
                    Ok(4)
                }
                pins::ResolutionState::Selected => {
                    let runtime = report.runtime.expect("selected has runtime");
                    let adapter_availability = match report
                        .pin_id
                        .as_ref()
                        .and_then(|pin_id| registry.pins.iter().find(|pin| &pin.pin_id == pin_id))
                        .and_then(|pin| pin.adapter_preference)
                    {
                        Some(pins::AdapterPreference::UmuProton) => format!(
                            "{:?}",
                            endfield_unix_runtime_linux::UmuProvider::default()
                                .scan()
                                .declaration
                                .availability()
                        ),
                        Some(pins::AdapterPreference::ManagedWine) => "not evaluated".to_owned(),
                        None => "not selected".to_owned(),
                    };
                    writeln!(out, "Runtime pin\n\nSelection source: {:?}\nRuntime family: {}\nBuild: {}\nInstallation: {}\nPin status: identity valid; P026 local baseline matches\nSource provenance: {:?}\nCompatibility: not verified\nAdapter availability: {}\nAutomatic fallback: not performed\nRuntime execution: not performed\nGame execution: not performed", report.source, runtime.family, runtime.build, runtime.installation_id, runtime.trust, adapter_availability).map_err(|e| e.to_string())?;
                    Ok(0)
                }
            }
        }
    }
}

fn print_pin(out: &mut impl Write, pin: &pins::RuntimePin, revision: u64) -> io::Result<()> {
    let scope = match &pin.scope {
        pins::RuntimePinScope::Machine { .. } => "machine".to_owned(),
        pins::RuntimePinScope::Profile { profile, .. } => format!("profile ({})", profile.as_str()),
    };
    writeln!(
        out,
        "Runtime pin\n\nScope: {scope}\nRuntime: {}\nBuild: {}\nInstallation: {}\nRegistry revision: {revision}\nPin status: stored; identity not revalidated\nSource provenance: {:?}\nCompatibility: not verified\nAdapter preference: {:?}\nIntegrity: not assessed by show\nRuntime execution: not performed\nGame execution: not performed",
        pin.runtime.family,
        pin.runtime.build,
        pin.runtime.installation_id,
        pin.runtime.trust,
        pin.adapter_preference
    )
}
