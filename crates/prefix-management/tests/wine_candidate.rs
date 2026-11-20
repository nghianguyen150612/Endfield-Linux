use endfield_unix_prefix_management::wine_candidate::CandidateProfile;
use endfield_unix_runtime_audit::Entry;

fn fixture() -> (CandidateProfile, Vec<Entry>) {
    let profile =
        include_str!("../../../docs/artifact-evidence/winehq-noble-10.0-candidate-profile.json");
    let inventory =
        include_str!("../../../docs/artifact-evidence/winehq-noble-10.0-candidate-inventory.json");
    (
        serde_json::from_str(profile).unwrap(),
        serde_json::from_str(inventory).unwrap(),
    )
}
#[test]
fn actual_package_roles_are_validated_without_granting_trust() {
    let (profile, inventory) = fixture();
    profile.validate(&inventory).unwrap();
    assert_eq!(
        profile.execution_blocker().to_string(),
        "native preloader pathname handoff does not preserve verified executable identity"
    );
    assert!(!profile.missing_companion_packages.is_empty());
}
#[test]
fn forged_roles_changed_inventory_and_downgrades_fail() {
    for change in 0..9 {
        let (mut profile, mut inventory) = fixture();
        match change {
            0 => profile.schema_version = 1,
            1 => profile.architecture = "i386".into(),
            2 => profile.roles[0].path = "../escape".into(),
            3 => profile.roles[0].sha256 = "0".repeat(64),
            4 => inventory.pop().map(|_| ()).unwrap(),
            5 => profile.roles.push(
                serde_json::from_value(serde_json::to_value(&profile.roles[0]).unwrap()).unwrap(),
            ),
            6 => profile.native_helper_roles.push("unverified_helper".into()),
            7 => profile.preloader_policy = "disabled".into(),
            _ => profile.invocation_semantics = "PATH_fallback".into(),
        }
        assert!(profile.validate(&inventory).is_err(), "change {change}");
    }
}
#[test]
fn source_build_cannot_impersonate_publisher() {
    let (profile, _) = fixture();
    let mut raw = serde_json::to_value(profile).unwrap();
    raw["provenance"] = "independently_authenticated_publisher".into();
    assert!(serde_json::from_value::<CandidateProfile>(raw).is_err());
}

#[test]
fn missing_roles_and_invalid_native_identities_remain_blocked() {
    let (mut profile, inventory) = fixture();
    profile.required_resource_roles.clear();
    assert!(profile.validate(&inventory).is_err());
    assert!(
        profile
            .validate_native_bytes("unknown", b"invalid")
            .is_err()
    );
    assert!(
        profile
            .validate_native_bytes("preloader", b"changed bytes")
            .is_err()
    );
}

#[test]
fn exact_hash_cannot_promote_wrong_native_architecture() {
    let (mut profile, _) = fixture();
    let mut bytes = std::fs::read("/proc/self/exe").unwrap();
    let mapped = profile
        .roles
        .iter_mut()
        .find(|r| r.role == "launcher")
        .unwrap();
    // A matching hash of an unsupported e_machine still fails architecture policy.
    bytes[18] = 3;
    bytes[19] = 0;
    mapped.sha256 = endfield_unix_runtime_verification::sha256(&bytes);
    assert!(profile.validate_native_bytes("launcher", &bytes).is_err());
}
