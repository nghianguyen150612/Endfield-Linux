use optimizer_ffi::{
    ABI_VERSION, Availability, Capability, CapabilityId, Error, Goal, Intensity, Measurement,
    Optimizer, Platform, Request, Unit, abi_version, validate_abi_compatibility,
};

fn optimizer() -> Optimizer {
    match Optimizer::new() {
        Ok(optimizer) => optimizer,
        Err(error) => panic!("linked C17 ABI should be compatible: {error}"),
    }
}

fn empty_request() -> Request {
    Request::new(Platform::Linux, Intensity::L0)
}

#[test]
fn rust_reads_the_published_abi_version() {
    assert_eq!(abi_version(), 0x0001_0000);
    assert_eq!(abi_version(), ABI_VERSION);
    assert_eq!(optimizer().abi_version(), ABI_VERSION);
}

#[test]
fn rust_invokes_c17_and_receives_the_documented_empty_plan_set() {
    let plans = match optimizer().plan(&empty_request()) {
        Ok(plans) => plans,
        Err(error) => panic!("valid request should be accepted: {error}"),
    };
    assert_eq!(plans.count(), Ok(0));
}

#[test]
fn missing_capability_data_is_distinct_from_a_zero_measurement() {
    let unknown = match Capability::new(
        CapabilityId::Ram,
        0,
        Availability::Unknown,
        None,
        Some("memory provider was not available".to_owned()),
    ) {
        Ok(capability) => capability,
        Err(error) => panic!("unknown capability without a value is valid: {error}"),
    };
    let request = empty_request().with_capability(unknown);
    let plans = match optimizer().plan(&request) {
        Ok(plans) => plans,
        Err(error) => panic!("unknown capability data is valid input: {error}"),
    };
    assert_eq!(plans.count(), Ok(0));

    let zero = Capability::new(
        CapabilityId::Ram,
        0,
        Availability::Available,
        Some(Measurement {
            value: 0,
            unit: Unit::Bytes,
        }),
        None,
    );
    assert!(zero.is_ok());
}

#[test]
fn invalid_request_metadata_is_rejected_before_native_planning() {
    let request = empty_request()
        .with_goal(Goal::TargetFpsMilli(60_000))
        .with_goal(Goal::TargetFpsMilli(90_000));
    assert!(matches!(
        optimizer().plan(&request),
        Err(Error::InvalidArguments("duplicate goal identifier"))
    ));

    let invalid_resolution = empty_request().with_goal(Goal::TargetResolutionPixels {
        width: 1920,
        height: 0,
    });
    assert!(matches!(
        optimizer().plan(&invalid_resolution),
        Err(Error::InvalidArguments(_))
    ));
}

#[test]
fn incompatible_abi_major_is_reported_without_reinterpreting_structs() {
    assert!(matches!(
        validate_abi_compatibility(0x0002_0000, ABI_VERSION),
        Err(Error::UnsupportedAbi {
            requested: 0x0002_0000,
            available: ABI_VERSION,
        })
    ));
    assert!(validate_abi_compatibility(ABI_VERSION, 0x0001_0001).is_ok());
}

#[test]
fn unknown_capability_identifiers_are_rejected_instead_of_guessed() {
    assert!(matches!(
        CapabilityId::try_from(999),
        Err(Error::UnsupportedIdentifier {
            identifier: Some(999)
        })
    ));
}

#[test]
fn plan_handle_is_owned_and_empty_access_is_bounds_checked() {
    let plans = match optimizer().plan(&empty_request()) {
        Ok(plans) => plans,
        Err(error) => panic!("valid request should be accepted: {error}"),
    };
    assert_eq!(plans.count(), Ok(0));
    assert!(matches!(
        plans.get(0),
        Err(Error::IndexOutOfBounds { index: 0, count: 0 })
    ));
    drop(plans);
}

#[test]
fn repeated_calls_are_deterministic_and_destroy_native_handles() {
    let planner = optimizer();
    for _ in 0..128 {
        let request = empty_request().with_goal(Goal::TargetFpsMilli(0));
        let plans = match planner.plan(&request) {
            Ok(plans) => plans,
            Err(error) => panic!("valid repeated request should succeed: {error}"),
        };
        assert_eq!(plans.count(), Ok(0));
        drop(plans);
    }
}

#[test]
fn rust_owned_provenance_is_borrowed_only_for_the_native_call() {
    let capability = match Capability::new(
        CapabilityId::CpuTopology,
        1,
        Availability::Available,
        Some(Measurement {
            value: 8,
            unit: Unit::Count,
        }),
        Some("normalized local snapshot".to_owned()),
    ) {
        Ok(capability) => capability,
        Err(error) => panic!("valid provenance should be accepted: {error}"),
    };
    let request = empty_request().with_capability(capability);
    let plans = match optimizer().plan(&request) {
        Ok(plans) => plans,
        Err(error) => panic!("owned request data should be safely marshalled: {error}"),
    };
    assert_eq!(plans.count(), Ok(0));
}
