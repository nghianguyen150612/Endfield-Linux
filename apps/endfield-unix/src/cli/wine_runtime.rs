//! Read-only Wine qualification; never creates storage or starts a runtime.
use super::compatibility::CompatibilityError;
use endfield_unix_prefix_management::wine_qualification::{
    WineRuntimeQualification, qualify_managed_wine,
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
    Ok(qualify_managed_wine(
        &paths,
        &id,
        selection.registry_revision,
        &runtime,
    ))
}
pub(crate) fn summary(report: &WineRuntimeQualification) -> String {
    format!(
        "Managed Wine: {}. Pin {} revision {}. P025 {:?}; P026 {:?}. Dependency observations: {}. Helper inventory {:?}; invocation {:?}; descendant completion {:?}; authentic qualification {:?}. Initialization {:?}; approval required: {}. Blockers: {}. Game launch remains unsupported.",
        report
            .runtime_installation_id
            .as_deref()
            .unwrap_or("unavailable"),
        report.pin_id.as_deref().unwrap_or("none"),
        report.pin_revision,
        report.ownership,
        report.integrity,
        report.dependencies.len(),
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
