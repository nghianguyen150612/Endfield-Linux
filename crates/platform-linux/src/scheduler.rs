//! Read-only process affinity and scheduler-policy queries.

use endfield_unix_domain::Availability;

use crate::errors::DataSourceError;
use crate::hardware_common::HardwareIssue;
use crate::hardware_sources::LinuxHardwareDataSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryAvailability {
    Available,
    Unavailable,
    PermissionRestricted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedulingFacts {
    /// Query support does not establish permission to change affinity.
    pub affinity_query: QueryAvailability,
    pub scheduler_policy_query: QueryAvailability,
    pub allowed_cpu_ids: Option<Vec<u32>>,
    pub scheduler_policy: Option<String>,
    pub current_static_priority: Option<i32>,
    /// P012 performs no set-affinity, set-scheduler, or priority changes.
    pub mutation_permission: QueryAvailability,
}

pub(crate) struct SchedulingCollection {
    pub facts: SchedulingFacts,
    pub capability_availability: Availability,
    pub issues: Vec<HardwareIssue>,
    pub observed: bool,
}

pub(crate) fn collect_scheduling<S: LinuxHardwareDataSource>(source: &S) -> SchedulingCollection {
    let (allowed_cpu_ids, affinity_query, affinity_issue) = match source.process_affinity() {
        Ok(mut cpus) => {
            cpus.sort_unstable();
            if cpus.is_empty() || cpus.windows(2).any(|pair| pair[0] == pair[1]) {
                (
                    None,
                    QueryAvailability::Unknown,
                    Some(HardwareIssue::malformed(
                        "linux-scheduler",
                        "the process CPU affinity mask was empty or invalid",
                    )),
                )
            } else {
                (Some(cpus), QueryAvailability::Available, None)
            }
        }
        Err(error) => {
            let (query, issue) = map_query_error("linux-scheduler", error, "CPU affinity query");
            (None, query, Some(issue))
        }
    };
    let (scheduler_policy, current_static_priority, scheduler_policy_query, scheduler_issue) =
        match source.scheduler_state() {
            Ok(state) if state.priority >= 0 => (
                policy_name(state.policy),
                Some(state.priority),
                QueryAvailability::Available,
                None,
            ),
            Ok(_) => (
                None,
                None,
                QueryAvailability::Unknown,
                Some(HardwareIssue::malformed(
                    "linux-scheduler",
                    "the scheduler policy query returned an invalid priority",
                )),
            ),
            Err(error) => {
                let (query, issue) =
                    map_query_error("linux-scheduler", error, "scheduler policy query");
                (None, None, query, Some(issue))
            }
        };
    let issues = affinity_issue
        .into_iter()
        .chain(scheduler_issue)
        .collect::<Vec<_>>();
    let supported_queries = usize::from(affinity_query == QueryAvailability::Available)
        + usize::from(scheduler_policy_query == QueryAvailability::Available);
    let capability_availability = if supported_queries > 0 {
        Availability::Available
    } else if affinity_query == QueryAvailability::Unavailable
        && scheduler_policy_query == QueryAvailability::Unavailable
    {
        Availability::Unavailable
    } else {
        Availability::Unknown
    };
    SchedulingCollection {
        facts: SchedulingFacts {
            affinity_query,
            scheduler_policy_query,
            allowed_cpu_ids,
            scheduler_policy,
            current_static_priority,
            mutation_permission: QueryAvailability::Unknown,
        },
        capability_availability,
        observed: true,
        issues,
    }
}

fn map_query_error(
    provider: &'static str,
    error: DataSourceError,
    operation: &'static str,
) -> (QueryAvailability, HardwareIssue) {
    let query = match error.kind {
        crate::errors::DataSourceErrorKind::Unsupported
        | crate::errors::DataSourceErrorKind::NotFound => QueryAvailability::Unavailable,
        crate::errors::DataSourceErrorKind::PermissionDenied => {
            QueryAvailability::PermissionRestricted
        }
        _ => QueryAvailability::Unknown,
    };
    let issue = HardwareIssue::from_source(provider, error.kind, operation);
    (query, issue)
}

fn policy_name(state: i32) -> Option<String> {
    let name = match state {
        0 => "other",
        1 => "fifo",
        2 => "round-robin",
        3 => "batch",
        5 => "idle",
        6 => "deadline",
        _ => return None,
    };
    Some(name.to_owned())
}
