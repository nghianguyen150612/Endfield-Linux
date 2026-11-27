//! Fail-closed lifecycle contract for a future PID-namespace supervisor.
//!
//! This module does not launch processes or establish containment. In particular,
//! receipt of a protocol message is not evidence of kernel-backed quiescence.
//! Production execution must remain blocked until a private namespace-init worker,
//! descriptor handoff and authenticated channel are implemented and qualified.

/// The lifecycle observed by the owner of one execution attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prepared,
    Started,
    Active,
    InitialExited,
    Cancelling,
    Quiescent,
    RecoveryRequired,
    Closed,
}

/// Bounded messages for the planned private channel. No message contains a path,
/// command, environment, or host PID. Attempt identity is scoped by the channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    Start,
    InitialExited,
    Quiescent,
    Cancel,
    Close,
    Failure,
}

/// Reject malformed, oversized and unknown protocol messages before state changes.
impl Operation {
    pub fn decode(message: &[u8]) -> Option<Self> {
        match message {
            [1] => Some(Self::Initialize),
            [2] => Some(Self::Start),
            [3] => Some(Self::InitialExited),
            [4] => Some(Self::Quiescent),
            [5] => Some(Self::Cancel),
            [6] => Some(Self::Close),
            [7] => Some(Self::Failure),
            _ => None,
        }
    }
}

/// Contract state only; it is not a containment capability. A future worker
/// must validate the channel, attempt, scope and kernel evidence separately.
#[derive(Debug)]
pub struct Lifecycle {
    phase: Phase,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            phase: Phase::Prepared,
        }
    }
}

impl Lifecycle {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Applies a validated event. Invalid transitions leave the state unchanged.
    /// Quiescent is deliberately not accepted here: only a future worker with
    /// kernel-backed evidence may establish it.
    pub fn apply(&mut self, operation: Operation) -> Result<Phase, InvalidTransition> {
        use Operation as O;
        use Phase as P;
        let next = match (self.phase, operation) {
            (P::Prepared, O::Initialize) => P::Started,
            (P::Started, O::Start) => P::Active,
            (P::Active, O::InitialExited) => P::InitialExited,
            (P::Active | P::InitialExited, O::Cancel) => P::Cancelling,
            (
                P::Prepared | P::Started | P::Active | P::InitialExited | P::Cancelling,
                O::Failure,
            ) => P::RecoveryRequired,
            (P::Prepared | P::Started, O::Close) => P::Closed,
            _ => return Err(InvalidTransition),
        };
        self.phase = next;
        Ok(next)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTransition;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_exit_does_not_prove_quiescence() {
        let mut state = Lifecycle::default();
        assert_eq!(state.apply(Operation::Initialize), Ok(Phase::Started));
        assert_eq!(state.apply(Operation::Start), Ok(Phase::Active));
        assert_eq!(
            state.apply(Operation::InitialExited),
            Ok(Phase::InitialExited)
        );
        assert_eq!(state.apply(Operation::Quiescent), Err(InvalidTransition));
        assert_eq!(state.apply(Operation::Close), Err(InvalidTransition));
        assert_eq!(state.phase(), Phase::InitialExited);
    }

    #[test]
    fn duplicate_start_and_malformed_messages_are_rejected() {
        let mut state = Lifecycle::default();
        state.apply(Operation::Initialize).unwrap();
        state.apply(Operation::Start).unwrap();
        assert_eq!(state.apply(Operation::Start), Err(InvalidTransition));
        for message in [&[][..], &[0], &[8], &[1, 2]] {
            assert_eq!(Operation::decode(message), None);
        }
        assert_eq!(Operation::decode(&[5]), Some(Operation::Cancel));
        state.apply(Operation::Cancel).unwrap();
        state.apply(Operation::Failure).unwrap();
        assert_eq!(state.phase(), Phase::RecoveryRequired);
        assert_eq!(state.apply(Operation::Close), Err(InvalidTransition));
    }
}
