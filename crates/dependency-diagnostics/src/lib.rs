//! Portable, read-only dependency assessment. Observations are supplied by callers.
//! No platform discovery, execution, or I/O is performed here.
use serde::Serialize;

pub const CATALOG_REVISION: &str = "2026-11-03.1";
pub const MAX_FINDINGS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Steam,
    Epic,
    Gryphlink,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Minimum,
    Recommended,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    SatisfiedByObservation,
    Missing,
    Unavailable,
    PermissionDenied,
    Stale,
    Conflicting,
    Unsupported,
    Unknown,
    NotComparable,
    NotTested,
    NotApplicable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Information,
    Warning,
    Blocker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Requirement {
    pub id: &'static str,
    pub channel: Channel,
    pub tier: Tier,
    pub component: &'static str,
    pub value: &'static str,
    pub unit: &'static str,
    pub comparison: &'static str,
    pub platform_scope: &'static str,
    pub source_url: &'static str,
    pub evidence_type: &'static str,
    pub observed_on: &'static str,
    pub linux_translation: bool,
    pub limitation: &'static str,
}

/// Official listings report GB without defining its byte multiplier. Keep the
/// source wording intact; never treat the catalog as an exact byte threshold.
pub fn catalog() -> Vec<Requirement> {
    let mut result = Vec::new();
    for (channel, url) in [
        (
            Channel::Steam,
            "https://store.steampowered.com/app/4732690/",
        ),
        (
            Channel::Epic,
            "https://store.epicgames.com/p/arknightsendfield-f0f167",
        ),
    ] {
        for tier in [Tier::Minimum, Tier::Recommended] {
            let (os, cpu, gpu, storage) = match (channel, tier) {
                (Channel::Steam, Tier::Minimum) => (
                    "Windows 10 64-bit",
                    "Intel Core i5-9400F",
                    "NVIDIA GeForce GTX 1060",
                    "60",
                ),
                (Channel::Steam, Tier::Recommended) => (
                    "Windows 10 64-bit",
                    "Intel Core i7-10700K",
                    "NVIDIA GeForce RTX 2060",
                    "60",
                ),
                (Channel::Epic, Tier::Minimum) => (
                    "Windows 10 64-bit",
                    "Intel Core i5-9400F or equivalent",
                    "NVIDIA GeForce GTX 1060 or equivalent",
                    "50",
                ),
                (Channel::Epic, Tier::Recommended) => (
                    "Windows 10/11 64-bit",
                    "Intel Core i7-10700K or equivalent or higher",
                    "NVIDIA GeForce RTX 2060 or equivalent or higher",
                    "50",
                ),
                _ => unreachable!(),
            };
            let prefix = match (channel, tier) {
                (Channel::Steam, Tier::Minimum) => "steam.minimum",
                (Channel::Steam, Tier::Recommended) => "steam.recommended",
                (Channel::Epic, Tier::Minimum) => "epic.minimum",
                _ => "epic.recommended",
            };
            for (component, value, unit, comparison, limitation) in [
                (
                    "os",
                    os,
                    "platform",
                    "source platform requirement",
                    "Windows platform requirement is not a Linux compatibility test.",
                ),
                (
                    "cpu",
                    cpu,
                    "model",
                    "source model or stated equivalent",
                    "CPU performance equivalence is not established.",
                ),
                (
                    "memory",
                    "16",
                    "GB RAM",
                    "at least source-reported amount",
                    "The listing does not define the GB byte multiplier; swap is not physical RAM.",
                ),
                (
                    "gpu",
                    gpu,
                    "model",
                    "source model or stated equivalent",
                    "GPU performance equivalence is not established.",
                ),
                (
                    "directx",
                    "11",
                    "version",
                    "source Windows API requirement",
                    "DirectX is not a Vulkan version or proof of translation support.",
                ),
                (
                    "storage",
                    storage,
                    "GB available",
                    "at least source-reported amount at installation target",
                    "The listing does not define the GB byte multiplier or future updates.",
                ),
            ] {
                let id = match (prefix, component) {
                    ("steam.minimum", "os") => "steam.minimum.os",
                    ("steam.minimum", "cpu") => "steam.minimum.cpu",
                    ("steam.minimum", "memory") => "steam.minimum.memory",
                    ("steam.minimum", "gpu") => "steam.minimum.gpu",
                    ("steam.minimum", "directx") => "steam.minimum.directx",
                    ("steam.minimum", _) => "steam.minimum.storage",
                    ("steam.recommended", "os") => "steam.recommended.os",
                    ("steam.recommended", "cpu") => "steam.recommended.cpu",
                    ("steam.recommended", "memory") => "steam.recommended.memory",
                    ("steam.recommended", "gpu") => "steam.recommended.gpu",
                    ("steam.recommended", "directx") => "steam.recommended.directx",
                    ("steam.recommended", _) => "steam.recommended.storage",
                    ("epic.minimum", "os") => "epic.minimum.os",
                    ("epic.minimum", "cpu") => "epic.minimum.cpu",
                    ("epic.minimum", "memory") => "epic.minimum.memory",
                    ("epic.minimum", "gpu") => "epic.minimum.gpu",
                    ("epic.minimum", "directx") => "epic.minimum.directx",
                    ("epic.minimum", _) => "epic.minimum.storage",
                    (_, "os") => "epic.recommended.os",
                    (_, "cpu") => "epic.recommended.cpu",
                    (_, "memory") => "epic.recommended.memory",
                    (_, "gpu") => "epic.recommended.gpu",
                    (_, "directx") => "epic.recommended.directx",
                    _ => "epic.recommended.storage",
                };
                result.push(Requirement {
                    id,
                    channel,
                    tier,
                    component,
                    value,
                    unit,
                    comparison,
                    platform_scope: "Windows",
                    source_url: url,
                    evidence_type: "official_store_listing",
                    observed_on: "2026-11-03",
                    linux_translation: false,
                    limitation,
                });
            }
            if channel == Channel::Steam {
                result.push(Requirement { id: if tier == Tier::Minimum { "steam.minimum.network" } else { "steam.recommended.network" }, channel, tier, component: "network", value: "Broadband Internet connection", unit: "connection", comparison: "source-reported connection requirement", platform_scope: "Windows", source_url: url, evidence_type: "official_store_listing", observed_on: "2026-11-03", linux_translation: false, limitation: "A local observation cannot establish future service reachability or account access." });
            }
            if channel == Channel::Epic && tier == Tier::Recommended {
                result.push(Requirement {
                    id: "epic.recommended.ssd",
                    channel,
                    tier,
                    component: "storage_medium",
                    value: "SSD recommended",
                    unit: "storage medium",
                    comparison: "recommendation",
                    platform_scope: "Windows",
                    source_url: url,
                    evidence_type: "official_store_listing",
                    observed_on: "2026-11-03",
                    linux_translation: false,
                    limitation: "An unknown destination cannot be assessed.",
                });
            }
            if channel == Channel::Epic {
                result.push(Requirement { id: if tier == Tier::Minimum { "epic.minimum.unpack" } else { "epic.recommended.unpack" }, channel, tier, component: "unpack_storage", value: "40", unit: "GB additional free", comparison: "additional free space for unpacking", platform_scope: "Windows", source_url: url, evidence_type: "official_store_listing", observed_on: "2026-11-03", linux_translation: false, limitation: "The listing does not define the GB byte multiplier or the target filesystem." });
            }
        }
    }
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupLimitState {
    #[default]
    Unknown,
    Unlimited,
    Limited,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MemoryObservation {
    pub physical_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub cgroup_limit_bytes: Option<u64>,
    pub cgroup_limit_state: CgroupLimitState,
    pub cgroup_ancestor_limits_assessed: bool,
    pub swap_bytes: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub component: &'static str,
    pub requirement_id: Option<&'static str>,
    pub state: State,
    pub severity: Severity,
    pub explanation: &'static str,
    pub evidence_source: &'static str,
    pub observed_bytes: Option<u64>,
    pub expected: &'static str,
    pub suggested_action: &'static str,
    pub supported_by_endfield_unix: bool,
    pub requires_separate_approval: bool,
    pub limitation: &'static str,
}

/// Compare only when both plausible GB interpretations produce the same
/// result. Cgroup limits are separate and swap never contributes to RAM.
pub fn assess_memory(memory: &MemoryObservation, channel: Option<Channel>) -> Vec<Finding> {
    let requirement = match channel {
        Some(Channel::Steam) => Some("steam.minimum.memory"),
        Some(Channel::Epic) => Some("epic.minimum.memory"),
        _ => None,
    };
    let Some(requirement_id) = requirement else {
        return vec![];
    };
    let mut findings = Vec::new();
    for (id, value, source, action) in [
        (
            "memory.physical",
            memory.physical_bytes,
            "P012 physical RAM",
            "Review the system memory against the selected store's requirement.",
        ),
        (
            "memory.cgroup_limit",
            memory.cgroup_limit_bytes,
            "P012 cgroup limit",
            "Review the process memory limit and its configuration.",
        ),
    ] {
        let (state, severity, explanation) = match value {
            Some(bytes) if bytes < 16_000_000_000 => (
                State::Missing,
                Severity::Warning,
                "Observed memory is below both plausible interpretations of 16 GB.",
            ),
            Some(bytes) if bytes >= 16 * 1024 * 1024 * 1024 => (
                State::SatisfiedByObservation,
                Severity::Information,
                "Observed memory meets both plausible interpretations of 16 GB.",
            ),
            Some(_) => (
                State::Unknown,
                Severity::Warning,
                "The result depends on the store's unspecified GB interpretation.",
            ),
            None => (
                State::Unknown,
                Severity::Warning,
                "The memory value was not observed; an absent cgroup limit may also mean no limit.",
            ),
        };
        let (state, severity, explanation) = if id == "memory.cgroup_limit"
            && memory.cgroup_limit_state == CgroupLimitState::Unlimited
        {
            (
                State::NotApplicable,
                Severity::Information,
                "The observed leaf cgroup reports no memory limit; ancestor limits may still apply.",
            )
        } else {
            (state, severity, explanation)
        };
        findings.push(Finding { id, component: "memory", requirement_id: Some(requirement_id), state, severity, explanation, evidence_source: source, observed_bytes: value, expected: "16 GB physical RAM (source unit unspecified)", suggested_action: action, supported_by_endfield_unix: false, requires_separate_approval: false, limitation: "Available RAM and runtime headroom can differ; swap is not counted. This is not game compatibility evidence." });
    }
    findings
}

/// Windows-only model and API references cannot establish Linux equivalence.
pub fn assess_noncomparable(channel: Option<Channel>) -> Vec<Finding> {
    let ids: &[(&str, &str, &str)] = match channel {
        Some(Channel::Steam) => &[
            ("steam.minimum.os", "platform", "Windows 10 64-bit"),
            ("steam.minimum.cpu", "cpu", "Intel Core i5-9400F"),
            ("steam.minimum.gpu", "gpu", "NVIDIA GeForce GTX 1060"),
            ("steam.minimum.directx", "translation_layer", "DirectX 11"),
        ],
        Some(Channel::Epic) => &[
            ("epic.minimum.os", "platform", "Windows 10 64-bit"),
            (
                "epic.minimum.cpu",
                "cpu",
                "Intel Core i5-9400F or equivalent",
            ),
            (
                "epic.minimum.gpu",
                "gpu",
                "NVIDIA GeForce GTX 1060 or equivalent",
            ),
            ("epic.minimum.directx", "translation_layer", "DirectX 11"),
        ],
        _ => &[],
    };
    ids.iter().map(|&(requirement_id, component, expected)| Finding {
        id: match component { "platform" => "requirement.windows_platform", "cpu" => "requirement.cpu_equivalence", "gpu" => "requirement.gpu_equivalence", _ => "requirement.directx_translation" },
        component, requirement_id: Some(requirement_id), state: State::NotComparable,
        severity: Severity::Warning, explanation: "The Windows listing cannot be compared reliably to these Linux observations.",
        evidence_source: "official store listing", observed_bytes: None, expected,
        suggested_action: "Review the official requirement and the separate observed hardware and compatibility evidence.",
        supported_by_endfield_unix: false, requires_separate_approval: false,
        limitation: "No CPU or GPU performance equivalence or DirectX translation capability is inferred.",
    }).collect()
}

/// Evaluate only an explicitly selected filesystem's free bytes. Separate
/// installation and unpack observations are never added across filesystems.
pub fn assess_storage(channel: Option<Channel>, available_bytes: Option<u64>) -> Vec<Finding> {
    let requirements: &[(&str, u64)] = match channel {
        Some(Channel::Steam) => &[("steam.minimum.storage", 60)],
        Some(Channel::Epic) => &[("epic.minimum.storage", 50), ("epic.minimum.unpack", 40)],
        _ => &[],
    };
    requirements.iter().map(|&(id, gb)| {
        let (state, explanation) = match available_bytes {
            None => (State::Unknown, "No explicit destination filesystem and available-space observation were supplied."),
            Some(bytes) if bytes < gb * 1_000_000_000 => (State::Missing, "Available space is below both plausible interpretations of the source figure."),
            Some(bytes) if bytes >= gb * 1024 * 1024 * 1024 => (State::SatisfiedByObservation, "Available space meets both plausible interpretations of the source figure."),
            Some(_) => (State::Unknown, "The comparison depends on the store's unspecified GB interpretation."),
        };
        let (state, explanation) = if id == "epic.minimum.unpack" && state == State::SatisfiedByObservation {
            (State::Unknown, "The unpack requirement is additional; the installation allocation and unpack filesystem are not established.")
        } else { (state, explanation) };
        Finding { id: if id == "epic.minimum.unpack" { "storage.unpack" } else { "storage.installation" }, component: "storage", requirement_id: Some(id), state, severity: if state == State::SatisfiedByObservation { Severity::Information } else { Severity::Warning }, explanation, evidence_source: "explicit destination filesystem observation", observed_bytes: available_bytes, expected: if gb == 60 { "60 GB available" } else if gb == 50 { "50 GB available" } else { "40 GB additional for unpacking" }, suggested_action: "Review free space on the intended filesystem against the selected store's guidance.", supported_by_endfield_unix: false, requires_separate_approval: false, limitation: "The store does not specify its GB multiplier; future updates and whether temporary space shares this filesystem are unknown. Separate requirements are not summed." }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requirements_are_attributed_and_channel_specific() {
        let c = catalog();
        assert!(
            c.iter()
                .any(|r| r.id == "steam.minimum.storage" && r.value == "60")
        );
        assert!(
            c.iter()
                .any(|r| r.id == "epic.minimum.storage" && r.value == "50")
        );
        assert!(
            c.iter()
                .any(|r| r.id == "epic.minimum.unpack" && r.value == "40")
        );
        assert!(!c.iter().any(|r| r.channel == Channel::Gryphlink));
        assert!(
            c.iter()
                .all(|r| !r.source_url.is_empty() && !r.linux_translation)
        );
    }
    #[test]
    fn memory_uses_ambiguous_units_and_never_counts_swap() {
        let mut m = MemoryObservation {
            physical_bytes: Some(15_000_000_000),
            swap_bytes: Some(u64::MAX),
            ..Default::default()
        };
        assert_eq!(
            assess_memory(&m, Some(Channel::Steam))[0].state,
            State::Missing
        );
        m.physical_bytes = Some(16_000_000_000);
        assert_eq!(
            assess_memory(&m, Some(Channel::Steam))[0].state,
            State::Unknown
        );
        m.physical_bytes = Some(16 * 1024 * 1024 * 1024);
        assert_eq!(
            assess_memory(&m, Some(Channel::Steam))[0].state,
            State::SatisfiedByObservation
        );
        assert!(assess_memory(&m, Some(Channel::Gryphlink)).is_empty());
    }
    #[test]
    fn storage_target_and_unpack_are_distinct() {
        let unknown = assess_storage(Some(Channel::Epic), None);
        assert_eq!(unknown.len(), 2);
        assert!(unknown.iter().all(|f| f.state == State::Unknown));
        let small = assess_storage(Some(Channel::Steam), Some(59_000_000_000));
        assert_eq!(small[0].state, State::Missing);
        assert_eq!(
            assess_storage(Some(Channel::Epic), Some(u64::MAX))[1].state,
            State::Unknown
        );
        assert!(assess_storage(Some(Channel::Gryphlink), Some(u64::MAX)).is_empty());
    }
    #[test]
    fn unknown_and_unlimited_cgroup_are_distinct() {
        let mut memory = MemoryObservation::default();
        assert_eq!(
            assess_memory(&memory, Some(Channel::Epic))[1].state,
            State::Unknown
        );
        memory.cgroup_limit_state = CgroupLimitState::Unlimited;
        assert_eq!(
            assess_memory(&memory, Some(Channel::Epic))[1].state,
            State::NotApplicable
        );
        memory.cgroup_limit_state = CgroupLimitState::Limited;
        memory.cgroup_limit_bytes = Some(8_000_000_000);
        assert_eq!(
            assess_memory(&memory, Some(Channel::Epic))[1].state,
            State::Missing
        );
    }
    #[test]
    fn catalog_ids_are_unique_and_cpu_gpu_never_compare() {
        let catalog = catalog();
        let mut ids = std::collections::BTreeSet::new();
        assert!(catalog.iter().all(|r| ids.insert(r.id)));
        for channel in [Channel::Steam, Channel::Epic] {
            assert!(
                assess_noncomparable(Some(channel))
                    .iter()
                    .all(|f| f.state == State::NotComparable)
            );
        }
    }
}
