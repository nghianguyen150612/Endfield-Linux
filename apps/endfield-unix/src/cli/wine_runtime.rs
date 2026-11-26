//! Read-only Wine qualification; never creates storage or starts a runtime.
use super::compatibility::CompatibilityError;
use endfield_unix_prefix_management::{
    authentic_qualification::{QualificationRequest, RecordedQualificationStatus},
    wine_qualification::{QualificationState, WineRuntimeQualification, qualify_managed_wine},
};
use endfield_unix_runtime_selection as pins;

pub(crate) fn collect(
    profile: Option<String>,
) -> Result<WineRuntimeQualification, CompatibilityError> {
    let profile = profile
        .map(pins::ProfileKey::new)
        .transpose()
        .map_err(|_| CompatibilityError::InvalidProfile)?;
    let Ok(paths) = endfield_unix_storage::AppPaths::discover() else {
        return Ok(WineRuntimeQualification::unavailable(
            "xdg_paths_unavailable",
        ));
    };
    let Ok(machine) = pins::current_machine_id() else {
        return Ok(WineRuntimeQualification::unavailable(
            "machine_identity_unavailable",
        ));
    };
    let Ok(registry) = pins::PinStore::for_app(&paths).load_read_only() else {
        return Ok(WineRuntimeQualification::unavailable(
            "pin_registry_unavailable",
        ));
    };
    let selection = pins::resolve(&registry, &machine, profile.as_ref(), None, |_| Ok(()));
    let (Some(id), Some(runtime)) = (selection.pin_id, selection.runtime) else {
        return Ok(WineRuntimeQualification::unavailable(
            "managed_wine_not_selected",
        ));
    };
    let mut report = qualify_managed_wine(&paths, &id, selection.registry_revision, &runtime);
    if let (Some(root), Some(digest)) = (
        std::env::var_os("ENDFIELD_AUTHENTIC_WINE_TEST_ROOT"),
        report.artifact_sha256.clone(),
    ) {
        let recorded = QualificationRequest {
            source_paths: paths,
            pin_id: id,
            pin_revision: selection.registry_revision,
            artifact_sha256: digest,
            test_root: root.into(),
        }
        .recorded_status();
        report.recorded_qualification = Some(recorded);
        if recorded != RecordedQualificationStatus::NoAttempt {
            report.authentic_qualification = QualificationState::Unknown;
            report
                .blockers
                .retain(|b| b != "authentic_qualification_not_run");
            report
                .blockers
                .push("recorded_qualification_requires_review".into());
        }
    }
    Ok(report)
}
pub(crate) fn summary(report: &WineRuntimeQualification) -> String {
    format!(
        "Managed Wine: {}. Pin {} revision {}. Manifest authority {:?}; signature/provenance {:?}. P025 {:?}; P026 {:?}. Dependency observations: {}. Artifact profile {:?}; profile blocker {:?}; host ABI {:?}; i386 host ABI {:?}; image provider {}; image digest {:?}; mounted view {:?}; isolated readiness {:?}; recorded outcome {:?}. Helper inventory {:?}; invocation {:?}; descendant completion {:?}; authentic qualification {:?}. Initialization {:?}; approval required: {}. Blockers: {}. Game launch remains unsupported.",
        report
            .runtime_installation_id
            .as_deref()
            .unwrap_or("unavailable"),
        report.pin_id.as_deref().unwrap_or("none"),
        report.pin_revision,
        report.manifest_authority_scope,
        report.manifest_signature_state,
        report.ownership,
        report.integrity,
        report.dependencies.len(),
        report.artifact_profile_id,
        report.artifact_profile_blocker,
        report.host_abi.state,
        report.host_i386_abi,
        report.runtime_image_provider,
        report.runtime_image_sha256,
        report.mounted_view_eligibility,
        report.isolated_qualification_readiness,
        report.recorded_qualification,
        report.helper_inventory,
        report.invocation_contract,
        report.descendant_completion,
        report.authentic_qualification,
        report.production_initialization,
        report.approval_required,
        report.blockers.join(", ")
    )
}
pub(crate) fn run(
    profile: Option<String>,
    json: bool,
    output: &mut dyn std::io::Write,
) -> Result<(), CompatibilityError> {
    let report = collect(profile)?;
    let bytes = if json {
        serde_json::to_vec_pretty(&report).map_err(|_| CompatibilityError::Serialization)?
    } else {
        summary(&report).into_bytes()
    };
    if bytes.len() > 1024 * 1024 {
        return Err(CompatibilityError::OutputLimit);
    }
    output
        .write_all(&bytes)
        .map_err(CompatibilityError::Output)?;
    output.write_all(b"\n").map_err(CompatibilityError::Output)
}
