use endfield_unix_sessions::{
    ExitOutcome, MonitorFailure, ProcessObservation, SessionError, SessionState,
};

#[test]
fn lifecycle_accepts_only_declared_single_direction_transitions() {
    for (from, to) in [
        (SessionState::Created, SessionState::Starting),
        (SessionState::Starting, SessionState::Running),
        (SessionState::Starting, SessionState::Exited),
        (SessionState::Starting, SessionState::Failed),
        (SessionState::Running, SessionState::Exited),
        (SessionState::Running, SessionState::Failed),
        (SessionState::Running, SessionState::Stopping),
        (SessionState::Stopping, SessionState::Stopped),
        (SessionState::Stopping, SessionState::Failed),
    ] {
        assert_eq!(from.transition(to), Ok(to));
    }
    for terminal in [
        SessionState::Exited,
        SessionState::Failed,
        SessionState::Stopped,
    ] {
        assert!(terminal.is_terminal());
        assert_eq!(
            terminal.transition(SessionState::Running),
            Err(SessionError::InvalidTransition)
        );
        assert_eq!(
            terminal.transition(terminal),
            Err(SessionError::InvalidTransition)
        );
    }
    assert_eq!(
        SessionState::Created.transition(SessionState::Running),
        Err(SessionError::InvalidTransition)
    );
    assert_eq!(
        SessionState::Running.transition(SessionState::Stopped),
        Err(SessionError::InvalidTransition)
    );
}

#[test]
fn monitor_outcomes_are_explicit_and_terminal_events_cannot_complete_twice() {
    let starting = SessionState::Starting;
    assert_eq!(
        starting.apply_observation(ProcessObservation::Running),
        Ok(SessionState::Running)
    );
    assert_eq!(
        starting.apply_observation(ProcessObservation::Exited(ExitOutcome::Exited { code: 0 })),
        Ok(SessionState::Exited),
        "exit during startup is recorded without fabricating Running"
    );
    assert_eq!(
        SessionState::Running.apply_observation(ProcessObservation::MonitorFailed(
            MonitorFailure::WaitFailed
        )),
        Ok(SessionState::Failed),
        "monitor failure is not process exit evidence"
    );
    assert_eq!(
        SessionState::Exited
            .apply_observation(ProcessObservation::Exited(ExitOutcome::Exited { code: 0 })),
        Err(SessionError::InvalidTransition)
    );
    for outcome in [
        ExitOutcome::Exited { code: 0 },
        ExitOutcome::Exited { code: 7 },
        ExitOutcome::Signaled { signal: 15 },
        ExitOutcome::StatusUnavailable,
    ] {
        assert_eq!(
            SessionState::Running.apply_observation(ProcessObservation::Exited(outcome)),
            Ok(SessionState::Exited)
        );
    }
    assert_ne!(
        ExitOutcome::Exited { code: 0 },
        ExitOutcome::Exited { code: 9 }
    );
    assert_eq!(
        ExitOutcome::Signaled { signal: 15 },
        ExitOutcome::Signaled { signal: 15 }
    );
}
