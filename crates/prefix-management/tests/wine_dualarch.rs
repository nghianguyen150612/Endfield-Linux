use endfield_unix_prefix_management::{wine_candidate::CandidateProfile, wine_native_abi};
use endfield_unix_runtime_audit::Entry;
use endfield_unix_runtime_verification::sha256;
fn fixture() -> (CandidateProfile, Vec<Entry>) {
    (
        serde_json::from_str(include_str!(
            "../../../docs/artifact-evidence/winehq-dualarch-profile.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "../../../docs/artifact-evidence/winehq-dualarch-transformed-inventory.json"
        ))
        .unwrap(),
    )
}
#[test]
fn complete_derived_profile_binds_inventory_and_transform_but_cannot_authorize() {
    let (profile, entries) = fixture();
    let raw = include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-packaging.json");
    profile
        .validate_derived_evidence(
            &entries,
            raw,
            profile.derived_artifact_sha256.as_ref().unwrap(),
            1,
        )
        .unwrap();
    assert!(profile.missing_companion_packages.is_empty());
    assert!(
        profile
            .validate_derived_evidence(
                &entries,
                b"forged",
                profile.derived_artifact_sha256.as_ref().unwrap(),
                1
            )
            .is_err()
    );
    assert!(
        profile
            .validate_derived_evidence(&entries, raw, &"0".repeat(64), 1)
            .is_err()
    );
    assert!(
        profile
            .validate_derived_evidence(
                &entries,
                raw,
                profile.derived_artifact_sha256.as_ref().unwrap(),
                2
            )
            .is_err()
    );
}
fn preloader32() -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&3u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&96u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&52u32.to_le_bytes());
    bytes[40..42].copy_from_slice(&52u16.to_le_bytes());
    bytes[42..44].copy_from_slice(&32u16.to_le_bytes());
    bytes[44..46].copy_from_slice(&1u16.to_le_bytes());
    bytes[52..56].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&128u32.to_le_bytes());
    bytes[72..76].copy_from_slice(&128u32.to_le_bytes());
    bytes[76..80].copy_from_slice(&5u32.to_le_bytes());
    bytes
}
#[test]
fn elf32_candidate_parser_does_not_expand_executor_architecture_policy() {
    let bytes = preloader32();
    assert!(endfield_unix_game_launch::elf_dependencies::inspect(&bytes).is_err());
    assert_eq!(
        endfield_unix_game_launch::elf_dependencies::inspect_candidate(&bytes)
            .unwrap()
            .class,
        32
    );
    assert!(wine_native_abi::preloader_shape(&bytes));
    for offset in [24, 76] {
        let mut bad = bytes.clone();
        bad[offset] = if offset == 24 { 0 } else { 7 };
        assert!(!wine_native_abi::preloader_shape(&bad));
    }
}
#[test]
fn candidate_preloader_shape_never_becomes_publisher_authentication() {
    let (mut profile, mut entries) = fixture();
    let bytes = preloader32();
    let hash = sha256(&bytes);
    let role = profile
        .roles
        .iter_mut()
        .find(|r| r.role == "preloader_i386")
        .unwrap();
    role.sha256 = hash.clone();
    let path = role.path.clone();
    let entry = entries.iter_mut().find(|e| e.path == path).unwrap();
    entry.kind = endfield_unix_runtime_audit::EntryKind::File {
        size: bytes.len() as u64,
        sha256: hash,
        executable: true,
        setid: false,
    };
    profile.inventory_sha256 = sha256(&serde_json::to_vec(&entries).unwrap());
    let result =
        wine_native_abi::assess_candidate_preloader(&profile, &entries, "preloader_i386", &bytes);
    assert!(result.structurally_supported);
    assert!(!result.execution_authorized);
    assert_eq!(
        result.state,
        wine_native_abi::PreloaderAbiState::PreloaderAbiUnverified
    );
    assert!(
        !wine_native_abi::assess_candidate_preloader(&profile, &entries, "launcher_i386", &bytes)
            .structurally_supported
    );
    let mut changed = bytes;
    changed[96] = 1;
    assert!(
        !wine_native_abi::assess_candidate_preloader(
            &profile,
            &entries,
            "preloader_i386",
            &changed
        )
        .role_inventory_bound
    );
}

#[test]
fn changed_source_ownership_or_provenance_record_is_not_accepted() {
    let (profile, _) = fixture();
    let source =
        include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-source-inventory.json");
    let transformation =
        include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-packaging.json");
    profile
        .validate_source_evidence(source, transformation)
        .unwrap();
    let mut changed = source.to_vec();
    changed.push(b' ');
    assert!(
        profile
            .validate_source_evidence(&changed, transformation)
            .is_err()
    );
}
