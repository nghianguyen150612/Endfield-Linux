use std::time::Duration;

use endfield_unixd::lifecycle::{LifecycleState, ShutdownCoordinator};
use endfield_unixd::session_service::SessionService;

#[test]
fn daemon_session_worker_is_ready_owned_and_completes_its_p009_token() {
    let coordinator = ShutdownCoordinator::new();
    let mut service =
        SessionService::start(coordinator.clone(), None).expect("session worker starts");
    let supervisor = service.supervisor().clone();
    assert!(supervisor.is_accepting().expect("registry is ready"));
    assert_eq!(coordinator.snapshot().unwrap().active_workers, 1);
    service
        .reject_new_registrations()
        .expect("shutdown closes registration first");
    assert!(!supervisor.is_accepting().unwrap());

    assert!(matches!(
        service.shutdown(Duration::ZERO),
        Err(endfield_unixd::errors::DaemonError::ShutdownTimeout)
    ));
    service
        .shutdown(Duration::from_secs(2))
        .expect("monitor acknowledges shutdown");
    service
        .shutdown(Duration::ZERO)
        .expect("repeated cleanup is idempotent");
    assert!(!supervisor.is_accepting().unwrap());
    assert_eq!(coordinator.snapshot().unwrap().active_workers, 0);
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Starting
    );
}
