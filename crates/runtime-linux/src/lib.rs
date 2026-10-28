//! Bounded, read-only discovery of locally installed Wine and Proton runtimes.
//!
//! Discovery never executes a candidate or accesses the network. A recognized
//! layout is local structural evidence only; it does not establish origin,
//! integrity, launch readiness, or game compatibility.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use endfield_unix_runtime::{
    AuthenticityState, CapabilityStatus, DiscoveryCompleteness, DiscoveryQuery, DiscoveryReport,
    Identifier, MetadataVerification, ProcessArchitecture, ProviderAvailability,
    ProviderCapability, ProviderCapabilityKind, ProviderDeclaration, ProviderError,
    ProviderErrorKind, ProviderId, ProviderOperation, RuntimeCandidate, RuntimeFamily, RuntimeId,
    RuntimeIdentity, RuntimeLineage, RuntimeOwnership, RuntimePlatform, RuntimeProvider,
    RuntimeRequirements, RuntimeVersion, SourceCategory, SourceIdentity, SourceMetadata,
    SourceVerification, SourceVerificationState,
};

pub const PROVIDER_ID: &str = "linux-runtime-discovery";
pub const MAX_ROOTS: usize = 32;
pub const MAX_CUSTOM_ROOTS: usize = 8;
pub const MAX_ENTRIES_PER_DIRECTORY: usize = 512;
pub const MAX_METADATA_BYTES: u64 = 256 * 1024;
pub const MAX_SYMLINK_DEPTH: usize = 8;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_DISCOVERY_ISSUES: usize = 128;
pub const MAX_RUNTIME_CANDIDATES: usize = endfield_unix_runtime::validation::MAX_RUNTIME_CANDIDATES;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceKind {
    SystemWine,
    PathWine,
    NativeSteam,
    FlatpakSteam,
    Heroic,
    Lutris,
    Explicit,
    UnknownExternalManager,
}

impl SourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemWine => "system executable",
            Self::PathWine => "PATH executable",
            Self::NativeSteam => "Steam compatibility tools",
            Self::FlatpakSteam => "Flatpak Steam",
            Self::Heroic => "Heroic managed runtime",
            Self::Lutris => "Lutris Wine runner",
            Self::Explicit => "explicit runtime directory",
            Self::UnknownExternalManager => "unknown external manager",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceReference {
    pub source: SourceKind,
    pub label: &'static str,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveredRuntime {
    pub candidate: RuntimeCandidate,
    pub lineage: RuntimeLineage,
    pub lineage_basis: &'static str,
    pub version: Option<RuntimeVersion>,
    pub architectures: Vec<ProcessArchitecture>,
    pub sources: Vec<SourceReference>,
    pub structure: &'static str,
    pub display_label: String,
    path: PathBuf,
    ownership: RuntimeOwnership,
    observed_files: Vec<FileObservation>,
}

impl fmt::Debug for DiscoveredRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscoveredRuntime")
            .field("candidate", &self.candidate)
            .field("lineage", &self.lineage)
            .field("lineage_basis", &self.lineage_basis)
            .field("version", &self.version)
            .field("architectures", &self.architectures)
            .field("sources", &self.sources)
            .field("structure", &self.structure)
            .field("display_label", &self.display_label)
            .field("ownership", &self.ownership)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryIssue {
    pub source: SourceKind,
    pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDiscoveryReport {
    pub contract: DiscoveryReport,
    pub runtimes: Vec<DiscoveredRuntime>,
    pub issues: Vec<DiscoveryIssue>,
    pub roots: Vec<RootAttempt>,
}

impl LinuxDiscoveryReport {
    /// True only when every applicable selected source failed inspection.
    /// Absence, partial inspection, and limit truncation are not source failures.
    pub fn all_sources_failed(&self) -> bool {
        let mut applicable = false;
        for root in self
            .roots
            .iter()
            .filter(|root| root.outcome != RootOutcome::Skipped)
        {
            applicable = true;
            if root.outcome != RootOutcome::Failed {
                return false;
            }
        }
        if applicable {
            true
        } else {
            // Collection can fail before a concrete root is registered (for
            // example, an oversized PATH hint). With no successful root
            // observations, that is a failed selected source rather than an
            // empty inventory.
            !self.issues.is_empty()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootOutcome {
    /// The location was checked and does not exist; this is a successful absence observation.
    NotPresent,
    /// The location was fully inspected without source errors.
    Inspected,
    /// The location was inspected, with one or more item-level issues reported.
    PartiallyInspected,
    /// The location was intentionally omitted by a documented provider policy.
    Skipped,
    /// The location could not be safely inspected.
    Failed,
    /// The location was only partly considered because a discovery resource limit was reached.
    Truncated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootAttempt {
    pub source: SourceKind,
    pub root_type: &'static str,
    pub ownership_expectation: RuntimeOwnership,
    pub available: bool,
    pub attempted: bool,
    pub completed: bool,
    pub outcome: RootOutcome,
    pub reason: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryOptions {
    pub additional_roots: Vec<PathBuf>,
    pub include_path: bool,
    /// Disable conventional user roots for deterministic isolated test scans.
    pub include_conventional: bool,
}

/// Environment hints used to select conventional runtime roots and PATH Wine.
///
/// `from_process` is used by the CLI. `isolated` lets tests supply synthetic
/// HOME/PATH values without changing the process environment or inspecting the
/// developer's real runtime directories.
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveryEnvironment {
    home: Option<PathBuf>,
    path: Option<OsString>,
    xdg_config_home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    include_system_wine: bool,
}

impl fmt::Debug for DiscoveryEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscoveryEnvironment")
            .field("home_configured", &self.home.is_some())
            .field("path_configured", &self.path.is_some())
            .field(
                "xdg_config_home_configured",
                &self.xdg_config_home.is_some(),
            )
            .field("xdg_data_home_configured", &self.xdg_data_home.is_some())
            .field("include_system_wine", &self.include_system_wine)
            .finish()
    }
}

impl DiscoveryEnvironment {
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            path: std::env::var_os("PATH"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            include_system_wine: true,
        }
    }

    pub fn isolated(home: Option<PathBuf>, path: Option<OsString>) -> Self {
        Self {
            home,
            path,
            xdg_config_home: None,
            xdg_data_home: None,
            include_system_wine: false,
        }
    }

    pub fn with_xdg_homes(
        mut self,
        config_home: Option<PathBuf>,
        data_home: Option<PathBuf>,
    ) -> Self {
        self.xdg_config_home = config_home;
        self.xdg_data_home = data_home;
        self
    }

    pub fn with_system_wine(mut self, include: bool) -> Self {
        self.include_system_wine = include;
        self
    }
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            additional_roots: Vec::new(),
            include_path: true,
            include_conventional: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Root {
    path: PathBuf,
    source: SourceKind,
    kind: RootKind,
}

impl Root {
    fn ownership_expectation(&self) -> RuntimeOwnership {
        match self.source {
            SourceKind::SystemWine
                if self.path.starts_with("/usr/bin") || self.path.starts_with("/usr/lib") =>
            {
                RuntimeOwnership::DistributionManaged
            }
            SourceKind::NativeSteam | SourceKind::FlatpakSteam => {
                RuntimeOwnership::ExternalUserManaged
            }
            SourceKind::Heroic | SourceKind::Lutris => other_manager_ownership(self.source),
            SourceKind::SystemWine
            | SourceKind::PathWine
            | SourceKind::Explicit
            | SourceKind::UnknownExternalManager => RuntimeOwnership::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RootKind {
    WineExecutable,
    ProtonDirectory,
    SteamLibraryProton,
    RuntimeDirectory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryConfigError {
    TooManyCustomRoots,
    InvalidCustomRoot,
}

#[derive(Debug)]
struct CandidatePathInfo {
    path: PathBuf,
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    version: Option<RuntimeVersion>,
    architectures: Vec<ProcessArchitecture>,
    ownership: RuntimeOwnership,
    metadata: fs::Metadata,
    observed_files: Vec<FileObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileObservation {
    path: PathBuf,
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    is_file: bool,
    is_directory: bool,
}

impl FileObservation {
    fn from_metadata(path: &Path, metadata: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
        }
    }

    fn is_unchanged(&self) -> bool {
        fs::symlink_metadata(&self.path).is_ok_and(|current| same_file_observation(self, &current))
    }
}

pub struct LinuxRuntimeProvider {
    declaration: ProviderDeclaration,
    options: DiscoveryOptions,
    environment: Option<DiscoveryEnvironment>,
    last_paths: Mutex<BTreeMap<RuntimeId, CandidatePathInfo>>,
}

impl fmt::Debug for LinuxRuntimeProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cached_identities = self
            .last_paths
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len();
        formatter
            .debug_struct("LinuxRuntimeProvider")
            .field("declaration", &self.declaration)
            .field("custom_root_count", &self.options.additional_roots.len())
            .field("include_path", &self.options.include_path)
            .field("include_conventional", &self.options.include_conventional)
            .field("environment_injected", &self.environment.is_some())
            .field("cached_identities", &cached_identities)
            .finish()
    }
}

impl LinuxRuntimeProvider {
    pub fn new(options: DiscoveryOptions) -> Result<Self, DiscoveryConfigError> {
        Self::new_inner(options, None)
    }

    pub fn new_with_environment(
        options: DiscoveryOptions,
        environment: DiscoveryEnvironment,
    ) -> Result<Self, DiscoveryConfigError> {
        Self::new_inner(options, Some(environment))
    }

    fn new_inner(
        options: DiscoveryOptions,
        environment: Option<DiscoveryEnvironment>,
    ) -> Result<Self, DiscoveryConfigError> {
        if options.additional_roots.len() > MAX_CUSTOM_ROOTS
            || options.additional_roots.iter().any(|p| {
                !p.is_absolute()
                    || p.as_os_str().len() > MAX_PATH_BYTES
                    || p.components().any(|c| c == std::path::Component::ParentDir)
            })
        {
            return Err(if options.additional_roots.len() > MAX_CUSTOM_ROOTS {
                DiscoveryConfigError::TooManyCustomRoots
            } else {
                DiscoveryConfigError::InvalidCustomRoot
            });
        }
        let provider_id = ProviderId::new(PROVIDER_ID).expect("fixed provider ID is valid");
        let implemented = CapabilityStatus::ImplementedByProvider;
        let declaration = ProviderDeclaration::new(
            provider_id,
            ProviderAvailability::Available,
            vec![RuntimeFamily::Wine, RuntimeFamily::Proton],
            vec![
                ProviderCapability::new(ProviderCapabilityKind::DiscoverBuilds, implemented),
                ProviderCapability::new(ProviderCapabilityKind::InspectIdentity, implemented),
                ProviderCapability::new(
                    ProviderCapabilityKind::VerifySource,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::VerifyIntegrity,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::AssessCompatibility,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::PlanInstallation,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::PrepareLaunchPlan,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::ManageInstallations,
                    CapabilityStatus::Unsupported,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::ManageUpdates,
                    CapabilityStatus::Unsupported,
                ),
            ],
        )
        .expect("fixed declaration is valid");
        Ok(Self {
            declaration,
            options,
            environment,
            last_paths: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn scan(&self) -> LinuxDiscoveryReport {
        let mut issues = Vec::new();
        let mut found: BTreeMap<PathBuf, DiscoveredRuntime> = BTreeMap::new();
        let mut root_reports = Vec::new();
        let environment = self
            .environment
            .clone()
            .unwrap_or_else(DiscoveryEnvironment::from_process);
        let mut roots = collect_roots(&self.options, &environment, &mut issues);
        roots.extend(collect_path_roots(&self.options, &environment, &mut issues));
        roots.sort_by_key(|root| (root_priority(root), root.path.clone()));
        let mut seen_roots = BTreeSet::new();
        roots.retain(|root| seen_roots.insert((root.path.clone(), root.source, root.kind)));

        // Absence probes do not consume the bounded directory-scan budget.
        // This keeps ordinary missing XDG/Steam paths and missing PATH hints
        // from displacing explicit roots or exhausting MAX_ROOTS.
        let mut present_roots = Vec::new();
        for root in roots {
            if root.path.as_os_str().len() > MAX_PATH_BYTES {
                issues.push(DiscoveryIssue {
                    source: root.source,
                    reason: "discovery-path-limit",
                });
                root_reports.push(root_attempt(
                    &root,
                    false,
                    false,
                    false,
                    RootOutcome::Truncated,
                    Some("discovery-path-limit"),
                ));
                continue;
            }
            match fs::symlink_metadata(&root.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    root_reports.push(root_attempt(
                        &root,
                        false,
                        true,
                        true,
                        RootOutcome::NotPresent,
                        None,
                    ));
                }
                Err(error) => {
                    let discovery_issue = issue(root.source, &error);
                    root_reports.push(root_attempt(
                        &root,
                        false,
                        true,
                        false,
                        RootOutcome::Failed,
                        Some(discovery_issue.reason),
                    ));
                    issues.push(discovery_issue);
                }
                Ok(_) => present_roots.push(root),
            }
        }

        present_roots.sort_by_key(|root| (root_priority(root), root.path.clone()));
        if present_roots.len() > MAX_ROOTS {
            let truncated = present_roots.split_off(MAX_ROOTS);
            for root in truncated {
                root_reports.push(root_attempt(
                    &root,
                    true,
                    false,
                    false,
                    RootOutcome::Truncated,
                    Some("discovery-root-limit"),
                ));
            }
            issues.push(DiscoveryIssue {
                source: SourceKind::PathWine,
                reason: "discovery-root-limit",
            });
        }

        for root in present_roots {
            let issue_start = issues.len();
            if root.source == SourceKind::SystemWine || root.source == SourceKind::PathWine {
                let runtime = inspect_wine_executable(&root.path, root.source, &mut issues);
                let candidate_identified = runtime.is_some();
                if let Some(runtime) = runtime {
                    merge_runtime(&mut found, runtime, &mut issues);
                }
                let has_issues = issues.len() != issue_start;
                let outcome = match (candidate_identified, has_issues) {
                    (true, true) => RootOutcome::PartiallyInspected,
                    (false, true) => RootOutcome::Failed,
                    (_, false) => RootOutcome::Inspected,
                };
                root_reports.push(root_attempt(
                    &root,
                    true,
                    true,
                    matches!(
                        outcome,
                        RootOutcome::Inspected | RootOutcome::PartiallyInspected
                    ),
                    outcome,
                    issues.get(issue_start).map(|issue| issue.reason),
                ));
                continue;
            }
            let outcome = inspect_root(&root, &mut found, &mut issues);
            let reason = issues
                .get(issue_start)
                .map(|issue| issue.reason)
                .or(match outcome {
                    RootOutcome::Truncated => Some("discovery-root-limit"),
                    _ => None,
                });
            root_reports.push(root_attempt(
                &root,
                true,
                true,
                matches!(
                    outcome,
                    RootOutcome::Inspected | RootOutcome::PartiallyInspected
                ),
                outcome,
                reason,
            ));
        }
        let runtimes: Vec<_> = found.into_values().collect();
        let mut stamps = BTreeMap::new();
        let mut seen_ids = BTreeSet::new();
        let mut validated_runtimes = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            let source = runtime
                .sources
                .first()
                .map_or(SourceKind::Explicit, |reference| reference.source);
            if !seen_ids.insert(runtime.candidate.runtime_id().clone()) {
                issues.push(DiscoveryIssue {
                    source,
                    reason: "runtime-id-collision",
                });
                continue;
            }
            let unchanged = runtime
                .observed_files
                .iter()
                .all(FileObservation::is_unchanged);
            match fs::metadata(&runtime.path) {
                Ok(metadata) if unchanged => {
                    stamps.insert(runtime.candidate.runtime_id().clone(), metadata);
                    validated_runtimes.push(runtime);
                }
                _ => {
                    issues.push(DiscoveryIssue {
                        source,
                        reason: "runtime-changed-during-scan",
                    });
                }
            }
        }
        let mut runtimes = validated_runtimes;
        runtimes.sort_by(|a, b| a.candidate.runtime_id().cmp(b.candidate.runtime_id()));
        if runtimes.len() > MAX_RUNTIME_CANDIDATES {
            runtimes.truncate(MAX_RUNTIME_CANDIDATES);
            issues.push(DiscoveryIssue {
                source: SourceKind::Explicit,
                reason: "runtime-candidate-limit",
            });
        }
        if issues.len() > MAX_DISCOVERY_ISSUES {
            issues.truncate(MAX_DISCOVERY_ISSUES - 1);
            issues.push(DiscoveryIssue {
                source: SourceKind::Explicit,
                reason: "discovery-issue-limit",
            });
        }
        let mut paths = self.last_paths.lock().unwrap_or_else(|e| e.into_inner());
        paths.clear();
        for runtime in &runtimes {
            if let Some(metadata) = stamps.remove(runtime.candidate.runtime_id()) {
                paths.insert(
                    runtime.candidate.runtime_id().clone(),
                    CandidatePathInfo {
                        path: runtime.path.clone(),
                        family: runtime.candidate.family(),
                        lineage: runtime.lineage.clone(),
                        version: runtime.version.clone(),
                        architectures: runtime.architectures.clone(),
                        ownership: runtime.ownership.clone(),
                        metadata,
                        observed_files: runtime.observed_files.clone(),
                    },
                );
            }
        }
        let candidates = runtimes.iter().map(|r| r.candidate.clone()).collect();
        let completeness = if issues.is_empty() {
            DiscoveryCompleteness::Complete
        } else {
            DiscoveryCompleteness::Partial {
                reason: Identifier::new("one-or-more-sources-unavailable").unwrap(),
            }
        };
        let contract = DiscoveryReport::new(
            self.declaration.provider_id().clone(),
            candidates,
            completeness,
        )
        .expect("discovery candidates satisfy P021");
        LinuxDiscoveryReport {
            contract,
            runtimes,
            issues,
            roots: root_reports,
        }
    }
}

impl RuntimeProvider for LinuxRuntimeProvider {
    fn declaration(&self) -> &ProviderDeclaration {
        &self.declaration
    }

    fn discover_impl(&self, query: &DiscoveryQuery) -> Result<DiscoveryReport, ProviderError> {
        let report = self.scan();
        if report.all_sources_failed() {
            return Err(self.error(
                ProviderOperation::Discover,
                ProviderErrorKind::ProviderUnavailable,
            ));
        }
        if query.families().is_empty() {
            return Ok(report.contract);
        }
        let selected = report
            .contract
            .candidates()
            .iter()
            .filter(|c| query.families().contains(&c.family()))
            .cloned()
            .collect();
        DiscoveryReport::new(
            self.declaration.provider_id().clone(),
            selected,
            report.contract.completeness().clone(),
        )
        .map_err(|_| {
            self.error(
                ProviderOperation::Discover,
                ProviderErrorKind::InvalidMetadata,
            )
        })
    }

    fn inspect_identity_impl(
        &self,
        candidate: &RuntimeCandidate,
    ) -> Result<RuntimeIdentity, ProviderError> {
        let paths = self.last_paths.lock().unwrap_or_else(|e| e.into_inner());
        let Some(info) = paths.get(candidate.runtime_id()) else {
            return Err(self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::MissingMetadata,
            ));
        };
        if candidate.provider_id() != self.declaration.provider_id() {
            return Err(self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::InvalidMetadata,
            ));
        }
        if candidate.family() != info.family
            || candidate.version_hint() != info.version.as_ref()
            || candidate.ownership() != &info.ownership
        {
            return Err(self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::InvalidMetadata,
            ));
        }
        let current = fs::metadata(&info.path).map_err(|_| {
            self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::ProviderUnavailable,
            )
        })?;
        if !same_file(&info.metadata, &current)
            || info.observed_files.iter().any(|file| !file.is_unchanged())
        {
            return Err(self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::ProviderUnavailable,
            ));
        }
        let Some(version) = info.version.clone() else {
            return Err(self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::MissingMetadata,
            ));
        };
        let source = SourceMetadata::new(
            SourceIdentity::new(
                self.declaration.provider_id().clone(),
                SourceCategory::ExternalInstallation,
                None,
                None,
                None,
            )
            .map_err(|_| {
                self.error(
                    ProviderOperation::InspectIdentity,
                    ProviderErrorKind::InvalidMetadata,
                )
            })?,
            SourceVerification::new(
                SourceVerificationState::CandidateIdentified,
                AuthenticityState::NotAssessed,
                MetadataVerification::NotVerified,
                None,
                None,
                None,
            )
            .map_err(|_| {
                self.error(
                    ProviderOperation::InspectIdentity,
                    ProviderErrorKind::InvalidMetadata,
                )
            })?,
        )
        .map_err(|_| {
            self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::InvalidMetadata,
            )
        })?;
        RuntimeIdentity::new(
            info.family,
            info.lineage.clone(),
            self.declaration.provider_id().clone(),
            version,
            None,
            RuntimeRequirements::new([RuntimePlatform::Linux], info.architectures.clone()).unwrap(),
            Some(source),
            None,
            info.ownership.clone(),
        )
        .map_err(|_| {
            self.error(
                ProviderOperation::InspectIdentity,
                ProviderErrorKind::InvalidMetadata,
            )
        })
    }
}

impl LinuxRuntimeProvider {
    fn error(&self, operation: ProviderOperation, kind: ProviderErrorKind) -> ProviderError {
        ProviderError::new(
            self.declaration.provider_id().identifier().clone(),
            operation,
            kind,
        )
    }
}

fn collect_roots(
    options: &DiscoveryOptions,
    environment: &DiscoveryEnvironment,
    issues: &mut Vec<DiscoveryIssue>,
) -> Vec<Root> {
    let mut roots = Vec::new();
    if environment.include_system_wine {
        for p in [
            "/usr/bin/wine",
            "/usr/bin/wine64",
            "/usr/local/bin/wine",
            "/usr/local/bin/wine64",
        ] {
            roots.push(Root {
                path: p.into(),
                source: SourceKind::SystemWine,
                kind: RootKind::WineExecutable,
            });
        }
    }
    if options.include_conventional {
        conventional_roots(&mut roots, environment, issues);
    }
    for path in &options.additional_roots {
        roots.push(Root {
            path: path.clone(),
            source: SourceKind::Explicit,
            kind: RootKind::RuntimeDirectory,
        });
    }
    roots
}

fn root_priority(root: &Root) -> u8 {
    match (root.source, root.kind) {
        (SourceKind::SystemWine, _) => 0,
        (SourceKind::Explicit, _) => 1,
        (_, RootKind::SteamLibraryProton) => 7,
        (SourceKind::NativeSteam, _) => 2,
        (SourceKind::FlatpakSteam, _) => 3,
        (SourceKind::Heroic, _) => 4,
        (SourceKind::Lutris, _) => 5,
        (SourceKind::UnknownExternalManager, _) => 6,
        (SourceKind::PathWine, _) => 8,
    }
}

fn root_attempt(
    root: &Root,
    available: bool,
    attempted: bool,
    completed: bool,
    outcome: RootOutcome,
    reason: Option<&'static str>,
) -> RootAttempt {
    RootAttempt {
        source: root.source,
        root_type: match (root.source, root.kind) {
            (SourceKind::SystemWine | SourceKind::PathWine, _) => "wine executable",
            (_, RootKind::ProtonDirectory | RootKind::SteamLibraryProton) => "proton directory",
            (_, RootKind::RuntimeDirectory) => "runtime directory",
            (_, RootKind::WineExecutable) => "wine executable",
        },
        ownership_expectation: root.ownership_expectation(),
        available,
        attempted,
        completed,
        outcome,
        reason,
    }
}

fn collect_path_roots(
    options: &DiscoveryOptions,
    environment: &DiscoveryEnvironment,
    issues: &mut Vec<DiscoveryIssue>,
) -> Vec<Root> {
    if !options.include_path {
        return Vec::new();
    }
    let Some(path) = environment.path.as_ref() else {
        return Vec::new();
    };
    if path.as_encoded_bytes().len() > 16 * 1024 {
        issues.push(DiscoveryIssue {
            source: SourceKind::PathWine,
            reason: "path-hint-too-large",
        });
        return Vec::new();
    }
    let mut dirs = std::env::split_paths(&path);
    let mut roots = Vec::new();
    for _ in 0..64 {
        let Some(dir) = dirs.next() else {
            break;
        };
        if dir.as_os_str().is_empty() || !dir.is_absolute() {
            continue;
        }
        for name in ["wine", "wine64"] {
            let candidate = dir.join(name);
            if candidate.as_os_str().len() > MAX_PATH_BYTES {
                issues.push(DiscoveryIssue {
                    source: SourceKind::PathWine,
                    reason: "path-limit",
                });
                continue;
            }
            roots.push(Root {
                path: candidate,
                source: SourceKind::PathWine,
                kind: RootKind::WineExecutable,
            });
        }
    }
    if dirs.next().is_some() {
        issues.push(DiscoveryIssue {
            source: SourceKind::PathWine,
            reason: "path-entry-limit",
        });
    }
    roots
}

fn conventional_roots(
    roots: &mut Vec<Root>,
    environment: &DiscoveryEnvironment,
    issues: &mut Vec<DiscoveryIssue>,
) {
    let Some(h) = environment.home.as_ref() else {
        return;
    };
    if !h.is_absolute()
        || h.as_os_str().len() > MAX_PATH_BYTES
        || h.components().any(|c| c == std::path::Component::ParentDir)
    {
        issues.push(DiscoveryIssue {
            source: SourceKind::NativeSteam,
            reason: "home-root-invalid",
        });
        return;
    }
    for (base, source) in [
        (".steam/root", SourceKind::NativeSteam),
        (".steam/steam", SourceKind::NativeSteam),
        (".local/share/Steam", SourceKind::NativeSteam),
        (
            ".var/app/com.valvesoftware.Steam/data/Steam",
            SourceKind::FlatpakSteam,
        ),
    ] {
        match steam_library_roots(&h.join(base), source) {
            Ok(found) => roots.extend(found),
            Err(reason) => issues.push(DiscoveryIssue { source, reason }),
        }
    }
    if let Some(config) = environment.xdg_config_home.as_ref() {
        if valid_xdg_root(config) {
            for p in ["heroic/tools/wine", "heroic/tools/proton"] {
                roots.push(Root {
                    path: config.join(p),
                    source: SourceKind::Heroic,
                    kind: RootKind::RuntimeDirectory,
                });
            }
        } else {
            issues.push(DiscoveryIssue {
                source: SourceKind::Heroic,
                reason: "xdg-config-root-invalid",
            });
        }
    }
    if let Some(data) = environment.xdg_data_home.as_ref() {
        if valid_xdg_root(data) {
            for p in ["Steam/compatibilitytools.d", "Steam/steamapps/common"] {
                roots.push(Root {
                    path: data.join(p),
                    source: SourceKind::NativeSteam,
                    kind: RootKind::ProtonDirectory,
                });
            }
            roots.push(Root {
                path: data.join("lutris/runners/wine"),
                source: SourceKind::Lutris,
                kind: RootKind::RuntimeDirectory,
            });
            for p in ["heroic/tools/wine", "heroic/tools/proton"] {
                roots.push(Root {
                    path: data.join(p),
                    source: SourceKind::Heroic,
                    kind: RootKind::RuntimeDirectory,
                });
            }
            match steam_library_roots(&data.join("Steam"), SourceKind::NativeSteam) {
                Ok(found) => roots.extend(found),
                Err(reason) => issues.push(DiscoveryIssue {
                    source: SourceKind::NativeSteam,
                    reason,
                }),
            }
        } else {
            issues.push(DiscoveryIssue {
                source: SourceKind::Lutris,
                reason: "xdg-data-root-invalid",
            });
        }
    }
    let mut add = |relative: &str, source, kind| {
        roots.push(Root {
            path: h.join(relative),
            source,
            kind,
        })
    };
    for p in [
        ".steam/root/compatibilitytools.d",
        ".steam/steam/compatibilitytools.d",
        ".local/share/Steam/compatibilitytools.d",
        ".steam/root/steamapps/common",
        ".steam/steam/steamapps/common",
        ".local/share/Steam/steamapps/common",
    ] {
        add(p, SourceKind::NativeSteam, RootKind::ProtonDirectory);
    }
    for p in [
        ".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d",
        ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/common",
    ] {
        add(p, SourceKind::FlatpakSteam, RootKind::ProtonDirectory);
    }
    for p in [
        ".config/heroic/tools/wine",
        ".config/heroic/tools/proton",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic/tools/wine",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic/tools/proton",
        ".var/app/com.heroicgameslauncher.hgl/data/heroic/tools/wine",
        ".var/app/com.heroicgameslauncher.hgl/data/heroic/tools/proton",
    ] {
        add(p, SourceKind::Heroic, RootKind::RuntimeDirectory);
    }
    add(
        ".var/app/net.lutris.Lutris/data/lutris/runners/wine",
        SourceKind::Lutris,
        RootKind::RuntimeDirectory,
    );
    add(
        ".local/share/lutris/runners/wine",
        SourceKind::Lutris,
        RootKind::RuntimeDirectory,
    );
}

fn valid_xdg_root(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= MAX_PATH_BYTES
        && !path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
}

fn inspect_root(
    root: &Root,
    found: &mut BTreeMap<PathBuf, DiscoveredRuntime>,
    issues: &mut Vec<DiscoveryIssue>,
) -> RootOutcome {
    let issue_start = issues.len();
    let metadata = match fs::symlink_metadata(&root.path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            issues.push(DiscoveryIssue {
                source: root.source,
                reason: "runtime-changed-during-scan",
            });
            return RootOutcome::Failed;
        }
        Err(e) => {
            issues.push(issue(root.source, &e));
            return RootOutcome::Failed;
        }
    };
    let scan_path = if metadata.file_type().is_symlink() {
        if root.source == SourceKind::Explicit {
            issues.push(DiscoveryIssue {
                source: root.source,
                reason: "unsafe-symlink-root",
            });
            return RootOutcome::Failed;
        }
        match resolve_alias(&root.path, None) {
            Ok(path) => path,
            Err(_) => {
                issues.push(DiscoveryIssue {
                    source: root.source,
                    reason: "unsafe-symlink",
                });
                return RootOutcome::Failed;
            }
        }
    } else {
        root.path.clone()
    };
    let resolved_metadata = match fs::metadata(&scan_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            issues.push(issue(root.source, &error));
            return RootOutcome::Failed;
        }
    };
    if !resolved_metadata.is_dir() {
        issues.push(DiscoveryIssue {
            source: root.source,
            reason: "root-not-directory",
        });
        return RootOutcome::Failed;
    }
    if root.source == SourceKind::Explicit {
        if let Some(runtime) = inspect_proton_directory_with_boundary(
            &scan_path,
            root.source,
            Some(&scan_path),
            issues,
        ) {
            merge_runtime(found, runtime, issues);
        }
        if let Some(runtime) =
            inspect_wine_directory_with_boundary(&scan_path, root.source, Some(&scan_path), issues)
        {
            merge_runtime(found, runtime, issues);
        }
    }
    let entries = match bounded_entries(&scan_path) {
        Ok(e) => e,
        Err(reason) => {
            issues.push(DiscoveryIssue {
                source: root.source,
                reason,
            });
            return if matches!(reason, "directory-entry-limit" | "discovery-path-limit") {
                RootOutcome::Truncated
            } else {
                RootOutcome::Failed
            };
        }
    };
    let steam_common =
        scan_path.ends_with("steamapps/common") || root.kind == RootKind::SteamLibraryProton;
    for path in entries {
        if steam_common && !potential_steam_proton_entry(&path) {
            continue;
        }
        // Steam's common directory also contains arbitrary game folders.
        // Inspect only one bounded level and accept a Proton candidate only
        // when its launcher and supported local metadata establish structure.
        let explicit_boundary =
            (root.source == SourceKind::Explicit).then_some(scan_path.as_path());
        if let Some(runtime) =
            inspect_proton_directory_with_boundary(&path, root.source, explicit_boundary, issues)
        {
            merge_runtime(found, runtime, issues);
            continue;
        }
        if root.kind == RootKind::RuntimeDirectory {
            if let Some(runtime) =
                inspect_wine_directory_with_boundary(&path, root.source, explicit_boundary, issues)
            {
                merge_runtime(found, runtime, issues);
            }
        }
    }
    if issues.len() == issue_start {
        RootOutcome::Inspected
    } else {
        RootOutcome::PartiallyInspected
    }
}

fn potential_steam_proton_entry(path: &Path) -> bool {
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    label.starts_with("proton")
        || label.starts_with("ge-proton")
        || label.starts_with("dwproton")
        || ["proton", "compatibilitytool.vdf", "toolmanifest.vdf"]
            .iter()
            .any(|name| fs::symlink_metadata(path.join(name)).is_ok())
}

fn bounded_entries(path: &Path) -> Result<Vec<PathBuf>, &'static str> {
    let before = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            "permission-denied"
        } else {
            "directory-read-failed"
        }
    })?;
    if !before.is_dir() {
        return Err("root-not-directory");
    }
    let mut out = Vec::new();
    let read = fs::read_dir(path).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            "permission-denied"
        } else {
            "directory-read-failed"
        }
    })?;
    for item in read {
        let entry = item.map_err(|_| "directory-read-failed")?;
        if entry.file_name().as_encoded_bytes().len() > 255 {
            continue;
        }
        if entry.path().as_os_str().len() > MAX_PATH_BYTES {
            return Err("discovery-path-limit");
        }
        out.push(entry.path());
        if out.len() > MAX_ENTRIES_PER_DIRECTORY {
            return Err("directory-entry-limit");
        }
    }
    let after = fs::symlink_metadata(path).map_err(|_| "runtime-changed-during-scan")?;
    if !after.is_dir() || !same_file(&before, &after) {
        return Err("runtime-changed-during-scan");
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
fn inspect_proton_directory(
    path: &Path,
    source: SourceKind,
    issues: &mut Vec<DiscoveryIssue>,
) -> Option<DiscoveredRuntime> {
    let boundary = if source == SourceKind::Explicit {
        let Some(parent) = path
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
        else {
            issues.push(DiscoveryIssue {
                source,
                reason: "unsafe-symlink-root",
            });
            return None;
        };
        Some(parent)
    } else {
        None
    };
    inspect_proton_directory_with_boundary(path, source, boundary.as_deref(), issues)
}

fn inspect_proton_directory_with_boundary(
    path: &Path,
    source: SourceKind,
    explicit_boundary: Option<&Path>,
    issues: &mut Vec<DiscoveryIssue>,
) -> Option<DiscoveredRuntime> {
    let resolved = match resolve_alias(path, explicit_boundary) {
        Ok(resolved) => resolved,
        Err(error) => {
            issues.push(DiscoveryIssue {
                source,
                reason: if error.kind() == io::ErrorKind::NotFound {
                    "runtime-changed-during-scan"
                } else {
                    "proton-path-unsafe-or-unavailable"
                },
            });
            return None;
        }
    };
    let runtime_metadata = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "runtime-changed-during-scan",
            });
            return None;
        }
    };
    if !runtime_metadata.is_dir() {
        return None;
    }
    let proton = resolved.join("proton");
    let manifest_path = resolved.join("compatibilitytool.vdf");
    let tool_path = resolved.join("toolmanifest.vdf");
    let version_path = resolved.join("version");
    let manifest_read = read_metadata_observed(&manifest_path);
    let tool_read = read_metadata_observed(&tool_path);
    let version_read = read_metadata_observed(&version_path);
    let manifest_raw = manifest_read.as_ref().map(|(text, _)| text.clone());
    let tool_raw = tool_read.as_ref().map(|(text, _)| text.clone());
    let version_file = version_read.as_ref().map(|(text, _)| text.clone());
    let mut observed_files = [
        manifest_read.as_ref(),
        tool_read.as_ref(),
        version_read.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|(_, observation)| observation.clone())
    .collect::<Vec<_>>();
    let has_metadata = [&manifest_path, &tool_path, &version_path]
        .iter()
        .any(|file| fs::symlink_metadata(file).is_ok());
    if !has_metadata {
        return None;
    }
    let manifest = manifest_raw
        .as_deref()
        .filter(|text| validate_vdf(text) && supported_proton_manifest(text, true))
        .map(str::to_owned);
    let tool = tool_raw
        .as_deref()
        .filter(|text| validate_vdf(text) && supported_proton_manifest(text, false))
        .map(str::to_owned);
    let launcher_path = match resolve_alias(&proton, explicit_boundary) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-launcher-missing",
            });
            return None;
        }
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-launcher-unsafe",
            });
            return None;
        }
    };
    let launcher_before = match fs::symlink_metadata(&launcher_path) {
        Ok(metadata) if metadata.is_file() && regular_executable(&launcher_path) => metadata,
        Ok(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-launcher-not-executable",
            });
            return None;
        }
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-launcher-missing",
            });
            return None;
        }
    };
    for (file, raw, parsed) in [
        (&manifest_path, manifest_raw.as_deref(), manifest.as_deref()),
        (&tool_path, tool_raw.as_deref(), tool.as_deref()),
    ] {
        if fs::symlink_metadata(file).is_ok() && parsed.is_none() {
            issues.push(DiscoveryIssue {
                source,
                reason: if raw.is_some_and(validate_vdf) {
                    "proton-manifest-unsupported"
                } else {
                    "proton-manifest-invalid"
                },
            });
        }
    }
    if fs::symlink_metadata(&version_path).is_ok() && version_file.is_none() {
        issues.push(DiscoveryIssue {
            source,
            reason: "proton-version-metadata-invalid",
        });
    }
    if manifest.is_none() && tool.is_none() && version_file.is_none() {
        return None;
    }
    let launcher_after = fs::symlink_metadata(&launcher_path);
    if !launcher_after.as_ref().is_ok_and(|metadata| {
        same_file(&launcher_before, metadata)
            && metadata.is_file()
            && regular_executable(&launcher_path)
    }) {
        issues.push(DiscoveryIssue {
            source,
            reason: "runtime-changed-during-scan",
        });
        return None;
    }
    observed_files.push(FileObservation::from_metadata(
        &launcher_path,
        launcher_after.as_ref().ok()?,
    ));
    let label = safe_label(resolved.file_name()?.to_string_lossy().as_ref());
    let folded = label.to_ascii_lowercase();
    let mut metadata_claims = BTreeSet::new();
    for metadata in [
        manifest.as_deref(),
        tool.as_deref(),
        version_file.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        metadata_claims.extend(claimed_lineages(metadata, true));
    }
    let metadata_lineage_conflict = metadata_claims.len() > 1;
    let metadata_lineage = (metadata_claims.len() == 1)
        .then(|| metadata_claims.into_iter().next())
        .flatten();
    let label_claims = claimed_lineages(&folded, true);
    let label_lineage_conflict = label_claims.len() > 1;
    let label_lineage = (label_claims.len() == 1)
        .then(|| label_claims.into_iter().next())
        .flatten();
    if metadata_lineage_conflict {
        issues.push(DiscoveryIssue {
            source,
            reason: "proton-manifest-lineage-conflict",
        });
    }
    if label_lineage_conflict {
        issues.push(DiscoveryIssue {
            source,
            reason: "proton-lineage-label-conflict",
        });
    }
    let lineage = match (&metadata_lineage, &label_lineage) {
        _ if metadata_lineage_conflict || label_lineage_conflict => RuntimeLineage::Unknown,
        (Some(metadata), Some(label)) if metadata != label => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-lineage-metadata-conflict",
            });
            RuntimeLineage::Unknown
        }
        (Some(metadata), _) => metadata.clone(),
        (_, Some(label)) => label.clone(),
        _ => RuntimeLineage::Unknown,
    };
    let lineage_basis = match (&metadata_lineage, &label_lineage) {
        _ if metadata_lineage_conflict => "conflicting local metadata",
        _ if label_lineage_conflict => "conflicting directory label",
        (Some(metadata), Some(label)) if metadata != label => {
            "conflicting local metadata and directory label"
        }
        (Some(_), _) => "local metadata",
        (_, Some(_)) => "directory label hint",
        _ => "unknown",
    };
    let file_version = version_file.as_deref().and_then(parse_version_file);
    if fs::symlink_metadata(&version_path).is_ok()
        && (version_file.is_none() || file_version.is_none())
    {
        issues.push(DiscoveryIssue {
            source,
            reason: "proton-version-metadata-invalid",
        });
    }
    let manifest_version = extract_manifest_version(manifest.as_deref());
    let tool_version = extract_manifest_version(tool.as_deref());
    let version_claims = [
        file_version.as_ref(),
        manifest_version.as_ref(),
        tool_version.as_ref(),
    ];
    let mut known_versions = version_claims.into_iter().flatten();
    let first_version = known_versions.next();
    let has_version_conflict =
        first_version.is_some_and(|first| known_versions.any(|candidate| candidate != first));
    let version = if has_version_conflict {
        issues.push(DiscoveryIssue {
            source,
            reason: "proton-version-metadata-conflict",
        });
        None
    } else {
        first_version.cloned()
    };
    let manifest_architectures = extract_architectures(manifest.as_deref());
    let tool_architectures = extract_architectures(tool.as_deref());
    let architectures = match (&manifest_architectures, &tool_architectures) {
        (Some(manifest), Some(tool)) if manifest != tool => {
            issues.push(DiscoveryIssue {
                source,
                reason: "proton-architecture-metadata-conflict",
            });
            Vec::new()
        }
        (Some(manifest), _) => manifest.clone(),
        (_, Some(tool)) => tool.clone(),
        _ => Vec::new(),
    };
    let ownership = match source {
        SourceKind::Heroic | SourceKind::Lutris => other_manager_ownership(source),
        SourceKind::NativeSteam | SourceKind::FlatpakSteam => RuntimeOwnership::ExternalUserManaged,
        _ => RuntimeOwnership::Unknown,
    };
    let structure = if (manifest.is_some() || tool.is_some()) && version.is_some() {
        "recognized"
    } else {
        "partially recognized"
    };
    Some(make_runtime(
        &resolved,
        source,
        RuntimeFamily::Proton,
        lineage,
        lineage_basis,
        version,
        architectures,
        ownership,
        observed_files,
        structure,
        label,
    ))
}

#[cfg(test)]
fn inspect_wine_directory(
    path: &Path,
    source: SourceKind,
    issues: &mut Vec<DiscoveryIssue>,
) -> Option<DiscoveredRuntime> {
    let boundary = if source == SourceKind::Explicit {
        let Some(parent) = path
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
        else {
            issues.push(DiscoveryIssue {
                source,
                reason: "unsafe-symlink-root",
            });
            return None;
        };
        Some(parent)
    } else {
        None
    };
    inspect_wine_directory_with_boundary(path, source, boundary.as_deref(), issues)
}

fn inspect_wine_directory_with_boundary(
    path: &Path,
    source: SourceKind,
    explicit_boundary: Option<&Path>,
    issues: &mut Vec<DiscoveryIssue>,
) -> Option<DiscoveredRuntime> {
    let resolved = match resolve_alias(path, explicit_boundary) {
        Ok(p) => p,
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "unsafe-symlink",
            });
            return None;
        }
    };
    let runtime_metadata = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "runtime-changed-during-scan",
            });
            return None;
        }
    };
    if !runtime_metadata.is_dir() {
        return None;
    }
    let bin = resolved.join("bin");
    let wine_paths = [
        bin.join("wine"),
        bin.join("wine64"),
        resolved.join("wine"),
        resolved.join("wine64"),
    ];
    let executable = wine_paths
        .iter()
        .find_map(|path| executable_metadata(path).map(|metadata| (path.clone(), metadata)));
    let (executable_path, executable_before) = executable?;
    let label = safe_label(resolved.file_name()?.to_string_lossy().as_ref());
    let version_path = resolved.join("version");
    let version_read = read_metadata_observed(&version_path);
    let version_text = version_read.as_ref().map(|(text, _)| text.clone());
    let version = version_text.as_deref().and_then(parse_version_file);
    if fs::symlink_metadata(&version_path).is_ok() && (version_text.is_none() || version.is_none())
    {
        issues.push(DiscoveryIssue {
            source,
            reason: "wine-version-metadata-invalid",
        });
    }
    let lineage = if version_text.as_deref().is_some_and(is_staging_metadata) {
        RuntimeLineage::WineStaging
    } else {
        RuntimeLineage::Unknown
    };
    let Some(executable_after) = executable_metadata(&executable_path) else {
        issues.push(DiscoveryIssue {
            source,
            reason: "runtime-changed-during-scan",
        });
        return None;
    };
    if !same_file(&executable_before, &executable_after) {
        issues.push(DiscoveryIssue {
            source,
            reason: "runtime-changed-during-scan",
        });
        return None;
    }
    let mut observed_files = vec![FileObservation::from_metadata(
        &executable_path,
        &executable_after,
    )];
    if let Some((_, observation)) = version_read {
        observed_files.push(observation);
    }
    // Installation names are hints only; without validated staging metadata lineage stays unknown.
    Some(make_runtime(
        &resolved,
        source,
        RuntimeFamily::Wine,
        lineage,
        if version_text.as_deref().is_some_and(is_staging_metadata) {
            "local version metadata"
        } else {
            "unknown"
        },
        version,
        Vec::new(),
        if source == SourceKind::Lutris || source == SourceKind::Heroic {
            other_manager_ownership(source)
        } else {
            RuntimeOwnership::Unknown
        },
        observed_files,
        "partially recognized",
        label,
    ))
}

fn inspect_wine_executable(
    path: &Path,
    source: SourceKind,
    issues: &mut Vec<DiscoveryIssue>,
) -> Option<DiscoveredRuntime> {
    let md = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            issues.push(DiscoveryIssue {
                source,
                reason: "runtime-changed-during-scan",
            });
            return None;
        }
        Err(e) => {
            issues.push(issue(source, &e));
            return None;
        }
    };
    let resolved = match resolve_alias(path, None) {
        Ok(p) => p,
        Err(_) => {
            issues.push(DiscoveryIssue {
                source,
                reason: "unsafe-symlink",
            });
            return None;
        }
    };
    if !(md.is_file() || md.file_type().is_symlink()) {
        return None;
    }
    let target_before = executable_metadata(&resolved)?;
    let label = safe_label(path.file_name()?.to_string_lossy().as_ref());
    let ownership = if resolved.starts_with("/usr/bin") || resolved.starts_with("/usr/lib") {
        RuntimeOwnership::DistributionManaged
    } else if resolved.starts_with("/usr/local") {
        RuntimeOwnership::Unknown
    } else {
        RuntimeOwnership::ExternalUserManaged
    };
    let Some(target_after) = executable_metadata(&resolved) else {
        issues.push(DiscoveryIssue {
            source,
            reason: "runtime-changed-during-scan",
        });
        return None;
    };
    if !same_file(&target_before, &target_after) {
        issues.push(DiscoveryIssue {
            source,
            reason: "runtime-changed-during-scan",
        });
        return None;
    }
    Some(make_runtime(
        &resolved,
        source,
        RuntimeFamily::Wine,
        RuntimeLineage::Unknown,
        "unknown",
        None,
        Vec::new(),
        ownership,
        vec![FileObservation::from_metadata(&resolved, &target_after)],
        "partially recognized",
        label,
    ))
}

#[allow(clippy::too_many_arguments)]
fn make_runtime(
    path: &Path,
    source: SourceKind,
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    lineage_basis: &'static str,
    version: Option<RuntimeVersion>,
    architectures: Vec<ProcessArchitecture>,
    ownership: RuntimeOwnership,
    observed_files: Vec<FileObservation>,
    structure: &'static str,
    label: String,
) -> DiscoveredRuntime {
    let path_bytes = path.as_os_str().as_encoded_bytes();
    let hash_a = stable_hash(path_bytes, 0xcbf29ce484222325);
    let hash_b = stable_hash(path_bytes, 0x84222325cbf29ce4);
    let id = RuntimeId::new(format!("local-{hash_a:016x}{hash_b:016x}")).unwrap();
    let provider = ProviderId::new(PROVIDER_ID).unwrap();
    let candidate = RuntimeCandidate::new(id, provider, family, version.clone(), ownership.clone());
    DiscoveredRuntime {
        candidate,
        lineage,
        lineage_basis,
        version,
        architectures,
        sources: vec![SourceReference {
            source,
            label: source.label(),
        }],
        structure,
        display_label: label,
        path: path.to_path_buf(),
        ownership,
        observed_files,
    }
}

fn merge_runtime(
    found: &mut BTreeMap<PathBuf, DiscoveredRuntime>,
    mut runtime: DiscoveredRuntime,
    issues: &mut Vec<DiscoveryIssue>,
) {
    let key = runtime.path.clone();
    if let Some(prior) = found.get_mut(&key) {
        prior.sources.append(&mut runtime.sources);
        prior.sources.sort_by_key(|s| s.source);
        prior.sources.dedup_by_key(|s| s.source);
        prior.observed_files.append(&mut runtime.observed_files);
        prior
            .observed_files
            .sort_by(|left, right| left.path.cmp(&right.path));
        if prior.ownership != runtime.ownership {
            prior.ownership = RuntimeOwnership::Unknown;
            prior.candidate = RuntimeCandidate::new(
                prior.candidate.runtime_id().clone(),
                prior.candidate.provider_id().clone(),
                prior.candidate.family(),
                prior.version.clone(),
                RuntimeOwnership::Unknown,
            );
        }
    } else if found.len() >= MAX_RUNTIME_CANDIDATES {
        if !issues
            .iter()
            .any(|issue| issue.reason == "runtime-candidate-limit")
        {
            issues.push(DiscoveryIssue {
                source: runtime
                    .sources
                    .first()
                    .map_or(SourceKind::Explicit, |reference| reference.source),
                reason: "runtime-candidate-limit",
            });
        }
    } else {
        found.insert(key, runtime);
    }
}

fn resolve_alias(path: &Path, boundary: Option<&Path>) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_SYMLINK_DEPTH {
        if !seen.insert(current.clone()) {
            return Err(io::Error::other("symlink cycle"));
        }
        let md = fs::symlink_metadata(&current)?;
        if !md.file_type().is_symlink() {
            let resolved = fs::canonicalize(&current)?;
            if let Some(base) = boundary {
                if !resolved.starts_with(base) {
                    return Err(io::Error::other("outside root"));
                }
            }
            return Ok(resolved);
        }
        let target = fs::read_link(&current)?;
        current = if target.is_absolute() {
            target
        } else {
            current.parent().unwrap_or(Path::new("/")).join(target)
        };
    }
    Err(io::Error::other("symlink depth"))
}

#[cfg(unix)]
fn executable_metadata(path: &Path) -> Option<fs::Metadata> {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn executable_metadata(path: &Path) -> Option<fs::Metadata> {
    fs::symlink_metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
}
fn regular_executable(path: &Path) -> bool {
    executable_metadata(path).is_some()
}

fn is_staging_metadata(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("wine-staging")
        || (lower.contains("wine") && lower.split_whitespace().any(|word| word == "staging"))
}

fn read_metadata(path: &Path) -> Option<String> {
    read_metadata_observed(path).map(|(text, _)| text)
}

fn read_metadata_observed(path: &Path) -> Option<(String, FileObservation)> {
    let before = fs::symlink_metadata(path).ok()?;
    if !before.is_file() || before.len() > MAX_METADATA_BYTES {
        return None;
    }
    // Metadata paths are untrusted and may change after lstat. O_NOFOLLOW
    // rejects a last-component symlink and O_NONBLOCK prevents a raced FIFO
    // from hanging discovery. The opened descriptor is still type-checked.
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() || !same_file(&before, &opened) {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let after = file.metadata().ok()?;
    let path_after = fs::symlink_metadata(path).ok()?;
    if bytes.len() as u64 > MAX_METADATA_BYTES
        || !same_file(&opened, &after)
        || !same_file(&after, &path_after)
    {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some((text, FileObservation::from_metadata(path, &after)))
}
#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.is_file() == right.is_file()
        && left.is_dir() == right.is_dir()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

fn same_file_observation(observation: &FileObservation, metadata: &fs::Metadata) -> bool {
    observation == &FileObservation::from_metadata(&observation.path, metadata)
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
        && left.is_file() == right.is_file()
        && left.is_dir() == right.is_dir()
        && left.modified().ok() == right.modified().ok()
}
fn read_vdf(path: &Path) -> Option<String> {
    let text = read_metadata(path)?;
    validate_vdf(&text).then_some(text)
}

fn supported_proton_manifest(text: &str, compatibility_tool: bool) -> bool {
    let Some(strings) = vdf_strings(text) else {
        return false;
    };
    let Some(root) = strings.first() else {
        return false;
    };
    if compatibility_tool {
        root.eq_ignore_ascii_case("compatibilitytool")
            && strings
                .iter()
                .any(|value| value.eq_ignore_ascii_case("compat_tools"))
    } else {
        matches!(
            root.to_ascii_lowercase().as_str(),
            "manifest" | "toolmanifest" | "tool_manifest"
        ) && strings.windows(2).any(|pair| {
            matches!(
                pair[0].to_ascii_lowercase().as_str(),
                "version" | "commandline" | "compatmanager_layer"
            )
        })
    }
}

fn validate_vdf(text: &str) -> bool {
    if text.len() > MAX_METADATA_BYTES as usize {
        return false;
    }
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut stack = vec![true]; // each scope alternates key, value
    let mut tokens = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'{' => {
                if stack.len() >= 16 || *stack.last().unwrap_or(&true) {
                    return false;
                }
                *stack.last_mut().unwrap() = true;
                stack.push(true);
                index += 1;
                tokens += 1;
            }
            b'}' => {
                if stack.len() <= 1 || !*stack.last().unwrap_or(&false) {
                    return false;
                }
                stack.pop();
                index += 1;
                tokens += 1;
            }
            b'"' => {
                index += 1;
                let mut closed = false;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\'
                            if index + 1 < bytes.len()
                                && matches!(bytes[index + 1], b'\\' | b'"') =>
                        {
                            index += 2
                        }
                        b'"' => {
                            index += 1;
                            closed = true;
                            break;
                        }
                        0..=31 => return false,
                        _ => index += 1,
                    }
                }
                if !closed {
                    return false;
                }
                let expects_key = stack.last_mut().unwrap();
                *expects_key = !*expects_key;
                tokens += 1;
            }
            _ => return false,
        }
        if tokens > 8192 {
            return false;
        }
    }
    stack.len() == 1 && stack[0]
}
fn steam_library_roots(steam_root: &Path, source: SourceKind) -> Result<Vec<Root>, &'static str> {
    let file = steam_root.join("steamapps/libraryfolders.vdf");
    match fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err("steam-library-metadata-invalid"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return Err("permission-denied");
        }
        Err(_) => return Err("steam-library-metadata-unavailable"),
    }
    let Some(text) = read_vdf(&file) else {
        return Err("steam-library-metadata-invalid");
    };
    if !vdf_strings(&text).is_some_and(|strings| {
        strings
            .first()
            .is_some_and(|root| root.eq_ignore_ascii_case("libraryfolders"))
    }) {
        return Err("steam-library-metadata-invalid");
    }
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut stack: Vec<(Option<String>, Option<String>, bool)> = vec![(None, None, true)];
    let mut paths = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'{' | b'}' => {
                let open = bytes[index] == b'{';
                index += 1;
                if open {
                    let Some(parent) = stack.last_mut() else {
                        return Err("steam-library-metadata-invalid");
                    };
                    if parent.2 {
                        return Err("steam-library-metadata-invalid");
                    }
                    let Some(object_name) = parent.1.take() else {
                        return Err("steam-library-metadata-invalid");
                    };
                    parent.2 = true;
                    stack.push((Some(object_name), None, true));
                } else if stack.len() > 1 && stack.last().is_some_and(|scope| scope.1.is_none()) {
                    stack.pop();
                } else {
                    return Err("steam-library-metadata-invalid");
                }
            }
            b'"' => {
                index += 1;
                let mut value = Vec::new();
                let mut closed = false;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\'
                            if index + 1 < bytes.len()
                                && matches!(bytes[index + 1], b'\\' | b'"') =>
                        {
                            value.push(bytes[index + 1]);
                            index += 2;
                        }
                        b'"' => {
                            index += 1;
                            closed = true;
                            break;
                        }
                        byte => {
                            value.push(byte);
                            index += 1;
                        }
                    }
                }
                if !closed {
                    return Err("steam-library-metadata-invalid");
                }
                let Ok(value) = String::from_utf8(value) else {
                    return Err("steam-library-metadata-invalid");
                };
                let scope_depth = stack.len();
                let is_library_record = scope_depth == 3
                    && stack[2].0.as_deref().is_some_and(|name| {
                        !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit())
                    });
                let Some(scope) = stack.last_mut() else {
                    return Err("steam-library-metadata-invalid");
                };
                if scope.2 {
                    scope.1 = Some(value);
                    scope.2 = false;
                } else {
                    let key = scope.1.take().ok_or("steam-library-metadata-invalid")?;
                    if key.eq_ignore_ascii_case("path") && !is_library_record {
                        return Err("steam-library-metadata-invalid");
                    }
                    if is_library_record && key.eq_ignore_ascii_case("path") {
                        if value.chars().any(char::is_control) {
                            return Err("steam-library-path-invalid");
                        }
                        let path = PathBuf::from(value);
                        if path.is_absolute()
                            && path.as_os_str().len() <= MAX_PATH_BYTES
                            && !path
                                .components()
                                .any(|c| c == std::path::Component::ParentDir)
                        {
                            if paths.len() >= MAX_ROOTS {
                                return Err("steam-library-root-limit");
                            }
                            paths.push(Root {
                                path: path.join("steamapps/common"),
                                source,
                                kind: RootKind::SteamLibraryProton,
                            });
                        } else {
                            return Err("steam-library-path-invalid");
                        }
                    }
                    scope.2 = true;
                }
            }
            _ => return Err("steam-library-metadata-invalid"),
        }
    }
    paths.sort_by(|a, b| a.path.cmp(&b.path));
    paths.dedup_by(|a, b| a.path == b.path);
    Ok(paths)
}
fn sanitize_version(s: &str) -> Option<RuntimeVersion> {
    let s = s.trim();
    if s.is_empty() || s.len() > RuntimeVersion::MAX_BYTES || s.chars().any(char::is_control) {
        None
    } else {
        RuntimeVersion::new(s.to_string()).ok()
    }
}
fn parse_version_file(text: &str) -> Option<RuntimeVersion> {
    if text.lines().count() > 8 {
        return None;
    }
    sanitize_version(text.trim())
}
fn extract_manifest_version(text: Option<&str>) -> Option<RuntimeVersion> {
    let text = text?;
    for (key, value) in vdf_pairs(text)? {
        if matches!(
            key.to_ascii_lowercase().as_str(),
            "version" | "version_name" | "buildid" | "build_id"
        ) {
            if let Some(version) = sanitize_version(value.trim()) {
                return Some(version);
            }
        }
    }
    None
}

fn extract_architectures(text: Option<&str>) -> Option<Vec<ProcessArchitecture>> {
    let fields = vdf_pairs(text?)?;
    let mut architectures = BTreeSet::new();
    for (key, value) in fields {
        if matches!(
            key.to_ascii_lowercase().as_str(),
            "arch"
                | "archs"
                | "architecture"
                | "architectures"
                | "supported_architecture"
                | "supported_architectures"
        ) {
            for token in value.split(|character: char| {
                character.is_ascii_whitespace() || matches!(character, ',' | ';')
            }) {
                if let Some(architecture) = parse_architecture(token) {
                    architectures.insert(architecture);
                }
            }
        }
    }
    (!architectures.is_empty()).then(|| architectures.into_iter().collect())
}

fn parse_architecture(value: &str) -> Option<ProcessArchitecture> {
    match value.to_ascii_lowercase().as_str() {
        "x86" | "i386" | "i486" | "i586" | "i686" => Some(ProcessArchitecture::X86),
        "x86_64" | "amd64" | "x64" => Some(ProcessArchitecture::X86_64),
        "aarch64" | "arm64" => Some(ProcessArchitecture::Aarch64),
        "arm" | "armv7" => Some(ProcessArchitecture::Arm),
        "riscv64" => Some(ProcessArchitecture::Riscv64),
        _ => None,
    }
}

fn vdf_pairs(text: &str) -> Option<Vec<(String, String)>> {
    if !validate_vdf(text) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut pending_keys: Vec<Option<String>> = vec![None];
    let mut pairs = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'{' => {
                pending_keys.last_mut()?.take()?;
                pending_keys.push(None);
                index += 1;
            }
            b'}' => {
                if pending_keys.len() <= 1 || pending_keys.last()?.is_some() {
                    return None;
                }
                pending_keys.pop();
                index += 1;
            }
            b'"' => {
                index += 1;
                let mut value = Vec::new();
                let mut closed = false;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\'
                            if index + 1 < bytes.len()
                                && matches!(bytes[index + 1], b'\\' | b'"') =>
                        {
                            value.push(bytes[index + 1]);
                            index += 2;
                        }
                        b'"' => {
                            index += 1;
                            closed = true;
                            break;
                        }
                        byte => {
                            value.push(byte);
                            index += 1;
                        }
                    }
                }
                if !closed {
                    return None;
                }
                let value = String::from_utf8(value).ok()?;
                if let Some(key) = pending_keys.last_mut()?.take() {
                    pairs.push((key, value));
                } else {
                    *pending_keys.last_mut()? = Some(value);
                }
                if pairs.len() > 8192 {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (pending_keys.len() == 1 && pending_keys[0].is_none()).then_some(pairs)
}

fn vdf_strings(text: &str) -> Option<Vec<String>> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut values = Vec::new();
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        index += 1;
        let mut value = Vec::new();
        let mut closed = false;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if index + 1 < bytes.len() && matches!(bytes[index + 1], b'\\' | b'"') => {
                    value.push(bytes[index + 1]);
                    index += 2;
                }
                b'"' => {
                    index += 1;
                    closed = true;
                    break;
                }
                byte => {
                    value.push(byte);
                    index += 1;
                }
            }
        }
        if !closed {
            return None;
        }
        values.push(String::from_utf8(value).ok()?);
        if values.len() > 8192 {
            return None;
        }
    }
    Some(values)
}
fn safe_label(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        .take(80)
        .collect()
}
fn claimed_lineages(text: &str, allow_valve_label: bool) -> BTreeSet<RuntimeLineage> {
    let lower = text.to_ascii_lowercase();
    let mut claims = BTreeSet::new();
    if lower.contains("dwproton") {
        claims.insert(RuntimeLineage::DwProton);
    }
    if lower.contains("ge-proton") {
        claims.insert(RuntimeLineage::GeProton);
    }
    if lower.contains("valve")
        || (allow_valve_label
            && (lower.starts_with("proton -")
                || (lower.starts_with("proton ") && lower.chars().any(|c| c.is_ascii_digit()))))
    {
        claims.insert(RuntimeLineage::ValveProton);
    }
    claims
}
fn stable_hash(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
fn other_manager_ownership(source: SourceKind) -> RuntimeOwnership {
    let id = match source {
        SourceKind::Heroic => "heroic",
        SourceKind::Lutris => "lutris",
        _ => "external-manager",
    };
    RuntimeOwnership::OtherLauncherManaged(ProviderId::new(id).unwrap())
}
fn issue(source: SourceKind, error: &io::Error) -> DiscoveryIssue {
    DiscoveryIssue {
        source,
        reason: if error.kind() == io::ErrorKind::PermissionDenied {
            "permission-denied"
        } else {
            "filesystem-entry-unavailable"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn declaration_only_advertises_real_operations() {
        let p = LinuxRuntimeProvider::new(DiscoveryOptions::default()).unwrap();
        assert!(
            p.declaration()
                .capability_status(ProviderCapabilityKind::DiscoverBuilds)
                .unwrap()
                .is_implemented()
        );
        assert_eq!(
            p.declaration()
                .capability_status(ProviderCapabilityKind::ManageInstallations),
            Some(CapabilityStatus::Unsupported)
        );
    }
    #[test]
    fn explicit_roots_must_be_absolute_and_bounded() {
        assert_eq!(
            LinuxRuntimeProvider::new(DiscoveryOptions {
                additional_roots: vec!["relative".into()],
                include_path: false,
                include_conventional: false
            })
            .unwrap_err(),
            DiscoveryConfigError::InvalidCustomRoot
        );
        assert_eq!(
            LinuxRuntimeProvider::new(DiscoveryOptions {
                additional_roots: (0..9).map(|i| PathBuf::from(format!("/tmp/{i}"))).collect(),
                include_path: false,
                include_conventional: false
            })
            .unwrap_err(),
            DiscoveryConfigError::TooManyCustomRoots
        );
        assert_eq!(
            LinuxRuntimeProvider::new(DiscoveryOptions {
                additional_roots: vec![PathBuf::from("/tmp/../home/runtimes")],
                include_path: false,
                include_conventional: false
            })
            .unwrap_err(),
            DiscoveryConfigError::InvalidCustomRoot
        );
    }
    #[test]
    fn proton_identity_is_structural_and_claims_are_unverified() {
        let dir = fixture("ge-proton").join("GE-Proton10-26");
        let _ = fs::remove_dir_all(dir.parent().unwrap());
        fs::create_dir_all(&dir).unwrap();
        write_proton(&dir.join("proton"));
        fs::write(dir.join("version"), "GE-Proton10-26\n").unwrap();
        fs::write(dir.join("compatibilitytool.vdf"), "\"compatibilitytool\" { \"compat_tools\" { \"GE-Proton10-26\" { \"version\" \"GE-Proton10-26\" \"architecture\" \"x86_64\" } } }").unwrap();
        let mut issues = Vec::new();
        let found = inspect_proton_directory(&dir, SourceKind::Explicit, &mut issues).unwrap();
        assert_eq!(found.lineage, RuntimeLineage::GeProton);
        assert_eq!(found.version.as_ref().unwrap().as_str(), "GE-Proton10-26");
        assert_eq!(found.candidate.ownership(), &RuntimeOwnership::Unknown);
        assert_eq!(found.candidate.family(), RuntimeFamily::Proton);
        assert_eq!(found.architectures, vec![ProcessArchitecture::X86_64]);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }
    fn fixture(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("endfield-{name}-{}", std::process::id()))
    }
    fn write_proton(path: &Path) {
        fs::write(path, b"not executed").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn proton_requires_supported_metadata_and_rejects_malformed_vdf() {
        let dir = fixture("bad-vdf");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write_proton(&dir.join("proton"));
        fs::write(dir.join("compatibilitytool.vdf"), "\"broken\" { ").unwrap();
        let mut issues = Vec::new();
        assert!(inspect_proton_directory(&dir, SourceKind::Explicit, &mut issues).is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-manifest-invalid")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn well_formed_but_unrecognized_proton_manifest_is_not_identity_evidence() {
        let dir = fixture("unsupported-proton-manifest");
        fs::create_dir_all(&dir).unwrap();
        write_proton(&dir.join("proton"));
        fs::write(
            dir.join("compatibilitytool.vdf"),
            "\"other-root\" { \"compat_tools\" { \"custom\" { } } }",
        )
        .unwrap();
        let mut issues = Vec::new();
        assert!(inspect_proton_directory(&dir, SourceKind::NativeSteam, &mut issues).is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-manifest-unsupported")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn proton_classifies_known_directory_labels_as_claims_for_all_sources() {
        let base = fixture("proton-claimed-lineages");
        for (name, expected) in [
            ("GE-Proton10-26", RuntimeLineage::GeProton),
            ("DWProton-1.0", RuntimeLineage::DwProton),
            ("Proton - Experimental", RuntimeLineage::ValveProton),
        ] {
            let dir = base.join(name);
            fs::create_dir_all(&dir).unwrap();
            write_proton(&dir.join("proton"));
            fs::write(dir.join("version"), "build-unknown\n").unwrap();
            let found =
                inspect_proton_directory(&dir, SourceKind::Heroic, &mut Vec::new()).unwrap();
            assert_eq!(found.lineage, expected);
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn proton_missing_launcher_and_version_conflict_remain_incomplete() {
        let base = fixture("proton-incomplete");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("version"), "release-a\n").unwrap();
        let mut issues = Vec::new();
        assert!(inspect_proton_directory(&base, SourceKind::NativeSteam, &mut issues).is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-launcher-missing")
        );

        write_proton(&base.join("proton"));
        fs::write(
            base.join("compatibilitytool.vdf"),
            "\"compatibilitytool\" { \"compat_tools\" { \"custom\" { \"version\" \"release-b\" } } }",
        )
        .unwrap();
        let found = inspect_proton_directory(&base, SourceKind::NativeSteam, &mut issues).unwrap();
        assert!(found.version.is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-version-metadata-conflict")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn wine_executable_and_wine64_only_candidates_are_found_without_execution() {
        let base = fixture("wine-executable");
        fs::create_dir_all(base.join("bin")).unwrap();
        fs::write(base.join("wine"), b"fixture executable").unwrap();
        fs::set_permissions(base.join("wine"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(base.join("bin/wine64"), b"fixture executable").unwrap();
        fs::set_permissions(base.join("bin/wine64"), fs::Permissions::from_mode(0o700)).unwrap();

        let mut issues = Vec::new();
        let executable =
            inspect_wine_executable(&base.join("wine"), SourceKind::PathWine, &mut issues).unwrap();
        assert_eq!(executable.candidate.family(), RuntimeFamily::Wine);
        assert!(executable.version.is_none());
        assert_eq!(executable.lineage, RuntimeLineage::Unknown);

        let directory = inspect_wine_directory(&base, SourceKind::Lutris, &mut issues).unwrap();
        assert_eq!(directory.candidate.family(), RuntimeFamily::Wine);
        assert_eq!(directory.lineage, RuntimeLineage::Unknown);
        assert_eq!(
            directory.candidate.ownership(),
            &other_manager_ownership(SourceKind::Lutris)
        );
        assert_eq!(fs::read(base.join("wine")).unwrap(), b"fixture executable");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn scan_never_invokes_a_discovered_executable() {
        let base = fixture("no-runtime-execution");
        let root = base.join("runtimes");
        let marker = base.join("executed");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&root).unwrap();
        let script = format!("#!/bin/sh\nprintf invoked > '{}'\n", marker.display());
        fs::write(root.join("wine"), script).unwrap();
        fs::set_permissions(root.join("wine"), fs::Permissions::from_mode(0o700)).unwrap();

        let provider = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![root],
            include_path: false,
            include_conventional: false,
        })
        .unwrap();
        let _ = provider.scan();
        assert!(!marker.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn broken_wine_alias_and_invalid_version_metadata_are_reported_safely() {
        let base = fixture("wine-invalid-metadata");
        fs::create_dir_all(base.join("bin")).unwrap();
        fs::write(base.join("bin/wine"), b"fixture executable").unwrap();
        fs::set_permissions(base.join("bin/wine"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            base.join("version"),
            vec![b'x'; RuntimeVersion::MAX_BYTES + 2],
        )
        .unwrap();
        let mut issues = Vec::new();
        let found = inspect_wine_directory(&base, SourceKind::Heroic, &mut issues).unwrap();
        assert!(found.version.is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "wine-version-metadata-invalid")
        );
        let broken = base.join("broken-wine");
        std::os::unix::fs::symlink(base.join("missing-target"), &broken).unwrap();
        assert!(inspect_wine_executable(&broken, SourceKind::PathWine, &mut issues).is_none());
        assert!(issues.iter().any(|issue| issue.reason == "unsafe-symlink"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn heroic_lutris_native_and_flatpak_sources_keep_manager_ownership() {
        let base = fixture("manager-source-roots");
        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        for (source, name) in [
            (SourceKind::Heroic, "heroic"),
            (SourceKind::Lutris, "lutris"),
            (SourceKind::FlatpakSteam, "flatpak-steam"),
        ] {
            let root = base.join(name);
            let build_name = if source == SourceKind::Lutris {
                "wine-ge-custom"
            } else {
                "DWProton-1.0"
            };
            let build = root.join(build_name);
            fs::create_dir_all(&build).unwrap();
            if source == SourceKind::Lutris {
                fs::create_dir_all(build.join("bin")).unwrap();
                fs::write(build.join("bin/wine"), b"fixture executable").unwrap();
                fs::set_permissions(build.join("bin/wine"), fs::Permissions::from_mode(0o700))
                    .unwrap();
            } else {
                write_proton(&build.join("proton"));
                fs::write(build.join("version"), "build-1\n").unwrap();
            }
            inspect_root(
                &Root {
                    path: root,
                    source,
                    kind: RootKind::RuntimeDirectory,
                },
                &mut found,
                &mut issues,
            );
        }
        assert_eq!(found.len(), 3);
        assert!(
            found
                .values()
                .all(|runtime| match runtime.sources[0].source {
                    SourceKind::Heroic =>
                        runtime.candidate.ownership()
                            == &other_manager_ownership(SourceKind::Heroic),
                    SourceKind::Lutris =>
                        runtime.candidate.ownership()
                            == &other_manager_ownership(SourceKind::Lutris),
                    SourceKind::FlatpakSteam =>
                        runtime.candidate.ownership() == &RuntimeOwnership::ExternalUserManaged,
                    _ => false,
                })
        );
        assert!(issues.is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn aliases_merge_sources_and_ownership_conflicts_conservatively() {
        let base = fixture("runtime-alias-merge");
        let build = base.join("GE-Proton10-26");
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(build.join("version"), "GE-Proton10-26\n").unwrap();
        let steam =
            inspect_proton_directory(&build, SourceKind::NativeSteam, &mut Vec::new()).unwrap();
        let heroic = inspect_proton_directory(&build, SourceKind::Heroic, &mut Vec::new()).unwrap();
        let mut merged = BTreeMap::new();
        let mut issues = Vec::new();
        merge_runtime(&mut merged, steam, &mut issues);
        merge_runtime(&mut merged, heroic, &mut issues);
        let runtime = merged.values().next().unwrap();
        assert_eq!(runtime.sources.len(), 2);
        assert_eq!(runtime.candidate.ownership(), &RuntimeOwnership::Unknown);
        assert_eq!(runtime.sources[0].source, SourceKind::NativeSteam);
        assert_eq!(runtime.sources[1].source, SourceKind::Heroic);
        assert!(issues.is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn conflicting_lineage_and_version_metadata_are_not_silently_preferred() {
        let dir = fixture("conflicting-proton").join("GE-Proton10-26");
        let _ = fs::remove_dir_all(dir.parent().unwrap());
        fs::create_dir_all(&dir).unwrap();
        write_proton(&dir.join("proton"));
        fs::write(dir.join("version"), "GE-Proton10-26\n").unwrap();
        fs::write(
            dir.join("compatibilitytool.vdf"),
            "\"compatibilitytool\" { \"compat_tools\" { \"DWProton\" { \"version\" \"1.0\" } } }",
        )
        .unwrap();
        let mut issues = Vec::new();
        let found = inspect_proton_directory(&dir, SourceKind::Explicit, &mut issues).unwrap();
        assert_eq!(found.lineage, RuntimeLineage::Unknown);
        assert!(found.version.is_none());
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-manifest-lineage-conflict")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "proton-version-metadata-conflict")
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn steam_libraryfolders_vdf_is_bounded_and_only_absolute_safe_paths_are_accepted() {
        let root = fixture("steam-library");
        let steam = root.join("Steam");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        let library = root.join("library-one");
        let vdf = format!(
            "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
            library.display()
        );
        fs::write(steam.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        let roots = steam_library_roots(&steam, SourceKind::NativeSteam).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, library.join("steamapps/common"));
        fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" { \"0\" { \"path\" \"relative\" } }",
        )
        .unwrap();
        assert_eq!(
            steam_library_roots(&steam, SourceKind::NativeSteam),
            Err("steam-library-path-invalid")
        );
        fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" { \"0\" { \"apps\" { \"path\" \"/unexpected\" } } }",
        )
        .unwrap();
        assert_eq!(
            steam_library_roots(&steam, SourceKind::NativeSteam),
            Err("steam-library-metadata-invalid")
        );
        fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" {",
        )
        .unwrap();
        assert_eq!(
            steam_library_roots(&steam, SourceKind::NativeSteam),
            Err("steam-library-metadata-invalid")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_root_does_not_follow_runtime_symlinks_outside_its_boundary() {
        let base = fixture("symlink-boundary");
        let root = base.join("root");
        let outside = base.join("outside/GE-Proton10-26");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        write_proton(&outside.join("proton"));
        fs::write(outside.join("version"), "GE-Proton10-26\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("runtime-link")).unwrap();
        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        inspect_root(
            &Root {
                path: root.clone(),
                source: SourceKind::Explicit,
                kind: RootKind::RuntimeDirectory,
            },
            &mut found,
            &mut issues,
        );
        assert!(found.is_empty());
        assert!(issues.iter().any(|issue| issue.reason == "unsafe-symlink"));

        let root_alias = base.join("root-alias");
        std::os::unix::fs::symlink(&root, &root_alias).unwrap();
        inspect_root(
            &Root {
                path: root_alias,
                source: SourceKind::Explicit,
                kind: RootKind::RuntimeDirectory,
            },
            &mut found,
            &mut issues,
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "unsafe-symlink-root")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn steam_common_ignores_game_version_files_and_finds_custom_named_proton() {
        let common = fixture("steam-common-structure").join("steamapps/common");
        let _ = fs::remove_dir_all(common.parent().unwrap().parent().unwrap());
        let game = common.join("Unrelated Game");
        let custom = common.join("Custom Runner");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("version"), "game-build-1\n").unwrap();
        fs::create_dir_all(&custom).unwrap();
        write_proton(&custom.join("proton"));
        fs::write(custom.join("version"), "custom-build-1\n").unwrap();
        fs::write(
            custom.join("compatibilitytool.vdf"),
            "\"compatibilitytool\" { \"compat_tools\" { \"custom\" { \"version\" \"custom-build-1\" } } }",
        )
        .unwrap();

        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        inspect_root(
            &Root {
                path: common,
                source: SourceKind::NativeSteam,
                kind: RootKind::SteamLibraryProton,
            },
            &mut found,
            &mut issues,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found.values().next().unwrap().display_label,
            "Custom Runner"
        );
        assert!(issues.is_empty());
        let _ = fs::remove_dir_all(fixture("steam-common-structure"));
    }

    #[test]
    fn supported_manager_root_alias_resolves_to_the_same_installation() {
        let base = fixture("manager-root-alias");
        let target = base.join("steam-target/compatibilitytools.d");
        let alias = base.join("steam-alias");
        let build = target.join("GE-Proton10-26");
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(build.join("version"), "GE-Proton10-26\n").unwrap();
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        inspect_root(
            &Root {
                path: alias,
                source: SourceKind::NativeSteam,
                kind: RootKind::ProtonDirectory,
            },
            &mut found,
            &mut issues,
        );
        assert_eq!(found.len(), 1);
        assert!(issues.is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn staging_requires_version_metadata_evidence() {
        let dir = fixture("wine-staging").join("wine");
        let _ = fs::remove_dir_all(dir.parent().unwrap());
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin/wine"), b"not executed").unwrap();
        fs::set_permissions(dir.join("bin/wine"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.join("version"), "Wine Staging 10.2\n").unwrap();
        let mut issues = Vec::new();
        let found = inspect_wine_directory(&dir, SourceKind::Explicit, &mut issues).unwrap();
        assert_eq!(found.lineage, RuntimeLineage::WineStaging);
        assert_eq!(
            found.version.as_ref().unwrap().as_str(),
            "Wine Staging 10.2"
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }
    #[test]
    fn provider_emits_valid_p021_partial_or_complete_report_without_mutating_fixture() {
        let dir = fixture("provider");
        let _ = fs::remove_dir_all(&dir);
        let build = dir.join("Proton-custom");
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(build.join("version"), "custom-build\n").unwrap();
        fs::write(
            build.join("compatibilitytool.vdf"),
            "\"compatibilitytool\" { \"compat_tools\" { \"custom\" { \"architecture\" \"x86_64\" } } }",
        )
        .unwrap();
        let options = DiscoveryOptions {
            additional_roots: vec![dir.clone()],
            include_path: false,
            include_conventional: false,
        };
        let provider = LinuxRuntimeProvider::new(options).unwrap();
        let report = provider.scan();
        assert_eq!(report.contract.candidates().len(), 1);
        assert!(report.roots.iter().any(|root| {
            root.source == SourceKind::Explicit
                && root.ownership_expectation == RuntimeOwnership::Unknown
                && root.attempted
                && root.completed
        }));
        assert_eq!(
            report.contract.candidates()[0].family(),
            RuntimeFamily::Proton
        );
        let before = fs::read_dir(&build).unwrap().count();
        let identity = provider
            .inspect_identity(&report.contract.candidates()[0])
            .unwrap();
        assert_eq!(identity.version().as_str(), "custom-build");
        assert_eq!(identity.ownership(), &RuntimeOwnership::Unknown);
        assert!(
            identity
                .requirements()
                .architectures()
                .contains(&ProcessArchitecture::X86_64)
        );
        assert_eq!(before, fs::read_dir(&build).unwrap().count());
        assert!(!format!("{report:?}").contains(&dir.display().to_string()));
        let source = identity.source().unwrap();
        assert_eq!(source.category(), SourceCategory::ExternalInstallation);
        assert_eq!(
            source.verification().state(),
            SourceVerificationState::CandidateIdentified
        );
        assert!(!source.verification().authenticity().is_established());
        assert!(source.verification().artifact_integrity().is_none());
        assert_eq!(
            source.verification().metadata(),
            &MetadataVerification::NotVerified
        );
        assert_eq!(
            report.contract.completeness(),
            &DiscoveryCompleteness::Complete
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_version_keeps_candidate_incomplete_and_identity_inspection_typed() {
        let root = fixture("unknown-runtime-version");
        let build = root.join("Proton-custom");
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(
            build.join("compatibilitytool.vdf"),
            "\"compatibilitytool\" { \"compat_tools\" { \"custom\" { \"display_name\" \"custom\" } } }",
        )
        .unwrap();
        let provider = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![root.clone()],
            include_path: false,
            include_conventional: false,
        })
        .unwrap();
        let report = provider.scan();
        assert_eq!(report.contract.candidates().len(), 1);
        assert!(report.contract.candidates()[0].version_hint().is_none());
        assert_eq!(
            provider
                .inspect_identity(&report.contract.candidates()[0])
                .unwrap_err()
                .kind(),
            ProviderErrorKind::MissingMetadata
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_runtime_metadata_invalidates_identity_inspection() {
        let root = fixture("changed-runtime-metadata");
        let build = root.join("Proton-custom");
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(build.join("version"), "release-a\n").unwrap();
        let provider = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![root.clone()],
            include_path: false,
            include_conventional: false,
        })
        .unwrap();
        let report = provider.scan();
        let candidate = report.contract.candidates()[0].clone();
        fs::write(build.join("version"), "release-b\n").unwrap();
        assert_eq!(
            provider.inspect_identity(&candidate).unwrap_err().kind(),
            ProviderErrorKind::ProviderUnavailable
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn partial_root_failure_preserves_other_candidates_and_empty_scan_is_complete() {
        let base = fixture("partial-scan");
        let good = base.join("good");
        let bad = base.join("bad-file");
        fs::create_dir_all(good.join("Proton-custom")).unwrap();
        write_proton(&good.join("Proton-custom/proton"));
        fs::write(good.join("Proton-custom/version"), "custom\n").unwrap();
        fs::write(&bad, b"not a directory").unwrap();
        let partial = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![good, bad],
            include_path: false,
            include_conventional: false,
        })
        .unwrap()
        .scan();
        assert_eq!(partial.contract.candidates().len(), 1);
        assert!(matches!(
            partial.contract.completeness(),
            DiscoveryCompleteness::Partial { .. }
        ));

        let empty = base.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let complete = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![empty],
            include_path: false,
            include_conventional: false,
        })
        .unwrap()
        .scan();
        assert!(complete.contract.candidates().is_empty());
        assert_eq!(
            complete.contract.completeness(),
            &DiscoveryCompleteness::Complete
        );
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn symlink_cycle_is_bounded() {
        let dir = std::env::temp_dir().join(format!("endfield-cycle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink("b", dir.join("a")).unwrap();
        std::os::unix::fs::symlink("a", dir.join("b")).unwrap();
        assert!(resolve_alias(&dir.join("a"), None).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn directory_enumeration_is_bounded() {
        let dir = fixture("directory-limit");
        fs::create_dir_all(&dir).unwrap();
        for index in 0..=MAX_ENTRIES_PER_DIRECTORY {
            fs::write(dir.join(format!("entry-{index:04}")), b"fixture").unwrap();
        }
        assert_eq!(bounded_entries(&dir), Err("directory-entry-limit"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn absent_and_wrong_type_roots_are_distinguished() {
        let missing = fixture("missing-root");
        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        let outcome = inspect_root(
            &Root {
                path: missing,
                source: SourceKind::Explicit,
                kind: RootKind::RuntimeDirectory,
            },
            &mut found,
            &mut issues,
        );
        assert_eq!(outcome, RootOutcome::Failed);
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "runtime-changed-during-scan")
        );
        let file = fixture("file-root");
        fs::write(&file, b"not a directory").unwrap();
        inspect_root(
            &Root {
                path: file.clone(),
                source: SourceKind::Explicit,
                kind: RootKind::RuntimeDirectory,
            },
            &mut found,
            &mut issues,
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.reason == "root-not-directory")
        );
        let _ = fs::remove_file(file);
    }

    #[test]
    fn permission_denied_root_is_not_reported_as_an_empty_directory() {
        let dir = fixture("permission-denied-root");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
        let permission_is_enforced = fs::read_dir(&dir)
            .err()
            .is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied);

        let mut found = BTreeMap::new();
        let mut issues = Vec::new();
        inspect_root(
            &Root {
                path: dir.clone(),
                source: SourceKind::Explicit,
                kind: RootKind::RuntimeDirectory,
            },
            &mut found,
            &mut issues,
        );
        if permission_is_enforced {
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.reason == "permission-denied")
            );
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn arbitrary_custom_root_nesting_is_not_recursively_traversed() {
        let root = fixture("no-recursive-walk");
        let build = root.join("level-one/level-two/Custom Proton");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&build).unwrap();
        write_proton(&build.join("proton"));
        fs::write(build.join("version"), "custom-build\n").unwrap();
        let report = LinuxRuntimeProvider::new(DiscoveryOptions {
            additional_roots: vec![root.clone()],
            include_path: false,
            include_conventional: false,
        })
        .unwrap()
        .scan();
        assert!(report.contract.candidates().is_empty());
        assert_eq!(
            report.contract.completeness(),
            &DiscoveryCompleteness::Complete
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_reads_reject_invalid_utf8_and_oversized_files() {
        let dir = fixture("metadata-bounds");
        fs::create_dir_all(&dir).unwrap();
        let invalid = dir.join("invalid");
        fs::write(&invalid, [0xff, 0xfe]).unwrap();
        assert!(read_metadata(&invalid).is_none());
        let large = dir.join("large");
        fs::write(&large, vec![b'x'; MAX_METADATA_BYTES as usize + 1]).unwrap();
        assert!(read_metadata(&large).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn metadata_reads_reject_symlinks_and_special_files() {
        let base = fixture("metadata-file-types");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("target"), "safe metadata").unwrap();
        std::os::unix::fs::symlink(base.join("target"), base.join("alias")).unwrap();
        assert!(read_metadata_observed(&base.join("alias")).is_none());
        assert!(read_metadata_observed(Path::new("/dev/null")).is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn architecture_is_preserved_only_from_explicit_supported_metadata() {
        assert_eq!(
            extract_architectures(Some("\"manifest\" { \"architectures\" \"x86_64, i686\" }")),
            Some(vec![ProcessArchitecture::X86, ProcessArchitecture::X86_64])
        );
        assert_eq!(
            extract_architectures(Some("\"manifest\" { \"arch\" \"universal\" }")),
            None
        );
        assert_eq!(
            extract_architectures(Some(
                "\"manifest\" { \"display_name\" \"architecture\" \"label\" \"x86_64\" }"
            )),
            None
        );
        assert_eq!(
            extract_manifest_version(Some(
                "\"manifest\" { \"display_name\" \"version\" \"architecture\" \"universal\" }"
            )),
            None
        );
        assert_eq!(extract_architectures(None), None);
    }

    #[test]
    fn equal_build_labels_at_distinct_locations_remain_distinct() {
        let dir = fixture("same-label");
        let first = dir.join("a/Proton-custom");
        let second = dir.join("b/Proton-custom");
        for build in [&first, &second] {
            fs::create_dir_all(build).unwrap();
            write_proton(&build.join("proton"));
            fs::write(build.join("version"), "custom\n").unwrap();
        }
        let a = inspect_proton_directory(&first, SourceKind::Explicit, &mut Vec::new()).unwrap();
        let b = inspect_proton_directory(&second, SourceKind::Explicit, &mut Vec::new()).unwrap();
        assert_ne!(a.candidate.runtime_id(), b.candidate.runtime_id());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn non_executable_wine_file_is_not_candidate() {
        let dir = std::env::temp_dir().join(format!("endfield-wine-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin/wine"), b"fixture").unwrap();
        fs::set_permissions(dir.join("bin/wine"), fs::Permissions::from_mode(0o600)).unwrap();
        let mut issues = Vec::new();
        assert!(inspect_wine_directory(&dir, SourceKind::Explicit, &mut issues).is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
