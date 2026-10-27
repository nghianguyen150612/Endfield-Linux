mod common;

use endfield_unix_runtime::{
    ContractError, ProcessArchitecture, RuntimeFamily, RuntimeIdentity, RuntimeLineage,
    RuntimeOwnership, RuntimePlatform, RuntimeRequirements, RuntimeVersion,
};

#[test]
fn models_wine_proton_and_verified_derivative_lineage() {
    let wine = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "wine-provider",
        "wine-10.0",
        RuntimeOwnership::ExternalUserManaged,
        None,
        None,
    );
    let proton = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::ValveProton,
        "steam-provider",
        "Proton-Experimental",
        RuntimeOwnership::OtherLauncherManaged(
            endfield_unix_runtime::ProviderId::new("steam").expect("provider id"),
        ),
        None,
        None,
    );
    let ge = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::GeProton,
        "ge-provider",
        "GE-Proton10-26",
        RuntimeOwnership::ExternalUserManaged,
        None,
        None,
    );
    let dw = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::DwProton,
        "dw-provider",
        "DWProton-example-build",
        RuntimeOwnership::ExternalUserManaged,
        None,
        None,
    );

    assert_eq!(wine.family(), RuntimeFamily::Wine);
    assert_eq!(proton.family(), RuntimeFamily::Proton);
    assert_eq!(ge.lineage(), &RuntimeLineage::GeProton);
    assert_eq!(dw.lineage(), &RuntimeLineage::DwProton);
    assert_ne!(wine, proton);
}

#[test]
fn unknown_family_and_provider_remain_representable() {
    let identity = common::runtime_identity(
        RuntimeFamily::Unknown,
        RuntimeLineage::Unknown,
        "unrecognized-provider",
        "vendor-private-build-label",
        RuntimeOwnership::Unknown,
        None,
        None,
    );
    assert_eq!(identity.family(), RuntimeFamily::Unknown);
    assert_eq!(identity.provider_id().as_str(), "unrecognized-provider");
    assert_eq!(identity.version().as_str(), "vendor-private-build-label");
}

#[test]
fn rejects_empty_version_and_family_lineage_mismatch() {
    assert!(matches!(
        RuntimeVersion::new(" \t"),
        Err(ContractError::InvalidText { .. })
    ));
    let result = RuntimeIdentity::new(
        RuntimeFamily::Wine,
        RuntimeLineage::ValveProton,
        endfield_unix_runtime::ProviderId::new("wine-provider").expect("provider"),
        RuntimeVersion::new("wine-10.0").expect("version"),
        None,
        RuntimeRequirements::default(),
        None,
        None,
        RuntimeOwnership::ExternalUserManaged,
    );
    assert!(matches!(result, Err(ContractError::InvalidValue { .. })));
}

#[test]
fn preserves_non_semver_version_text_without_coercion() {
    for value in [
        "wine-10.0",
        "GE-Proton10-26",
        "Proton-Experimental",
        "DWProton-example-build",
    ] {
        assert_eq!(RuntimeVersion::new(value).expect("version").as_str(), value);
    }
}

#[test]
fn equal_version_labels_do_not_prove_equal_build_bytes() {
    let left = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        Some(common::sha256('a')),
        None,
    );
    let right = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        Some(common::sha256('b')),
        None,
    );
    let without_hash = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        None,
        None,
    );

    assert!(!left.same_verified_build(&right));
    assert!(!left.same_verified_build(&without_hash));
    assert!(left.same_verified_build(&left.clone()));
}

#[test]
fn identities_are_exactly_comparable_and_family_sensitive() {
    let base = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::GeProton,
        "provider",
        "GE-Proton10-26",
        RuntimeOwnership::ExternalUserManaged,
        Some(common::sha256('c')),
        None,
    );
    assert_eq!(base, base.clone());
    let wine = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "provider",
        "GE-Proton10-26",
        RuntimeOwnership::ExternalUserManaged,
        Some(common::sha256('c')),
        None,
    );
    assert!(!base.same_verified_build(&wine));
}

#[test]
fn reports_an_unsupported_process_architecture_as_a_static_mismatch() {
    let requirements =
        RuntimeRequirements::new([RuntimePlatform::Linux], [ProcessArchitecture::X86_64])
            .expect("requirements");
    assert_eq!(
        requirements.supports_architecture(ProcessArchitecture::Aarch64),
        Some(false)
    );
    assert_eq!(
        requirements.supports_architecture(ProcessArchitecture::Unknown),
        None
    );
}

#[test]
fn runtime_requirement_iterators_are_bounded_before_collection() {
    assert!(matches!(
        RuntimeRequirements::new(
            std::iter::repeat(RuntimePlatform::Linux),
            std::iter::empty(),
        ),
        Err(ContractError::MetadataTooLarge)
    ));
}

#[test]
fn rejects_an_invalid_identifier() {
    assert!(matches!(
        endfield_unix_runtime::ProviderId::new("provider/path"),
        Err(ContractError::InvalidIdentifier { .. })
    ));
}
