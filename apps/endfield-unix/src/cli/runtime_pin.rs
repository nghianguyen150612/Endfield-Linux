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
    /// Pin one explicitly selected candidate reported by P022 discovery.
    SetExternal {
        #[arg(long)]
        candidate_id: String,
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long = "root", value_name = "PATH", action = clap::ArgAction::Append)]
        roots: Vec<std::path::PathBuf>,
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
    ExternalWine,
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
                    "{kind}{}: {} / {} / {}",
                    if profile.is_empty() {
                        String::new()
                    } else {
                        format!(" ({profile})")
                    },
                    pin.runtime.family(),
                    pin.runtime.build().unwrap_or("unknown build"),
                    runtime_label(&pin.runtime)
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
                AdapterArg::ExternalWine => pins::AdapterPreference::ExternalWine,
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
        Operation::SetExternal {
            candidate_id,
            scope: kind,
            profile,
            roots,
            expected_revision,
            replace,
            adapter,
        } => {
            if roots.len() > endfield_unix_runtime_linux::MAX_CUSTOM_ROOTS {
                return Err("too many discovery roots were supplied".into());
            }
            let machine = pins::current_machine_id().map_err(|e| e.to_string())?;
            let target = scope(kind, profile, machine)?;
            let provider = endfield_unix_runtime_linux::LinuxRuntimeProvider::new(
                endfield_unix_runtime_linux::DiscoveryOptions {
                    additional_roots: roots,
                    include_path: true,
                    include_conventional: true,
                },
            )
            .map_err(|_| "Linux runtime discovery options are invalid".to_owned())?;
            let report = provider.scan();
            let selected = report
                .runtimes
                .iter()
                .find(|runtime| runtime.candidate.runtime_id().as_str() == candidate_id)
                .ok_or_else(|| "candidate is no longer present in P022 discovery".to_owned())?;
            let snapshot = selected
                .external_snapshot()
                .map_err(|_| "external candidate identity could not be revalidated".to_owned())?;
            let runtime = external_reference(snapshot)?;
            let adapter = adapter.map(|a| match a {
                AdapterArg::UmuProton => pins::AdapterPreference::UmuProton,
                AdapterArg::ManagedWine => pins::AdapterPreference::ManagedWine,
                AdapterArg::ExternalWine => pins::AdapterPreference::ExternalWine,
            });
            let runtime = pins::PinnedRuntimeReference::External(runtime);
            let pin_id = pins::stable_pin_id(&target, &runtime);
            let pin = pins::RuntimePin::new(pin_id, target, runtime, adapter, now())
                .map_err(|e| e.to_string())?;
            let saved = store
                .set(pin, expected_revision, replace)
                .map_err(|e| e.to_string())?;
            writeln!(out, "External runtime pin saved\nRegistry revision: {}\nSource authenticity: unverified\nRuntime integrity: not assessed\nCompatibility: not verified\nAutomatic updates: disabled by policy\nRuntime execution: not performed\nGame execution: not performed", saved.revision).map_err(|e| e.to_string())?;
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
            let report = pins::resolve(
                &registry,
                &machine,
                profile.as_ref(),
                None,
                validate_reference,
            );
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
                        Some(pins::AdapterPreference::ExternalWine) => "not evaluated".to_owned(),
                        None => "not selected".to_owned(),
                    };
                    let (pin_status, ownership, provenance, discovery_source) = match &runtime {
                        pins::PinnedRuntimeReference::Managed(reference) => (
                            "P025 identity valid; P026 local baseline matches",
                            "Endfield-Unix managed",
                            format!("{:?}", reference.trust),
                            "managed installation".to_owned(),
                        ),
                        pins::PinnedRuntimeReference::External(reference) => (
                            "P022 structural identity revalidated; integrity not assessed",
                            reference.ownership.as_str(),
                            "external source authenticity unverified".to_owned(),
                            reference.sources.join(", "),
                        ),
                    };
                    writeln!(out, "Runtime pin\n\nSelection source: {:?}\nRuntime family: {}\nBuild: {}\nInstallation: {}\nPin status: {pin_status}\nOwnership: {ownership}\nDiscovery source: {discovery_source}\nSource provenance: {provenance}\nCompatibility: not verified\nAdapter availability: {}\nAutomatic fallback: not performed\nRuntime execution: not performed\nGame execution: not performed", report.source, runtime.family(), runtime.build().unwrap_or("unknown"), runtime_label(&runtime), adapter_availability).map_err(|e| e.to_string())?;
                    Ok(0)
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PrefixRuntimeResolution {
    pub runtime: pins::PinnedRuntimeReference,
    pub source: super::prefix::RuntimeScope,
    pub pin_id: String,
    pub revision: u64,
    pub adapter_preference: Option<pins::AdapterPreference>,
}

pub(crate) fn resolve_for_prefix(profile: Option<&str>) -> Result<PrefixRuntimeResolution, String> {
    let machine = pins::current_machine_id().map_err(|error| error.to_string())?;
    let profile = profile
        .map(|value| pins::ProfileKey::new(value.to_owned()))
        .transpose()
        .map_err(|_| "profile identifier is invalid".to_owned())?;
    let registry = store()?.load().map_err(|error| error.to_string())?;
    let report = pins::resolve(
        &registry,
        &machine,
        profile.as_ref(),
        None,
        validate_reference,
    );
    let runtime = match report.state {
        pins::ResolutionState::Selected => report
            .runtime
            .ok_or_else(|| "P027 selected a runtime without preserving its identity".to_owned())?,
        pins::ResolutionState::NoPin => {
            return Err("P027 has no runtime pin for the requested machine/profile".into());
        }
        pins::ResolutionState::Stale(reason) => {
            return Err(format!("P027 selected pin is blocked or stale: {reason}"));
        }
    };
    let pin_id = report
        .pin_id
        .ok_or_else(|| "P027 selection has no persistent pin identity".to_owned())?;
    let pin = registry
        .pins
        .iter()
        .find(|pin| pin.pin_id == pin_id)
        .ok_or_else(|| "P027 selected pin disappeared during resolution".to_owned())?;
    let source = match report.source {
        pins::SelectionSource::ProfilePin => super::prefix::RuntimeScope::Profile,
        pins::SelectionSource::MachinePin => super::prefix::RuntimeScope::Machine,
        pins::SelectionSource::ExplicitOverride | pins::SelectionSource::None => {
            return Err("P027 did not report a persistent machine/profile pin".into());
        }
    };
    Ok(PrefixRuntimeResolution {
        runtime,
        source,
        pin_id,
        revision: report.registry_revision,
        adapter_preference: pin.adapter_preference,
    })
}

pub(crate) fn validate_reference(reference: &pins::PinnedRuntimeReference) -> Result<(), String> {
    match reference {
        pins::PinnedRuntimeReference::Managed(reference) => {
            let paths = endfield_unix_storage::AppPaths::discover()
                .map_err(|_| "XDG paths unavailable".to_owned())?;
            let identity =
                endfield_unix_runtime_install::pin_identity(&paths, &reference.installation_id)
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
                        .all(|finding| finding.kind == FindingKind::Match) =>
                {
                    Ok(())
                }
                Completeness::Complete => Err("P026 detected a local baseline mismatch".into()),
                Completeness::Unverifiable => Err(
                    "P026 integrity is unverifiable because a baseline is missing or legacy".into(),
                ),
                Completeness::Partial => {
                    Err("P026 audit was partial; integrity is not established".into())
                }
                Completeness::Failed => {
                    Err("P026 audit failed; integrity is not established".into())
                }
            }
        }
        pins::PinnedRuntimeReference::External(reference) => revalidate_external(reference),
    }
}

fn print_pin(out: &mut impl Write, pin: &pins::RuntimePin, revision: u64) -> io::Result<()> {
    let scope = match &pin.scope {
        pins::RuntimePinScope::Machine { .. } => "machine".to_owned(),
        pins::RuntimePinScope::Profile { profile, .. } => format!("profile ({})", profile.as_str()),
    };
    let (provenance, ownership, discovery_source) = match &pin.runtime {
        pins::PinnedRuntimeReference::Managed(reference) => (
            format!("{:?}", reference.trust),
            "Endfield-Unix managed",
            "managed installation".to_owned(),
        ),
        pins::PinnedRuntimeReference::External(reference) => (
            "external source authenticity unverified".to_owned(),
            reference.ownership.as_str(),
            reference.sources.join(", "),
        ),
    };
    writeln!(
        out,
        "Runtime pin\n\nScope: {scope}\nRuntime: {}\nBuild: {}\nInstallation: {}\nOwnership: {ownership}\nDiscovery source: {discovery_source}\nRegistry revision: {revision}\nPin status: stored; identity not revalidated\nSource provenance: {provenance}\nCompatibility: not verified\nAdapter preference: {:?}\nIntegrity: not assessed by show\nRuntime execution: not performed\nGame execution: not performed",
        pin.runtime.family(),
        pin.runtime.build().unwrap_or("unknown"),
        runtime_label(&pin.runtime),
        pin.adapter_preference
    )
}

fn runtime_label(runtime: &pins::PinnedRuntimeReference) -> &str {
    match runtime {
        pins::PinnedRuntimeReference::Managed(reference) => &reference.installation_id,
        pins::PinnedRuntimeReference::External(_) => "external candidate",
    }
}

fn external_reference(
    snapshot: endfield_unix_runtime_linux::ExternalRuntimeSnapshot,
) -> Result<pins::ExternalRuntimeReference, String> {
    let location = snapshot
        .location
        .to_str()
        .ok_or_else(|| "candidate location is not valid UTF-8".to_owned())?
        .to_owned();
    let mut sources = snapshot
        .sources
        .iter()
        .map(|source| source.id().to_owned())
        .collect::<Vec<_>>();
    sources.sort();
    let mut architectures = snapshot
        .architectures
        .iter()
        .map(|architecture| architecture.as_str().to_owned())
        .collect::<Vec<_>>();
    architectures.sort();
    architectures.dedup();
    let reference = pins::ExternalRuntimeReference {
        location,
        family: snapshot.family.as_str().to_owned(),
        lineage: snapshot.lineage,
        lineage_basis: snapshot.lineage_basis,
        provider: snapshot.provider,
        sources,
        build: snapshot.build,
        architectures,
        structure: snapshot.structure,
        ownership: snapshot.ownership,
        identity_sha256: snapshot.identity_sha256,
        trust: pins::Provenance::UnknownExternalSource,
    };
    reference
        .validate()
        .map_err(|_| "external candidate identity is invalid".to_owned())?;
    Ok(reference)
}

fn revalidate_external(reference: &pins::ExternalRuntimeReference) -> Result<(), String> {
    let location = std::path::PathBuf::from(&reference.location);
    let provider = endfield_unix_runtime_linux::LinuxRuntimeProvider::new(
        endfield_unix_runtime_linux::DiscoveryOptions {
            additional_roots: vec![location.clone()],
            include_path: false,
            include_conventional: false,
        },
    )
    .map_err(|_| "external runtime discovery could not be configured".to_owned())?;
    let report = provider.scan();
    let candidate = report
        .runtimes
        .iter()
        .find(|runtime| runtime.path() == location)
        .ok_or_else(|| "selected external runtime is missing or no longer recognized".to_owned())?;
    let current = candidate
        .external_snapshot()
        .map_err(|_| "selected external runtime changed during inspection".to_owned())?;
    if current.identity_sha256 != reference.identity_sha256
        || current.family.as_str() != reference.family
        || current.lineage != reference.lineage
        || current.lineage_basis != reference.lineage_basis
        || current.provider != reference.provider
        || current.build != reference.build
        || current.structure != reference.structure
        || current
            .architectures
            .iter()
            .map(|architecture| architecture.as_str())
            .collect::<Vec<_>>()
            != reference
                .architectures
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
    {
        return Err("selected external runtime identity changed".into());
    }
    Ok(())
}
