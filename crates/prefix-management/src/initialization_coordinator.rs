//! Narrow P034 coordinator for approved managed Wine prefix initialization.
//!
//! The durable state transitions live in `PrefixStore`. This module owns the
//! ordering between those transitions and the descriptor-bound executor. The
//! authorization type is intentionally opaque; production currently has no
//! constructor because the Wine launcher/helper dependency closure is not
//! qualified. Unit tests create a separate synthetic capability under
//! `cfg(test)`.

#[cfg(test)]
use crate::SyntheticInitializationApproval;
use crate::{
    FilesystemIdentity, PrefixError, PrefixId, PrefixInitializationApproval,
    PrefixInitializationPhase, PrefixInitializationPlan, PrefixInitializationProcessOutcome,
    PrefixRecord, PrefixRuntimeAssociation, PrefixStore, current_uid,
};
use endfield_unix_game_launch::linux_executor::{
    BoundExecutable, CancellationToken, DirectoryIdentity, ExecutionAttemptId, ExecutionError,
    ExpectedExecutableIdentity, ObservedProcessResult, OwnedChild, TrustedRoot,
};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    time::Duration,
};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitializationStep {
    RequestValidated,
    IntentPersisted {
        registry_revision: u64,
    },
    ChildCreated {
        attempt_id: ExecutionAttemptId,
    },
    RunningPersisted {
        registry_revision: u64,
    },
    ProcessObserved {
        exit_code: Option<i32>,
    },
    OutcomePersisted {
        phase: PrefixInitializationPhase,
        registry_revision: u64,
    },
    CompletionVerified {
        registry_revision: u64,
    },
    CompletionVerificationPending {
        registry_revision: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationEvidenceSource {
    Wine10StructuralVerifier,
    #[cfg(test)]
    SyntheticNativeFixture,
}

/// A production execution capability. Its fields and constructors stay
/// private so callers cannot turn arbitrary paths or a synthetic fixture into
/// a Managed Wine authorization. The trusted P025/P026/P027 adapter must be
/// added before production can construct this value.
pub struct InitializationExecutionAuthorization {
    binding: ExecutionBinding,
    executable: BoundExecutable,
    working_directory: TrustedRoot,
    wine_prefix: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    evidence_source: InitializationEvidenceSource,
    #[cfg(test)]
    source: AuthorizationSource,
}

struct ExecutionBinding {
    plan_id: String,
    runtime_installation_id: String,
    runtime_generation: u64,
    runtime_pin_id: String,
    runtime_pin_revision: u64,
    runtime_reference_sha256: String,
    runtime_ownership_record_sha256: String,
    runtime_artifact_sha256: String,
    runtime_root: DirectoryIdentity,
    executable_identity: ExpectedExecutableIdentity,
    executable_sha256: String,
    prefix_id: String,
    prefix_registry_revision: u64,
    prefix_generation: u64,
    prefix_identity: FilesystemIdentity,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthorizationSource {
    SyntheticFixture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DurablePhase {
    Starting,
    Running,
    Terminal(InitializationAttemptPhase),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationAttemptPhase {
    Starting,
    Running,
    VerificationPending,
    Failed,
    Interrupted,
    Initialized,
}

#[derive(Debug, Error)]
pub enum InitializationCoordinatorError {
    #[error("initialization authorization does not match the exact P027/P028 binding")]
    AuthorizationMismatch,
    #[error("initialization approval or plan is stale")]
    StaleApproval,
    #[error("initialization was cancelled before durable intent publication")]
    CancelledBeforeIntent,
    #[error("P028 initialization rejected the request: {0}")]
    Prefix(#[from] PrefixError),
    #[error("initializer process creation failed; durable state is {durable_state:?}")]
    SpawnFailed {
        failure: ExecutorFailure,
        durable_state: SpawnFailureState,
    },
    #[error(
        "final execution checks blocked initialization ({blocker:?}); durable state is {durable_state:?}"
    )]
    PreSpawnBlocked {
        blocker: PreSpawnBlocker,
        durable_state: SpawnFailureState,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutorFailure {
    CancelledBeforeSpawn,
    SpawnOrExecFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnFailureState {
    Failed { registry_revision: u64 },
    Interrupted { registry_revision: u64 },
    Unresolved { starting_revision: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreSpawnBlocker {
    AuthorizationMismatch,
    RecoveryStateNotClear,
    RegistryChanged,
    RuntimeOrPrefixChanged,
    Cancelled,
}

#[derive(Debug, Error)]
pub enum InitializationAttemptError {
    #[error("durable initialization transition failed: {0}")]
    Prefix(#[from] PrefixError),
    #[error("bounded direct-child observation failed")]
    Observation,
    #[error("process outcome {outcome:?} was observed but not durably recorded")]
    OutcomePublication {
        attempt_id: ExecutionAttemptId,
        outcome: PrefixInitializationProcessOutcome,
        previous_revision: u64,
        #[source]
        source: PrefixError,
    },
    #[error("initialization attempt is already terminal")]
    AlreadyTerminal,
}

#[derive(Debug)]
pub enum InitializationStart {
    Running(InitializationAttempt),
    /// The child has been created and remains owned. The coordinator has not
    /// observed it; callers may retry the Running transition or explicitly
    /// abandon the unjournaled child, which marks the intent Interrupted.
    RunningPublicationFailed {
        attempt: InitializationAttempt,
        error: PrefixError,
    },
}

#[derive(Debug)]
pub enum InitializationObservation {
    Running {
        attempt_id: ExecutionAttemptId,
        registry_revision: u64,
    },
    Initialized {
        attempt_id: ExecutionAttemptId,
        record: Box<PrefixRecord>,
        registry_revision: u64,
        evidence_source: InitializationEvidenceSource,
    },
    Failed {
        attempt_id: ExecutionAttemptId,
        exit_code: i32,
        registry_revision: u64,
    },
    Interrupted {
        attempt_id: ExecutionAttemptId,
        registry_revision: u64,
    },
    VerificationPending {
        attempt_id: ExecutionAttemptId,
        registry_revision: u64,
        error: PrefixError,
        evidence_source: InitializationEvidenceSource,
    },
}

pub struct InitializationCoordinator {
    store: PrefixStore,
}

type RuntimeValidationCallback = Box<
    dyn for<'association> FnMut(&'association PrefixRuntimeAssociation) -> Result<(), String>
        + Send
        + 'static,
>;

struct InitializationStartHooks<'a, RecoveryState> {
    recovery_state: RecoveryState,
    validate_runtime: RuntimeValidationCallback,
    cancellation: &'a CancellationToken,
}

impl<'a, RecoveryState> InitializationStartHooks<'a, RecoveryState> {
    fn new(
        recovery_state: RecoveryState,
        validate_runtime: impl for<'association> FnMut(
            &'association PrefixRuntimeAssociation,
        ) -> Result<(), String>
        + Send
        + 'static,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            recovery_state,
            validate_runtime: Box::new(validate_runtime),
            cancellation,
        }
    }
}

impl InitializationCoordinator {
    pub fn new(store: PrefixStore) -> Self {
        Self { store }
    }

    /// Persist intent, spawn the exact authorized descriptor, and persist
    /// Running before returning an owner that can observe process completion.
    /// No process exit observation occurs before Running is durable.
    pub fn start(
        &self,
        plan: &PrefixInitializationPlan,
        approval: &PrefixInitializationApproval,
        authorization: InitializationExecutionAuthorization,
        recovery_state: impl FnMut() -> Option<bool>,
        validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String> + Send + 'static,
        cancellation: &CancellationToken,
    ) -> Result<InitializationStart, InitializationCoordinatorError> {
        self.start_with_adapter(
            plan,
            approval.plan_id(),
            false,
            authorization,
            InitializationStartHooks::new(recovery_state, validate_runtime, cancellation),
            &DescriptorBoundExecutor,
        )
    }

    #[cfg(test)]
    fn start_synthetic(
        &self,
        plan: &PrefixInitializationPlan,
        approval: &SyntheticInitializationApproval,
        authorization: InitializationExecutionAuthorization,
        recovery_state: impl FnMut() -> Option<bool>,
        validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String> + Send + 'static,
        cancellation: &CancellationToken,
    ) -> Result<InitializationStart, InitializationCoordinatorError> {
        self.start_with_adapter(
            plan,
            &approval.plan_id,
            true,
            authorization,
            InitializationStartHooks::new(recovery_state, validate_runtime, cancellation),
            &DescriptorBoundExecutor,
        )
    }

    #[cfg(test)]
    fn start_synthetic_with_adapter<RecoveryState>(
        &self,
        plan: &PrefixInitializationPlan,
        approval: &SyntheticInitializationApproval,
        authorization: InitializationExecutionAuthorization,
        hooks: InitializationStartHooks<'_, RecoveryState>,
        executor: &impl InitializationProcessExecutor,
    ) -> Result<InitializationStart, InitializationCoordinatorError>
    where
        RecoveryState: FnMut() -> Option<bool>,
    {
        self.start_with_adapter(
            plan,
            &approval.plan_id,
            true,
            authorization,
            hooks,
            executor,
        )
    }

    fn start_with_adapter<RecoveryState>(
        &self,
        plan: &PrefixInitializationPlan,
        approval_plan_id: &str,
        allow_synthetic_test: bool,
        authorization: InitializationExecutionAuthorization,
        hooks: InitializationStartHooks<'_, RecoveryState>,
        executor: &impl InitializationProcessExecutor,
    ) -> Result<InitializationStart, InitializationCoordinatorError>
    where
        RecoveryState: FnMut() -> Option<bool>,
    {
        let InitializationStartHooks {
            mut recovery_state,
            mut validate_runtime,
            cancellation,
        } = hooks;
        validate_execution_authorization(
            plan,
            approval_plan_id,
            &authorization,
            &self.store,
            allow_synthetic_test,
        )?;
        if cancellation.is_cancelled() {
            return Err(InitializationCoordinatorError::CancelledBeforeIntent);
        }

        let initial_recovery_state = recovery_state();
        let (_intent, starting_revision) = if allow_synthetic_test {
            #[cfg(test)]
            {
                let approval = SyntheticInitializationApproval {
                    plan_id: approval_plan_id.to_owned(),
                };
                self.store.begin_synthetic_initialization_intent(
                    plan,
                    &approval,
                    initial_recovery_state,
                    &mut validate_runtime,
                )?
            }
            #[cfg(not(test))]
            unreachable!("synthetic authorization is test-only")
        } else {
            let approval = PrefixInitializationApproval {
                plan_id: approval_plan_id.to_owned(),
            };
            self.store.begin_initialization_intent(
                plan,
                &approval,
                initial_recovery_state,
                &mut validate_runtime,
            )?
        };
        let id = PrefixId::new(plan.prefix_id.clone())?;
        let mut steps = vec![InitializationStep::RequestValidated];
        steps.push(InitializationStep::IntentPersisted {
            registry_revision: starting_revision,
        });

        // Recheck volatile evidence after durable intent publication and as
        // close as possible to spawn. P028 rechecks its exact Starting state,
        // prefix identity, ownership, emptiness, use, and recovery journal.
        let blocker = if recovery_state() != Some(false) {
            Some(PreSpawnBlocker::RecoveryStateNotClear)
        } else if validate_execution_authorization(
            plan,
            approval_plan_id,
            &authorization,
            &self.store,
            allow_synthetic_test,
        )
        .is_err()
        {
            Some(PreSpawnBlocker::AuthorizationMismatch)
        } else if cancellation.is_cancelled() {
            Some(PreSpawnBlocker::Cancelled)
        } else {
            match self.store.validate_starting_initialization(
                plan,
                starting_revision,
                Some(false),
                &mut validate_runtime,
            ) {
                Ok(()) => None,
                Err(PrefixError::RecoveryRequired) => Some(PreSpawnBlocker::RecoveryStateNotClear),
                Err(PrefixError::RevisionConflict | PrefixError::InitializationIntentMismatch) => {
                    Some(PreSpawnBlocker::RegistryChanged)
                }
                Err(_) => Some(PreSpawnBlocker::RuntimeOrPrefixChanged),
            }
        };
        if let Some(blocker) = blocker {
            let durable_state = match self.store.record_initialization_process_outcome(
                &id,
                &plan.plan_id,
                starting_revision,
                PrefixInitializationProcessOutcome::Interrupted,
            ) {
                Ok(registry_revision) => SpawnFailureState::Interrupted { registry_revision },
                Err(_) => SpawnFailureState::Unresolved { starting_revision },
            };
            return Err(InitializationCoordinatorError::PreSpawnBlocked {
                blocker,
                durable_state,
            });
        }

        let child = match executor.spawn(&authorization, cancellation) {
            Ok(child) => child,
            Err(error) => {
                let failure = execution_failure(&error);
                let outcome = if matches!(error, ExecutionError::Cancelled) {
                    PrefixInitializationProcessOutcome::Interrupted
                } else {
                    PrefixInitializationProcessOutcome::SpawnFailed
                };
                let state = match self.store.record_initialization_process_outcome(
                    &id,
                    &plan.plan_id,
                    starting_revision,
                    outcome,
                ) {
                    Ok(registry_revision)
                        if outcome == PrefixInitializationProcessOutcome::Interrupted =>
                    {
                        SpawnFailureState::Interrupted { registry_revision }
                    }
                    Ok(registry_revision) => SpawnFailureState::Failed { registry_revision },
                    Err(_) => SpawnFailureState::Unresolved { starting_revision },
                };
                return Err(InitializationCoordinatorError::SpawnFailed {
                    failure,
                    durable_state: state,
                });
            }
        };
        let attempt_id = child.attempt_id();
        steps.push(InitializationStep::ChildCreated { attempt_id });
        let mut attempt = InitializationAttempt {
            store: self.store.clone(),
            id,
            plan_id: plan.plan_id.clone(),
            revision: starting_revision,
            phase: DurablePhase::Starting,
            child: Some(child),
            validate_runtime,
            evidence_source: authorization.evidence_source,
            steps,
        };

        // `mark_running` is deliberately called immediately after spawn and
        // before try_wait/wait. The child may already have exited here.
        match attempt.store.mark_initialization_running(
            &attempt.id,
            &attempt.plan_id,
            attempt.revision,
        ) {
            Ok(registry_revision) => {
                attempt.revision = registry_revision;
                attempt.phase = DurablePhase::Running;
                attempt
                    .steps
                    .push(InitializationStep::RunningPersisted { registry_revision });
                Ok(InitializationStart::Running(attempt))
            }
            Err(error) => Ok(InitializationStart::RunningPublicationFailed { attempt, error }),
        }
    }
}

pub struct InitializationAttempt {
    store: PrefixStore,
    id: PrefixId,
    plan_id: String,
    revision: u64,
    phase: DurablePhase,
    child: Option<OwnedChild>,
    validate_runtime: RuntimeValidationCallback,
    evidence_source: InitializationEvidenceSource,
    steps: Vec<InitializationStep>,
}

impl std::fmt::Debug for InitializationAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InitializationAttempt")
            .field("attempt_id", &self.attempt_id())
            .field("process_id", &self.process_id())
            .field("prefix_id", &self.id)
            .field("durable_phase", &self.durable_phase())
            .field("registry_revision", &self.revision)
            .finish()
    }
}

impl InitializationAttempt {
    pub fn attempt_id(&self) -> Option<ExecutionAttemptId> {
        self.child.as_ref().map(OwnedChild::attempt_id)
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(OwnedChild::child_id)
    }

    pub fn durable_phase(&self) -> InitializationAttemptPhase {
        match self.phase {
            DurablePhase::Starting => InitializationAttemptPhase::Starting,
            DurablePhase::Running => InitializationAttemptPhase::Running,
            DurablePhase::Terminal(phase) => phase,
        }
    }

    pub fn registry_revision(&self) -> u64 {
        self.revision
    }

    pub fn steps(&self) -> &[InitializationStep] {
        &self.steps
    }

    /// Observe the owned direct child for at most `timeout`. A timeout leaves
    /// the owner active. When Starting is still durable because the original
    /// Running publication failed, the coordinator retries Running before it
    /// performs any process observation.
    pub fn observe(
        &mut self,
        timeout: Duration,
    ) -> Result<InitializationObservation, InitializationAttemptError> {
        if matches!(self.phase, DurablePhase::Terminal(_)) {
            return Err(InitializationAttemptError::AlreadyTerminal);
        }
        if self.phase == DurablePhase::Starting {
            let revision =
                self.store
                    .mark_initialization_running(&self.id, &self.plan_id, self.revision)?;
            self.revision = revision;
            self.phase = DurablePhase::Running;
            self.steps.push(InitializationStep::RunningPersisted {
                registry_revision: revision,
            });
        }
        let attempt_id = self
            .child
            .as_ref()
            .map(OwnedChild::attempt_id)
            .ok_or(InitializationAttemptError::Observation)?;
        let result = self
            .child
            .as_mut()
            .ok_or(InitializationAttemptError::Observation)?
            .wait_for(timeout)
            .map_err(|_| InitializationAttemptError::Observation)?;
        let Some(result) = result else {
            return Ok(InitializationObservation::Running {
                attempt_id,
                registry_revision: self.revision,
            });
        };
        self.steps.push(InitializationStep::ProcessObserved {
            exit_code: result.status.code(),
        });
        self.child.take();

        let outcome = process_outcome(&result);
        let revision = self
            .store
            .record_initialization_process_outcome(&self.id, &self.plan_id, self.revision, outcome)
            .map_err(|source| InitializationAttemptError::OutcomePublication {
                attempt_id,
                outcome,
                previous_revision: self.revision,
                source,
            })?;
        self.revision = revision;
        let phase = match outcome {
            PrefixInitializationProcessOutcome::ExitedZero => {
                PrefixInitializationPhase::VerificationPending
            }
            PrefixInitializationProcessOutcome::ExitedNonzero(_) => {
                PrefixInitializationPhase::Failed
            }
            PrefixInitializationProcessOutcome::Interrupted
            | PrefixInitializationProcessOutcome::SpawnFailed => {
                PrefixInitializationPhase::Interrupted
            }
        };
        self.steps.push(InitializationStep::OutcomePersisted {
            phase,
            registry_revision: revision,
        });
        self.phase = DurablePhase::Terminal(match outcome {
            PrefixInitializationProcessOutcome::ExitedZero => {
                InitializationAttemptPhase::VerificationPending
            }
            PrefixInitializationProcessOutcome::ExitedNonzero(_) => {
                InitializationAttemptPhase::Failed
            }
            PrefixInitializationProcessOutcome::Interrupted
            | PrefixInitializationProcessOutcome::SpawnFailed => {
                InitializationAttemptPhase::Interrupted
            }
        });

        match outcome {
            PrefixInitializationProcessOutcome::ExitedZero => {
                match self.store.complete_verified_wine10_initialization(
                    &self.id,
                    &self.plan_id,
                    revision,
                    |association| (self.validate_runtime)(association),
                ) {
                    Ok((record, registry_revision)) => {
                        self.revision = registry_revision;
                        self.phase =
                            DurablePhase::Terminal(InitializationAttemptPhase::Initialized);
                        self.steps
                            .push(InitializationStep::CompletionVerified { registry_revision });
                        Ok(InitializationObservation::Initialized {
                            attempt_id,
                            record: Box::new(record),
                            registry_revision,
                            evidence_source: self.evidence_source,
                        })
                    }
                    Err(error) => {
                        self.steps
                            .push(InitializationStep::CompletionVerificationPending {
                                registry_revision: revision,
                            });
                        Ok(InitializationObservation::VerificationPending {
                            attempt_id,
                            registry_revision: revision,
                            error,
                            evidence_source: self.evidence_source,
                        })
                    }
                }
            }
            PrefixInitializationProcessOutcome::ExitedNonzero(exit_code) => {
                Ok(InitializationObservation::Failed {
                    attempt_id,
                    exit_code,
                    registry_revision: revision,
                })
            }
            PrefixInitializationProcessOutcome::Interrupted
            | PrefixInitializationProcessOutcome::SpawnFailed => {
                Ok(InitializationObservation::Interrupted {
                    attempt_id,
                    registry_revision: revision,
                })
            }
        }
    }

    /// Cancel only the direct child held by this attempt. If it already exited,
    /// its actual outcome is recorded. wineserver and other descendants are
    /// never located or signaled by name or PID.
    pub fn cancel(&mut self) -> Result<InitializationObservation, InitializationAttemptError> {
        if self.phase == DurablePhase::Starting {
            let revision =
                self.store
                    .mark_initialization_running(&self.id, &self.plan_id, self.revision)?;
            self.revision = revision;
            self.phase = DurablePhase::Running;
            self.steps.push(InitializationStep::RunningPersisted {
                registry_revision: revision,
            });
        }
        self.child
            .as_mut()
            .ok_or(InitializationAttemptError::AlreadyTerminal)?
            .cancel()
            .map_err(|_| InitializationAttemptError::Observation)?;
        self.observe(Duration::from_secs(5))
    }

    /// Explicitly abandon an attempt whose Running transition could not be
    /// persisted. Dropping the owner kills/reaps only its direct child without
    /// reading an exit result, then records Interrupted against the exact
    /// Starting revision. A revision conflict leaves the existing intent
    /// unresolved and blocked.
    pub fn abandon_unjournaled(mut self) -> Result<u64, PrefixError> {
        if self.phase != DurablePhase::Starting {
            return Err(PrefixError::InvalidState);
        }
        self.child.take();
        let revision =
            self.store
                .mark_initialization_interrupted(&self.id, &self.plan_id, self.revision)?;
        self.phase = DurablePhase::Terminal(InitializationAttemptPhase::Interrupted);
        self.revision = revision;
        Ok(revision)
    }
}

trait InitializationProcessExecutor {
    fn spawn(
        &self,
        authorization: &InitializationExecutionAuthorization,
        cancellation: &CancellationToken,
    ) -> Result<OwnedChild, ExecutionError>;
}

struct DescriptorBoundExecutor;

impl InitializationProcessExecutor for DescriptorBoundExecutor {
    fn spawn(
        &self,
        authorization: &InitializationExecutionAuthorization,
        cancellation: &CancellationToken,
    ) -> Result<OwnedChild, ExecutionError> {
        authorization.executable.spawn_cancellable(
            &authorization.arguments,
            &authorization.environment,
            &authorization.working_directory,
            cancellation,
        )
    }
}

fn validate_execution_authorization(
    plan: &PrefixInitializationPlan,
    approval_plan_id: &str,
    authorization: &InitializationExecutionAuthorization,
    store: &PrefixStore,
    allow_synthetic_test: bool,
) -> Result<(), InitializationCoordinatorError> {
    plan.validate_integrity()
        .map_err(|_| InitializationCoordinatorError::StaleApproval)?;
    if approval_plan_id != plan.plan_id {
        return Err(InitializationCoordinatorError::StaleApproval);
    }
    #[cfg(not(test))]
    if authorization.binding.plan_id != plan.plan_id || allow_synthetic_test {
        return Err(InitializationCoordinatorError::AuthorizationMismatch);
    }
    #[cfg(test)]
    if authorization.binding.plan_id != plan.plan_id
        || authorization.source != AuthorizationSource::SyntheticFixture
        || !allow_synthetic_test
    {
        return Err(InitializationCoordinatorError::AuthorizationMismatch);
    }
    let prefix_id = PrefixId::new(plan.prefix_id.clone())
        .map_err(|_| InitializationCoordinatorError::AuthorizationMismatch)?;
    let expected_prefix_path = store.target_path(&prefix_id);
    let inspection = store
        .show(&prefix_id, |_| Ok(()))
        .map_err(|_| InitializationCoordinatorError::AuthorizationMismatch)?;
    let runtime_reference = &inspection.record.runtime.runtime;
    let runtime_reference_hash = crate::runtime_reference_sha256(runtime_reference)
        .map_err(|_| InitializationCoordinatorError::AuthorizationMismatch)?;
    let runtime_binding_matches = matches!(
        runtime_reference,
        endfield_unix_runtime_selection::PinnedRuntimeReference::Managed(runtime)
            if runtime.family == "wine"
                && runtime.lineage == "upstream-wine"
                && runtime.build == "wine-10.0"
                && runtime.trust
                    == endfield_unix_runtime_selection::Provenance::
                        IndependentlyAuthenticatedPublisher
                && runtime.installation_id == authorization.binding.runtime_installation_id
                && runtime.generation == authorization.binding.runtime_generation
                && runtime.ownership_record_sha256
                    == authorization.binding.runtime_ownership_record_sha256
                && runtime.artifact_sha256 == authorization.binding.runtime_artifact_sha256
    );
    let executable_identity = authorization.executable.identity();
    let root_identity = authorization.executable.root_identity();
    let expected = authorization.binding.executable_identity;
    let has_exact_environment = authorization.environment.len() == 1
        && authorization.environment[0].0 == OsStr::new("WINEPREFIX")
        && authorization.environment[0].1 == expected_prefix_path.as_os_str();
    if authorization.binding.runtime_installation_id.is_empty()
        || authorization.binding.runtime_generation == 0
        || authorization.binding.runtime_pin_id != plan.runtime_pin_id
        || authorization.binding.runtime_pin_revision != plan.runtime_pin_revision
        || authorization.binding.runtime_reference_sha256 != plan.runtime_reference_sha256
        || runtime_reference_hash != plan.runtime_reference_sha256
        || !runtime_binding_matches
        || inspection.record.runtime.pin_id != plan.runtime_pin_id
        || inspection.record.runtime.pin_revision != plan.runtime_pin_revision
        || inspection.record.runtime.adapter_preference
            != Some(crate::AdapterPreference::ManagedWine)
        || inspection.record.scope != plan.scope
        || inspection.record.owner_uid != current_uid()
        || inspection.record.filesystem_identity != plan.filesystem_identity
        || inspection.record.generation != plan.prefix_generation
        || !valid_sha256(&authorization.binding.executable_sha256)
        || authorization.executable.integrity_sha256()
            != Some(authorization.binding.executable_sha256.as_str())
        || !authorization
            .executable
            .sha256()
            .is_ok_and(|digest| digest == authorization.binding.executable_sha256)
        || executable_identity.device != expected.device
        || executable_identity.inode != expected.inode
        || executable_identity.size != expected.size
        || root_identity != authorization.binding.runtime_root
        || root_identity.owner_uid != current_uid()
        || expected_prefix_path != authorization.wine_prefix
        || !has_exact_environment
        || authorization.binding.prefix_id != plan.prefix_id
        || authorization.binding.prefix_registry_revision != plan.registry_revision
        || authorization.binding.prefix_generation != plan.prefix_generation
        || Some(authorization.binding.prefix_identity) != plan.filesystem_identity
        || authorization.working_directory.identity().device
            != authorization.binding.prefix_identity.device
        || authorization.working_directory.identity().inode
            != authorization.binding.prefix_identity.inode
        || authorization.working_directory.identity().owner_uid != current_uid()
    {
        return Err(InitializationCoordinatorError::AuthorizationMismatch);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn execution_failure(error: &ExecutionError) -> ExecutorFailure {
    if matches!(error, ExecutionError::Cancelled) {
        ExecutorFailure::CancelledBeforeSpawn
    } else {
        ExecutorFailure::SpawnOrExecFailed
    }
}

fn process_outcome(result: &ObservedProcessResult) -> PrefixInitializationProcessOutcome {
    match result.status.code() {
        Some(0) => PrefixInitializationProcessOutcome::ExitedZero,
        Some(code) => PrefixInitializationProcessOutcome::ExitedNonzero(code),
        None => PrefixInitializationProcessOutcome::Interrupted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdapterPreference, InitializationEvidenceOutcome, MachineId, PrefixLifecycleState,
        PrefixRuntimeAssociation, PrefixScope, RuntimeSelectionSource,
    };
    use endfield_unix_runtime_selection::{
        ManagedRuntimeReference, PinnedRuntimeReference, Provenance,
    };
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::Path,
        process::Command,
        sync::{Arc, Barrier, atomic::AtomicUsize},
        thread,
    };
    use tempfile::TempDir;

    struct Fixture {
        _temporary: TempDir,
        store: PrefixStore,
        id: PrefixId,
        plan: PrefixInitializationPlan,
        approval: SyntheticInitializationApproval,
        runtime: PrefixRuntimeAssociation,
        unrelated_id: PrefixId,
        unrelated_record: PrefixRecord,
        game_data: PathBuf,
        child_root: PathBuf,
        child_path: PathBuf,
    }

    impl Fixture {
        fn new(prefix_id: &str) -> Self {
            let temporary = tempfile::tempdir().unwrap();
            let paths = endfield_unix_storage::AppPaths::from_base_dirs(
                temporary.path().join("config"),
                temporary.path().join("data"),
                temporary.path().join("cache"),
            )
            .unwrap();
            let store = crate::tests::coordinator_prefix_store(paths.data_dir().to_path_buf());
            let machine = MachineId::from_digest("a".repeat(64)).unwrap();
            let scope = PrefixScope::machine(machine);
            let runtime = PrefixRuntimeAssociation {
                runtime: PinnedRuntimeReference::Managed(ManagedRuntimeReference {
                    installation_id: "wine-synthetic-fixture".into(),
                    family: "wine".into(),
                    lineage: "upstream-wine".into(),
                    provider: "synthetic-tests".into(),
                    project: "synthetic-tests".into(),
                    build: "wine-10.0".into(),
                    artifact_sha256: "a".repeat(64),
                    manifest_sha256: "b".repeat(64),
                    generation: 1,
                    ownership_record_sha256: "c".repeat(64),
                    trust: Provenance::IndependentlyAuthenticatedPublisher,
                }),
                selection_source: RuntimeSelectionSource::MachinePin,
                pin_id: "wine-synthetic-pin".into(),
                pin_revision: 9,
                adapter_preference: Some(AdapterPreference::ManagedWine),
            };
            let id = PrefixId::new(prefix_id).unwrap();
            let unrelated_id = PrefixId::new("coordinator-unrelated").unwrap();
            let (unrelated_record, revision) = store
                .create(unrelated_id.clone(), scope.clone(), runtime.clone(), 0)
                .unwrap();
            let (record, revision) = store
                .create(id.clone(), scope, runtime.clone(), revision)
                .unwrap();
            let plan = store
                .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
                .unwrap();
            assert!(!plan.execution_supported);
            let approval = plan.synthetic_approval_for_test(&plan.plan_id).unwrap();

            let child_root = temporary.path().join("synthetic-runtime");
            fs::create_dir(&child_root).unwrap();
            fs::set_permissions(&child_root, fs::Permissions::from_mode(0o700)).unwrap();
            let child_path = child_root.join("wine-fixture");
            let fixture_source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/synthetic_initializer.c");
            let status = Command::new("cc")
                .arg("-std=c17")
                .arg("-O2")
                .arg("-Wall")
                .arg("-Wextra")
                .arg("-Werror")
                .arg("-o")
                .arg(&child_path)
                .arg(fixture_source)
                .status()
                .expect("C compiler required for native coordinator fixture");
            assert!(status.success());

            let game_data = temporary.path().join("unrelated-game-data.bin");
            fs::write(&game_data, b"must remain unchanged").unwrap();
            Self {
                _temporary: temporary,
                store,
                id,
                plan,
                approval,
                runtime,
                unrelated_id,
                unrelated_record,
                game_data,
                child_root,
                child_path,
            }
        }

        fn authorization(&self, mode: &str) -> InitializationExecutionAuthorization {
            let uid = current_uid();
            let runtime_metadata = fs::metadata(&self.child_root).unwrap();
            let runtime_root = TrustedRoot::open(
                &self.child_root,
                DirectoryIdentity {
                    device: runtime_metadata.dev(),
                    inode: runtime_metadata.ino(),
                    owner_uid: runtime_metadata.uid(),
                },
                uid,
            )
            .unwrap();
            let executable =
                BoundExecutable::open(&runtime_root, Path::new("wine-fixture"), uid).unwrap();
            let identity = executable.identity();
            let executable = BoundExecutable::open_matching(
                &runtime_root,
                Path::new("wine-fixture"),
                uid,
                ExpectedExecutableIdentity {
                    device: identity.device,
                    inode: identity.inode,
                    size: identity.size,
                },
                &executable.sha256().unwrap(),
            )
            .unwrap();
            let prefix_path = self.store.target_path(&self.id);
            let prefix_metadata = fs::metadata(&prefix_path).unwrap();
            let prefix_root = TrustedRoot::open(
                &prefix_path,
                DirectoryIdentity {
                    device: prefix_metadata.dev(),
                    inode: prefix_metadata.ino(),
                    owner_uid: prefix_metadata.uid(),
                },
                uid,
            )
            .unwrap();
            let plan_identity = executable.identity();
            InitializationExecutionAuthorization {
                binding: ExecutionBinding {
                    plan_id: self.plan.plan_id.clone(),
                    runtime_installation_id: "wine-synthetic-fixture".into(),
                    runtime_generation: 1,
                    runtime_pin_id: self.plan.runtime_pin_id.clone(),
                    runtime_pin_revision: self.plan.runtime_pin_revision,
                    runtime_reference_sha256: self.plan.runtime_reference_sha256.clone(),
                    runtime_ownership_record_sha256: "c".repeat(64),
                    runtime_artifact_sha256: "a".repeat(64),
                    runtime_root: runtime_root.identity(),
                    executable_identity: ExpectedExecutableIdentity {
                        device: plan_identity.device,
                        inode: plan_identity.inode,
                        size: plan_identity.size,
                    },
                    executable_sha256: executable.integrity_sha256().unwrap().into(),
                    prefix_id: self.plan.prefix_id.clone(),
                    prefix_registry_revision: self.plan.registry_revision,
                    prefix_generation: self.plan.prefix_generation,
                    prefix_identity: self.plan.filesystem_identity.unwrap(),
                },
                executable,
                working_directory: prefix_root,
                wine_prefix: prefix_path.clone(),
                arguments: vec!["synthetic-wineboot".into(), mode.into()],
                environment: vec![("WINEPREFIX".into(), prefix_path.into_os_string())],
                evidence_source: InitializationEvidenceSource::SyntheticNativeFixture,
                source: AuthorizationSource::SyntheticFixture,
            }
        }
    }

    struct FailingExecutor;

    impl InitializationProcessExecutor for FailingExecutor {
        fn spawn(
            &self,
            _authorization: &InitializationExecutionAuthorization,
            _cancellation: &CancellationToken,
        ) -> Result<OwnedChild, ExecutionError> {
            Err(ExecutionError::Spawn(std::io::Error::other(
                "fixture failure",
            )))
        }
    }

    struct DelayedReturnExecutor {
        delay: Duration,
    }

    impl InitializationProcessExecutor for DelayedReturnExecutor {
        fn spawn(
            &self,
            authorization: &InitializationExecutionAuthorization,
            cancellation: &CancellationToken,
        ) -> Result<OwnedChild, ExecutionError> {
            let child = DescriptorBoundExecutor.spawn(authorization, cancellation)?;
            // Keep the returned owner without observing it. The native fixture
            // exits during this delay, before the coordinator can persist
            // Running. The coordinator must still publish Running first.
            std::thread::sleep(self.delay);
            Ok(child)
        }
    }

    struct RevisionBumpingExecutor {
        store: PrefixStore,
        scope: PrefixScope,
        runtime: PrefixRuntimeAssociation,
        expected_revision: u64,
    }

    impl InitializationProcessExecutor for RevisionBumpingExecutor {
        fn spawn(
            &self,
            authorization: &InitializationExecutionAuthorization,
            cancellation: &CancellationToken,
        ) -> Result<OwnedChild, ExecutionError> {
            let child = DescriptorBoundExecutor.spawn(authorization, cancellation)?;
            self.store
                .create(
                    PrefixId::new("coordinator-revision-race").unwrap(),
                    self.scope.clone(),
                    self.runtime.clone(),
                    self.expected_revision,
                )
                .expect("concurrent unrelated registry update");
            Ok(child)
        }
    }

    fn start(fixture: &Fixture, mode: &str) -> InitializationStart {
        InitializationCoordinator::new(fixture.store.clone())
            .start_synthetic(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization(mode),
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            )
            .unwrap()
    }

    fn running(start: InitializationStart) -> InitializationAttempt {
        match start {
            InitializationStart::Running(attempt) => attempt,
            InitializationStart::RunningPublicationFailed { .. } => {
                panic!("running publication unexpectedly failed")
            }
        }
    }

    #[test]
    fn native_elf_runs_through_durable_running_and_verified_completion() {
        let fixture = Fixture::new("coordinator-success");
        let unrelated_before = fs::read(&fixture.game_data).unwrap();
        let start = start(&fixture, "complete");
        let mut attempt = running(start);
        let steps = attempt.steps().to_vec();
        let intent_at = steps
            .iter()
            .position(|step| matches!(step, InitializationStep::IntentPersisted { .. }))
            .unwrap();
        let spawn_at = steps
            .iter()
            .position(|step| matches!(step, InitializationStep::ChildCreated { .. }))
            .unwrap();
        let running_at = steps
            .iter()
            .position(|step| matches!(step, InitializationStep::RunningPersisted { .. }))
            .unwrap();
        assert!(intent_at < spawn_at && spawn_at < running_at);

        let observed = attempt.observe(Duration::from_secs(5)).unwrap();
        let (record, revision) = match observed {
            InitializationObservation::Initialized {
                record,
                registry_revision,
                evidence_source,
                ..
            } => {
                assert_eq!(
                    evidence_source,
                    InitializationEvidenceSource::SyntheticNativeFixture
                );
                (record, registry_revision)
            }
            other => panic!("expected fixture completion, got {other:?}"),
        };
        assert_eq!(
            attempt.durable_phase(),
            InitializationAttemptPhase::Initialized
        );
        assert_eq!(record.lifecycle, PrefixLifecycleState::Initialized);
        assert_eq!(
            record.initialization_evidence.unwrap().outcome,
            InitializationEvidenceOutcome::CompletedAndVerified
        );
        assert_eq!(record.runtime, fixture.runtime);
        assert_eq!(
            fixture
                .store
                .show(&fixture.unrelated_id, |_| Ok(()))
                .unwrap()
                .record,
            fixture.unrelated_record
        );
        assert_eq!(
            fixture
                .store
                .validate(&fixture.id, |_| Ok(()))
                .unwrap()
                .registry_revision,
            revision
        );
        assert_eq!(fs::read(&fixture.game_data).unwrap(), unrelated_before);
    }

    #[test]
    fn immediate_exit_is_observed_only_after_running_is_persisted() {
        let fixture = Fixture::new("coordinator-immediate");
        let mut attempt = running(start(&fixture, "complete"));
        let initial_steps = attempt.steps().to_vec();
        assert!(matches!(
            initial_steps.last(),
            Some(InitializationStep::RunningPersisted { .. })
        ));
        let result = attempt.observe(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            result,
            InitializationObservation::Initialized { .. }
        ));
        let observed_at = attempt
            .steps()
            .iter()
            .position(|step| matches!(step, InitializationStep::ProcessObserved { .. }))
            .unwrap();
        let running_at = attempt
            .steps()
            .iter()
            .position(|step| matches!(step, InitializationStep::RunningPersisted { .. }))
            .unwrap();
        assert!(running_at < observed_at);
    }

    #[test]
    fn child_exit_before_running_commit_is_still_published_in_order() {
        let fixture = Fixture::new("coordinator-exit-before-running");
        let coordinator = InitializationCoordinator::new(fixture.store.clone());
        let start = coordinator
            .start_synthetic_with_adapter(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("complete"),
                InitializationStartHooks::new(
                    || Some(false),
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                &DelayedReturnExecutor {
                    delay: Duration::from_millis(150),
                },
            )
            .unwrap();
        let mut attempt = running(start);
        assert!(matches!(
            attempt.steps().last(),
            Some(InitializationStep::RunningPersisted { .. })
        ));
        assert!(matches!(
            attempt.observe(Duration::from_secs(1)).unwrap(),
            InitializationObservation::Initialized { .. }
        ));
        let steps = attempt.steps();
        let running_at = steps
            .iter()
            .position(|step| matches!(step, InitializationStep::RunningPersisted { .. }))
            .unwrap();
        let observed_at = steps
            .iter()
            .position(|step| matches!(step, InitializationStep::ProcessObserved { .. }))
            .unwrap();
        assert!(running_at < observed_at);
    }

    #[test]
    fn nonzero_exit_is_persisted_as_failed_and_prefix_data_is_preserved() {
        let fixture = Fixture::new("coordinator-nonzero");
        let mut attempt = running(start(&fixture, "exit37"));
        assert!(matches!(
            attempt.observe(Duration::from_secs(5)).unwrap(),
            InitializationObservation::Failed { exit_code: 37, .. }
        ));
        assert_eq!(attempt.durable_phase(), InitializationAttemptPhase::Failed);
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(record.lifecycle, PrefixLifecycleState::RecoveryRequired);
        assert!(fixture.store.target_path(&fixture.id).exists());
        assert_eq!(
            fs::read(&fixture.game_data).unwrap(),
            b"must remain unchanged"
        );
    }

    #[test]
    fn zero_exit_without_completion_artifacts_stays_verification_pending() {
        let fixture = Fixture::new("coordinator-no-evidence");
        let mut attempt = running(start(&fixture, "noevidence"));
        assert!(matches!(
            attempt.observe(Duration::from_secs(5)).unwrap(),
            InitializationObservation::VerificationPending { .. }
        ));
        assert_eq!(
            attempt.durable_phase(),
            InitializationAttemptPhase::VerificationPending
        );
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(record.lifecycle, PrefixLifecycleState::Incomplete);
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::VerificationPending
        );
    }

    #[test]
    fn spawn_failure_is_durable_and_cancel_before_intent_is_a_noop() {
        let fixture = Fixture::new("coordinator-spawn-fail");
        let coordinator = InitializationCoordinator::new(fixture.store.clone());
        let error = coordinator
            .start_synthetic_with_adapter(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("complete"),
                InitializationStartHooks::new(
                    || Some(false),
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                &FailingExecutor,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            InitializationCoordinatorError::SpawnFailed {
                durable_state: SpawnFailureState::Failed { .. },
                ..
            }
        ));
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(record.lifecycle, PrefixLifecycleState::RecoveryRequired);
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Failed
        );

        let second = Fixture::new("coordinator-cancel-before-intent");
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            InitializationCoordinator::new(second.store.clone()).start_synthetic(
                &second.plan,
                &second.approval,
                second.authorization("complete"),
                || Some(false),
                |_| Ok(()),
                &cancellation,
            ),
            Err(InitializationCoordinatorError::CancelledBeforeIntent)
        ));
        assert!(
            second
                .store
                .show(&second.id, |_| Ok(()))
                .unwrap()
                .record
                .initialization_intent
                .is_none()
        );
    }

    #[test]
    fn running_publication_failure_keeps_child_owned_and_never_observes_it() {
        let fixture = Fixture::new("coordinator-running-publication-failure");
        let coordinator = InitializationCoordinator::new(fixture.store.clone());
        let result = coordinator
            .start_synthetic_with_adapter(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("sleep"),
                InitializationStartHooks::new(
                    || Some(false),
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                &RevisionBumpingExecutor {
                    store: fixture.store.clone(),
                    scope: fixture.plan.scope.clone(),
                    runtime: fixture.runtime.clone(),
                    expected_revision: fixture.plan.registry_revision + 1,
                },
            )
            .unwrap();
        let mut attempt = match result {
            InitializationStart::RunningPublicationFailed { attempt, error } => {
                assert!(matches!(error, PrefixError::RevisionConflict));
                attempt
            }
            other => panic!("expected failed Running publication, got {other:?}"),
        };
        let id = attempt.attempt_id().unwrap();
        assert!(attempt.process_id().is_some());
        assert_eq!(
            attempt.durable_phase(),
            InitializationAttemptPhase::Starting
        );
        assert!(matches!(
            attempt.observe(Duration::from_millis(20)),
            Err(InitializationAttemptError::Prefix(
                PrefixError::RevisionConflict
            ))
        ));
        assert_eq!(attempt.attempt_id(), Some(id));
        assert!(
            !attempt
                .steps()
                .iter()
                .any(|step| matches!(step, InitializationStep::ProcessObserved { .. }))
        );
        assert!(matches!(
            attempt.abandon_unjournaled(),
            Err(PrefixError::RevisionConflict)
        ));
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Starting
        );
    }

    #[test]
    fn cancellation_after_spawn_records_interrupted_without_retry() {
        let fixture = Fixture::new("coordinator-cancel-running");
        let mut attempt = running(start(&fixture, "partial-sleep"));
        assert!(matches!(
            attempt.cancel().unwrap(),
            InitializationObservation::Interrupted { .. }
        ));
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(record.lifecycle, PrefixLifecycleState::RecoveryRequired);
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Interrupted
        );
        assert!(
            fixture
                .store
                .target_path(&fixture.id)
                .join("drive_c")
                .exists()
        );
        assert!(
            fixture
                .store
                .target_path(&fixture.id)
                .join("dosdevices/c:")
                .symlink_metadata()
                .is_ok()
        );
        assert!(
            !fixture
                .store
                .target_path(&fixture.id)
                .join("system.reg")
                .exists()
        );
    }

    #[test]
    fn invalid_binding_stale_approval_revision_and_recovery_block_spawn() {
        let fixture = Fixture::new("coordinator-invalid-binding");
        let coordinator = InitializationCoordinator::new(fixture.store.clone());
        let mut authorization = fixture.authorization("complete");
        authorization.binding.runtime_pin_revision += 1;
        assert!(matches!(
            coordinator.start_synthetic(
                &fixture.plan,
                &fixture.approval,
                authorization,
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            ),
            Err(InitializationCoordinatorError::AuthorizationMismatch)
        ));
        assert!(
            fixture
                .store
                .show(&fixture.id, |_| Ok(()))
                .unwrap()
                .record
                .initialization_intent
                .is_none()
        );

        let mut stale_approval = fixture.approval.clone();
        stale_approval.plan_id =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
        assert!(matches!(
            coordinator.start_synthetic(
                &fixture.plan,
                &stale_approval,
                fixture.authorization("complete"),
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            ),
            Err(InitializationCoordinatorError::StaleApproval)
        ));

        let (_other, _revision) = fixture
            .store
            .create(
                PrefixId::new("coordinator-revision-bump").unwrap(),
                fixture.plan.scope.clone(),
                fixture.runtime.clone(),
                fixture.plan.registry_revision,
            )
            .unwrap();
        assert!(matches!(
            coordinator.start_synthetic(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("complete"),
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            ),
            Err(InitializationCoordinatorError::Prefix(
                PrefixError::RevisionConflict
            ))
        ));
    }

    #[test]
    fn prefix_identity_replacement_blocks_before_child_creation() {
        let fixture = Fixture::new("coordinator-prefix-replaced");
        let authorization = fixture.authorization("complete");
        let path = fixture.store.target_path(&fixture.id);
        let displaced = fixture._temporary.path().join("displaced-prefix");
        fs::rename(&path, &displaced).unwrap();
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            InitializationCoordinator::new(fixture.store.clone()).start_synthetic(
                &fixture.plan,
                &fixture.approval,
                authorization,
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            ),
            Err(InitializationCoordinatorError::Prefix(PrefixError::Blocked))
        ));
        assert!(displaced.exists());
        assert!(fs::read(&fixture.game_data).unwrap() == b"must remain unchanged");
    }

    #[test]
    fn outcome_publication_conflict_leaves_running_intent_blocked() {
        let fixture = Fixture::new("coordinator-outcome-conflict");
        let mut attempt = running(start(&fixture, "sleep"));
        fixture
            .store
            .create(
                PrefixId::new("coordinator-outcome-revision").unwrap(),
                fixture.plan.scope.clone(),
                fixture.runtime.clone(),
                attempt.registry_revision(),
            )
            .unwrap();
        assert!(matches!(
            attempt.cancel(),
            Err(InitializationAttemptError::OutcomePublication {
                outcome: PrefixInitializationProcessOutcome::Interrupted,
                ..
            })
        ));
        let inspection = fixture.store.show(&fixture.id, |_| Ok(())).unwrap();
        assert_eq!(
            inspection.observed_lifecycle,
            PrefixLifecycleState::RecoveryRequired
        );
        assert_eq!(
            inspection.record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Running
        );
    }

    #[test]
    fn pending_or_unknown_recovery_and_stale_runtime_block_before_intent() {
        for recovery in [None, Some(true)] {
            let fixture = Fixture::new("coordinator-recovery");
            assert!(matches!(
                InitializationCoordinator::new(fixture.store.clone()).start_synthetic(
                    &fixture.plan,
                    &fixture.approval,
                    fixture.authorization("complete"),
                    || recovery,
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                Err(InitializationCoordinatorError::Prefix(
                    PrefixError::RecoveryRequired
                ))
            ));
            assert!(
                fixture
                    .store
                    .show(&fixture.id, |_| Ok(()))
                    .unwrap()
                    .record
                    .initialization_intent
                    .is_none()
            );
        }
        let fixture = Fixture::new("coordinator-runtime-stale");
        assert!(matches!(
            InitializationCoordinator::new(fixture.store.clone()).start_synthetic(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("complete"),
                || Some(false),
                |_| Err("runtime pin changed".into()),
                &CancellationToken::default(),
            ),
            Err(InitializationCoordinatorError::Prefix(PrefixError::Blocked))
        ));
    }

    #[test]
    fn final_pre_spawn_revalidation_records_interruption_without_spawning() {
        let fixture = Fixture::new("coordinator-recovery-changed-before-spawn");
        let mut recovery_reads = 0;
        let result = InitializationCoordinator::new(fixture.store.clone())
            .start_synthetic_with_adapter(
                &fixture.plan,
                &fixture.approval,
                fixture.authorization("complete"),
                InitializationStartHooks::new(
                    move || {
                        recovery_reads += 1;
                        Some(recovery_reads != 1)
                    },
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                &FailingExecutor,
            );
        assert!(matches!(
            result,
            Err(InitializationCoordinatorError::PreSpawnBlocked {
                blocker: PreSpawnBlocker::RecoveryStateNotClear,
                durable_state: SpawnFailureState::Interrupted { .. }
            })
        ));
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert_eq!(record.lifecycle, PrefixLifecycleState::RecoveryRequired);
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Interrupted
        );

        let stale_runtime = Fixture::new("coordinator-runtime-changed-before-spawn");
        let runtime_checks = AtomicUsize::new(0);
        let result = InitializationCoordinator::new(stale_runtime.store.clone())
            .start_synthetic_with_adapter(
                &stale_runtime.plan,
                &stale_runtime.approval,
                stale_runtime.authorization("complete"),
                InitializationStartHooks::new(
                    || Some(false),
                    move |_| {
                        if runtime_checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                            Ok(())
                        } else {
                            Err("runtime evidence changed".into())
                        }
                    },
                    &CancellationToken::default(),
                ),
                &FailingExecutor,
            );
        assert!(
            matches!(
                result,
                Err(InitializationCoordinatorError::PreSpawnBlocked {
                    blocker: PreSpawnBlocker::RuntimeOrPrefixChanged,
                    durable_state: SpawnFailureState::Interrupted { .. }
                })
            ),
            "unexpected final prefix revalidation result: {result:?}"
        );
        let record = stale_runtime
            .store
            .show(&stale_runtime.id, |_| Ok(()))
            .unwrap()
            .record;
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Interrupted
        );

        let replaced_prefix = Fixture::new("coordinator-prefix-changed-before-spawn");
        let prefix_path = replaced_prefix.store.target_path(&replaced_prefix.id);
        let displaced = replaced_prefix
            ._temporary
            .path()
            .join("prefix-before-final-revalidation");
        let displaced_for_callback = displaced.clone();
        let mut validation_calls = 0;
        let result = InitializationCoordinator::new(replaced_prefix.store.clone())
            .start_synthetic_with_adapter(
                &replaced_prefix.plan,
                &replaced_prefix.approval,
                replaced_prefix.authorization("complete"),
                InitializationStartHooks::new(
                    || Some(false),
                    move |_| {
                        validation_calls += 1;
                        if validation_calls == 2 {
                            fs::rename(&prefix_path, &displaced_for_callback).unwrap();
                            fs::create_dir(&prefix_path).unwrap();
                            fs::set_permissions(&prefix_path, fs::Permissions::from_mode(0o700))
                                .unwrap();
                        }
                        Ok(())
                    },
                    &CancellationToken::default(),
                ),
                &FailingExecutor,
            );
        assert!(
            matches!(
                result,
                Err(InitializationCoordinatorError::PreSpawnBlocked {
                    blocker: PreSpawnBlocker::RuntimeOrPrefixChanged,
                    durable_state: SpawnFailureState::Unresolved { .. }
                })
            ),
            "unexpected final prefix revalidation result: {result:?}"
        );
        assert!(displaced.exists());
        let record = replaced_prefix
            .store
            .show(&replaced_prefix.id, |_| Ok(()))
            .unwrap()
            .record;
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Starting
        );

        let revision_race = Fixture::new("coordinator-registry-changed-before-spawn");
        let race_store = revision_race.store.clone();
        let race_scope = revision_race.plan.scope.clone();
        let race_runtime = revision_race.runtime.clone();
        let race_revision = revision_race.plan.registry_revision + 1;
        let mut recovery_reads = 0;
        let result = InitializationCoordinator::new(revision_race.store.clone())
            .start_synthetic_with_adapter(
                &revision_race.plan,
                &revision_race.approval,
                revision_race.authorization("complete"),
                InitializationStartHooks::new(
                    move || {
                        recovery_reads += 1;
                        if recovery_reads == 2 {
                            race_store
                                .create(
                                    PrefixId::new("coordinator-final-revision-race").unwrap(),
                                    race_scope.clone(),
                                    race_runtime.clone(),
                                    race_revision,
                                )
                                .unwrap();
                        }
                        Some(false)
                    },
                    |_| Ok(()),
                    &CancellationToken::default(),
                ),
                &FailingExecutor,
            );
        assert!(matches!(
            result,
            Err(InitializationCoordinatorError::PreSpawnBlocked {
                blocker: PreSpawnBlocker::RegistryChanged,
                durable_state: SpawnFailureState::Unresolved { .. }
            })
        ));
        let record = revision_race
            .store
            .show(&revision_race.id, |_| Ok(()))
            .unwrap()
            .record;
        assert_eq!(
            record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Starting
        );
    }

    #[test]
    fn unresolved_intent_is_not_retried_after_owner_drop() {
        let fixture = Fixture::new("coordinator-restart-block");
        let attempt = running(start(&fixture, "partial-sleep"));
        drop(attempt);
        let inspection = fixture.store.show(&fixture.id, |_| Ok(())).unwrap();
        assert_eq!(
            inspection.observed_lifecycle,
            PrefixLifecycleState::RecoveryRequired
        );
        assert_eq!(
            inspection.record.initialization_intent.unwrap().phase,
            PrefixInitializationPhase::Running
        );
        assert!(
            fixture
                .store
                .target_path(&fixture.id)
                .join("drive_c")
                .exists()
        );
        assert!(matches!(
            fixture.store.plan_initialization(
                &fixture.id,
                fixture.plan.registry_revision,
                &fixture.runtime,
                Some(false),
                |_| Ok(()),
            ),
            Err(PrefixError::RevisionConflict) | Err(PrefixError::Blocked)
        ));
    }

    #[test]
    fn opened_executable_remains_bound_after_path_replacement() {
        let fixture = Fixture::new("coordinator-exe-replaced");
        let authorization = fixture.authorization("complete");
        fs::rename(
            &fixture.child_path,
            fixture.child_root.join("wine-fixture-held"),
        )
        .unwrap();
        fs::write(&fixture.child_path, b"replacement executable").unwrap();
        fs::set_permissions(&fixture.child_path, fs::Permissions::from_mode(0o700)).unwrap();
        let coordinator = InitializationCoordinator::new(fixture.store.clone());
        let start = coordinator
            .start_synthetic(
                &fixture.plan,
                &fixture.approval,
                authorization,
                || Some(false),
                |_| Ok(()),
                &CancellationToken::default(),
            )
            .unwrap();
        let mut attempt = running(start);
        assert!(matches!(
            attempt.observe(Duration::from_secs(5)).unwrap(),
            InitializationObservation::Initialized { .. }
        ));
    }

    #[test]
    fn duplicate_concurrent_attempts_publish_only_one_intent() {
        let fixture = Fixture::new("coordinator-concurrent");
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = fixture.store.clone();
            let plan = fixture.plan.clone();
            let approval = fixture.approval.clone();
            let runtime = fixture.runtime.clone();
            let runtime_root = fixture.child_root.clone();
            let prefix_path = store.target_path(&fixture.id);
            let id = fixture.id.clone();
            let child_path = fixture.child_path.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let auth = synthetic_authorization_for_paths(
                    &plan,
                    &runtime_root,
                    &child_path,
                    &prefix_path,
                    &id,
                    "sleep",
                );
                barrier.wait();
                InitializationCoordinator::new(store).start_synthetic(
                    &plan,
                    &approval,
                    auth,
                    || Some(false),
                    move |association| {
                        if association == &runtime {
                            Ok(())
                        } else {
                            Err("runtime association mismatch".into())
                        }
                    },
                    &CancellationToken::default(),
                )
            }));
        }
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = results.into_iter().find_map(Result::ok).unwrap();
        let attempt = running(winner);
        drop(attempt);
        let record = fixture.store.show(&fixture.id, |_| Ok(())).unwrap().record;
        assert!(record.initialization_intent.is_some());
    }

    fn synthetic_authorization_for_paths(
        plan: &PrefixInitializationPlan,
        runtime_root_path: &std::path::Path,
        child_path: &std::path::Path,
        prefix_path: &std::path::Path,
        prefix_id: &PrefixId,
        mode: &str,
    ) -> InitializationExecutionAuthorization {
        let uid = current_uid();
        let runtime_metadata = fs::metadata(runtime_root_path).unwrap();
        let runtime_root = TrustedRoot::open(
            runtime_root_path,
            DirectoryIdentity {
                device: runtime_metadata.dev(),
                inode: runtime_metadata.ino(),
                owner_uid: runtime_metadata.uid(),
            },
            uid,
        )
        .unwrap();
        let relative = child_path.file_name().unwrap();
        let initial = BoundExecutable::open(&runtime_root, Path::new(relative), uid).unwrap();
        let identity = initial.identity();
        let digest = initial.sha256().unwrap();
        let executable = BoundExecutable::open_matching(
            &runtime_root,
            Path::new(relative),
            uid,
            ExpectedExecutableIdentity {
                device: identity.device,
                inode: identity.inode,
                size: identity.size,
            },
            &digest,
        )
        .unwrap();
        let prefix_metadata = fs::metadata(prefix_path).unwrap();
        let prefix_root = TrustedRoot::open(
            prefix_path,
            DirectoryIdentity {
                device: prefix_metadata.dev(),
                inode: prefix_metadata.ino(),
                owner_uid: prefix_metadata.uid(),
            },
            uid,
        )
        .unwrap();
        let executable_identity = executable.identity();
        InitializationExecutionAuthorization {
            binding: ExecutionBinding {
                plan_id: plan.plan_id.clone(),
                runtime_installation_id: "wine-synthetic-fixture".into(),
                runtime_generation: 1,
                runtime_pin_id: plan.runtime_pin_id.clone(),
                runtime_pin_revision: plan.runtime_pin_revision,
                runtime_reference_sha256: plan.runtime_reference_sha256.clone(),
                runtime_ownership_record_sha256: "c".repeat(64),
                runtime_artifact_sha256: "a".repeat(64),
                runtime_root: runtime_root.identity(),
                executable_identity: ExpectedExecutableIdentity {
                    device: executable_identity.device,
                    inode: executable_identity.inode,
                    size: executable_identity.size,
                },
                executable_sha256: digest,
                prefix_id: prefix_id.as_str().into(),
                prefix_registry_revision: plan.registry_revision,
                prefix_generation: plan.prefix_generation,
                prefix_identity: plan.filesystem_identity.unwrap(),
            },
            executable,
            working_directory: prefix_root,
            wine_prefix: prefix_path.to_path_buf(),
            arguments: vec!["synthetic-wineboot".into(), mode.into()],
            environment: vec![("WINEPREFIX".into(), prefix_path.as_os_str().to_owned())],
            evidence_source: InitializationEvidenceSource::SyntheticNativeFixture,
            source: AuthorizationSource::SyntheticFixture,
        }
    }
}
