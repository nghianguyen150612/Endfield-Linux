//! One-shot, read-only Linux cpufreq and platform-profile discovery.

use std::collections::BTreeSet;

use endfield_unix_domain::Availability;

use crate::cpu_topology::parse_cpu_list;
use crate::errors::DataSourceErrorKind;
use crate::hardware_common::HardwareIssue;
use crate::hardware_sources::{CpuFrequencyField, LinuxHardwareDataSource, PlatformProfileField};

pub const MAX_CPUFREQ_POLICIES: usize = 256;
const MAX_GOVERNORS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrequencyRange {
    pub minimum_hz: u64,
    pub maximum_hz: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuFrequencyPolicyFacts {
    /// Snapshot-local report index, not a persistent machine identifier.
    pub policy_index: u32,
    pub related_cpu_ids: Option<Vec<u32>>,
    pub affected_cpu_ids: Option<Vec<u32>>,
    pub scaling_driver: Option<String>,
    pub current_governor: Option<String>,
    pub available_governors: Vec<String>,
    pub hardware_frequency_limits: Option<FrequencyRange>,
    pub policy_frequency_limits: Option<FrequencyRange>,
    pub energy_performance_preference: Option<String>,
    pub available_energy_performance_preferences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuFrequencyFacts {
    pub availability: Availability,
    pub policies: Vec<CpuFrequencyPolicyFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformPowerFacts {
    pub availability: Availability,
    pub current_profile: Option<String>,
    pub available_profiles: Vec<String>,
}

pub(crate) struct CpuFrequencyCollection {
    pub facts: CpuFrequencyFacts,
    pub issues: Vec<HardwareIssue>,
    pub observed: bool,
}

pub(crate) struct PlatformPowerCollection {
    pub facts: PlatformPowerFacts,
    pub issues: Vec<HardwareIssue>,
    pub observed: bool,
}

pub(crate) fn collect_cpufreq<S: LinuxHardwareDataSource>(source: &S) -> CpuFrequencyCollection {
    let policy_ids = match source.enumerate_cpufreq_policies() {
        Ok(ids) => ids,
        Err(error) if error.kind == DataSourceErrorKind::NotFound => {
            return CpuFrequencyCollection {
                facts: CpuFrequencyFacts {
                    availability: Availability::Unavailable,
                    policies: Vec::new(),
                },
                issues: Vec::new(),
                observed: true,
            };
        }
        Err(error) if error.kind == DataSourceErrorKind::Unsupported => {
            return CpuFrequencyCollection {
                facts: CpuFrequencyFacts {
                    availability: Availability::Unknown,
                    policies: Vec::new(),
                },
                issues: vec![HardwareIssue::from_source(
                    "linux-cpufreq",
                    error.kind,
                    "CPU frequency policy discovery is unsupported by this source",
                )],
                observed: false,
            };
        }
        Err(error) => {
            return CpuFrequencyCollection {
                facts: CpuFrequencyFacts {
                    availability: Availability::Unknown,
                    policies: Vec::new(),
                },
                issues: vec![HardwareIssue::from_source(
                    "linux-cpufreq",
                    error.kind,
                    "CPU frequency policies could not be enumerated",
                )],
                observed: false,
            };
        }
    };
    if policy_ids.len() > MAX_CPUFREQ_POLICIES {
        return CpuFrequencyCollection {
            facts: CpuFrequencyFacts {
                availability: Availability::Degraded,
                policies: Vec::new(),
            },
            issues: vec![HardwareIssue::from_source(
                "linux-cpufreq",
                DataSourceErrorKind::TooLarge,
                "CPU frequency policy count exceeded the probe bound",
            )],
            observed: true,
        };
    }
    let mut unique = BTreeSet::new();
    let mut issues = Vec::new();
    let mut policies = Vec::new();
    for policy_index in policy_ids {
        if !unique.insert(policy_index) {
            issues.push(HardwareIssue::malformed(
                "linux-cpufreq",
                "CPU frequency policy identifiers were duplicated",
            ));
            continue;
        }
        let (policy, mut policy_issues) = collect_policy(source, policy_index);
        policies.push(policy);
        issues.append(&mut policy_issues);
    }
    policies.sort_by_key(|policy| policy.policy_index);
    let availability = if policies.is_empty() {
        Availability::Unavailable
    } else if issues.is_empty() {
        Availability::Available
    } else {
        Availability::Degraded
    };
    CpuFrequencyCollection {
        facts: CpuFrequencyFacts {
            availability,
            policies,
        },
        issues,
        observed: true,
    }
}

fn collect_policy<S: LinuxHardwareDataSource>(
    source: &S,
    policy_index: u32,
) -> (CpuFrequencyPolicyFacts, Vec<HardwareIssue>) {
    let mut issues = Vec::new();
    let scaling_driver = parse_token(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::ScalingDriver,
            &mut issues,
        ),
        "linux-cpufreq",
        &mut issues,
    );
    let current_governor = parse_token(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::ScalingGovernor,
            &mut issues,
        ),
        "linux-cpufreq",
        &mut issues,
    );
    let available_governors = parse_tokens(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::AvailableGovernors,
            &mut issues,
        ),
        "linux-cpufreq",
        &mut issues,
    );
    let cpuinfo_min = parse_frequency(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::CpuInfoMinFrequencyKhz,
            &mut issues,
        ),
        &mut issues,
    );
    let cpuinfo_max = parse_frequency(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::CpuInfoMaxFrequencyKhz,
            &mut issues,
        ),
        &mut issues,
    );
    let scaling_min = parse_frequency(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::ScalingMinFrequencyKhz,
            &mut issues,
        ),
        &mut issues,
    );
    let scaling_max = parse_frequency(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::ScalingMaxFrequencyKhz,
            &mut issues,
        ),
        &mut issues,
    );
    let related_cpu_ids = parse_list(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::RelatedCpus,
            &mut issues,
        ),
        &mut issues,
    );
    let affected_cpu_ids = parse_list(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::AffectedCpus,
            &mut issues,
        ),
        &mut issues,
    );
    let energy_performance_preference = parse_token(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::EnergyPerformancePreference,
            &mut issues,
        ),
        "linux-cpufreq",
        &mut issues,
    );
    let available_energy_performance_preferences = parse_tokens(
        read_policy_field(
            source,
            policy_index,
            CpuFrequencyField::AvailableEnergyPerformancePreferences,
            &mut issues,
        ),
        "linux-cpufreq",
        &mut issues,
    );
    let hardware_frequency_limits = validate_frequency_range(cpuinfo_min, cpuinfo_max, &mut issues);
    let policy_frequency_limits = validate_frequency_range(scaling_min, scaling_max, &mut issues);
    if let (Some(hardware), Some(policy)) = (&hardware_frequency_limits, &policy_frequency_limits) {
        if policy.minimum_hz < hardware.minimum_hz || policy.maximum_hz > hardware.maximum_hz {
            issues.push(HardwareIssue::malformed(
                "linux-cpufreq",
                "reported policy frequency limits were outside the hardware limits",
            ));
        }
    }
    (
        CpuFrequencyPolicyFacts {
            policy_index,
            related_cpu_ids,
            affected_cpu_ids,
            scaling_driver,
            current_governor,
            available_governors,
            hardware_frequency_limits,
            policy_frequency_limits,
            energy_performance_preference,
            available_energy_performance_preferences,
        },
        issues,
    )
}

fn read_policy_field<S: LinuxHardwareDataSource>(
    source: &S,
    policy_index: u32,
    field: CpuFrequencyField,
    issues: &mut Vec<HardwareIssue>,
) -> Option<Vec<u8>> {
    match source.read_cpufreq_policy_field(policy_index, field) {
        Ok(value) => value,
        Err(error) => {
            issues.push(HardwareIssue::from_source(
                "linux-cpufreq",
                error.kind,
                "a CPU frequency policy field could not be read",
            ));
            None
        }
    }
}

pub(crate) fn collect_platform_power<S: LinuxHardwareDataSource>(
    source: &S,
) -> PlatformPowerCollection {
    let mut issues = Vec::new();
    let current_read = source.read_platform_profile_field(PlatformProfileField::Current);
    let choices_read = source.read_platform_profile_field(PlatformProfileField::Available);
    let observed = current_read.is_ok() || choices_read.is_ok();
    let current = match current_read {
        Ok(bytes) => parse_token(bytes, "linux-power", &mut issues),
        Err(error) => {
            issues.push(HardwareIssue::from_source(
                "linux-power",
                error.kind,
                "the current platform power profile could not be read",
            ));
            None
        }
    };
    let available = match choices_read {
        Ok(bytes) => parse_tokens(bytes, "linux-power", &mut issues),
        Err(error) => {
            issues.push(HardwareIssue::from_source(
                "linux-power",
                error.kind,
                "available platform power profiles could not be read",
            ));
            Vec::new()
        }
    };
    let has_interface_state = current.is_some() || !available.is_empty();
    let availability = if has_interface_state {
        if issues.is_empty() {
            Availability::Available
        } else {
            Availability::Degraded
        }
    } else if observed && issues.is_empty() {
        Availability::Unavailable
    } else {
        Availability::Unknown
    };
    PlatformPowerCollection {
        facts: PlatformPowerFacts {
            availability,
            current_profile: current,
            available_profiles: available,
        },
        issues,
        observed,
    }
}

fn parse_token(
    bytes: Option<Vec<u8>>,
    provider: &'static str,
    issues: &mut Vec<HardwareIssue>,
) -> Option<String> {
    let bytes = bytes?;
    if bytes.len() > 4096 {
        issues.push(HardwareIssue::from_source(
            provider,
            DataSourceErrorKind::TooLarge,
            "a CPU frequency policy value exceeded its bound",
        ));
        return None;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        issues.push(HardwareIssue::from_source(
            provider,
            DataSourceErrorKind::InvalidUtf8,
            "a CPU frequency policy value was not valid UTF-8",
        ));
        return None;
    };
    let text = text.trim();
    if is_identifier(text) {
        Some(text.to_owned())
    } else {
        issues.push(HardwareIssue::malformed(
            provider,
            "a CPU frequency policy name was malformed",
        ));
        None
    }
}

fn parse_tokens(
    bytes: Option<Vec<u8>>,
    provider: &'static str,
    issues: &mut Vec<HardwareIssue>,
) -> Vec<String> {
    let Some(bytes) = bytes else {
        return Vec::new();
    };
    if bytes.len() > 4096 {
        issues.push(HardwareIssue::from_source(
            provider,
            DataSourceErrorKind::TooLarge,
            "a CPU frequency policy list exceeded its bound",
        ));
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        issues.push(HardwareIssue::from_source(
            provider,
            DataSourceErrorKind::InvalidUtf8,
            "a CPU frequency policy list was not valid UTF-8",
        ));
        return Vec::new();
    };
    let mut values = BTreeSet::new();
    for token in text.split_ascii_whitespace() {
        if !is_identifier(token) {
            issues.push(HardwareIssue::malformed(
                provider,
                "a CPU frequency policy list contained an invalid name",
            ));
            return Vec::new();
        }
        values.insert(token.to_owned());
        if values.len() > MAX_GOVERNORS {
            issues.push(HardwareIssue::from_source(
                provider,
                DataSourceErrorKind::TooLarge,
                "a CPU frequency policy list exceeded its item limit",
            ));
            return Vec::new();
        }
    }
    values.into_iter().collect()
}

fn parse_frequency(bytes: Option<Vec<u8>>, issues: &mut Vec<HardwareIssue>) -> Option<u64> {
    let bytes = bytes?;
    let parsed = std::str::from_utf8(&bytes)
        .map(str::trim)
        .ok()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .and_then(|khz| khz.checked_mul(1000));
    match parsed {
        Some(hz) => Some(hz),
        None => {
            issues.push(HardwareIssue::malformed(
                "linux-cpufreq",
                "a CPU frequency value was invalid or overflowed during kHz conversion",
            ));
            None
        }
    }
}

fn parse_list(bytes: Option<Vec<u8>>, issues: &mut Vec<HardwareIssue>) -> Option<Vec<u32>> {
    let bytes = bytes?;
    match parse_cpu_list(&bytes) {
        Ok(cpus) => Some(cpus),
        Err(_) => {
            issues.push(HardwareIssue::malformed(
                "linux-cpufreq",
                "a CPU frequency policy CPU list was invalid",
            ));
            None
        }
    }
}

fn validate_frequency_range(
    minimum: Option<u64>,
    maximum: Option<u64>,
    issues: &mut Vec<HardwareIssue>,
) -> Option<FrequencyRange> {
    match (minimum, maximum) {
        (Some(minimum_hz), Some(maximum_hz)) if minimum_hz <= maximum_hz => Some(FrequencyRange {
            minimum_hz,
            maximum_hz,
        }),
        (Some(_), Some(_)) => {
            issues.push(HardwareIssue::malformed(
                "linux-cpufreq",
                "a CPU frequency minimum exceeded its maximum",
            ));
            None
        }
        _ => None,
    }
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
