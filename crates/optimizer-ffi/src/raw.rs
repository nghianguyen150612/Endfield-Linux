#![allow(unsafe_code)]

use super::safe::Request;
use core::ptr::{self, NonNull};

pub(crate) const STATUS_SUCCESS: u32 = 0;
pub(crate) const STATUS_INVALID_ARGUMENT: u32 = 1;
pub(crate) const STATUS_UNSUPPORTED_ABI: u32 = 2;
pub(crate) const STATUS_UNSUPPORTED_IDENTIFIER: u32 = 3;
pub(crate) const STATUS_INCOMPATIBLE_CONSTRAINTS: u32 = 4;
pub(crate) const STATUS_RESOURCE_EXHAUSTION: u32 = 5;
pub(crate) const STATUS_INTERNAL_FAILURE: u32 = 6;

const ABI_VERSION: u32 = super::ABI_VERSION;
const MAX_CAPABILITIES: usize = 4096;
const MAX_GOALS: usize = 8;
const MAX_PROVENANCE_BYTES: usize = 4096;

#[repr(C)]
pub(crate) struct Capability {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) capability_id: u32,
    pub(crate) instance_id: u32,
    pub(crate) availability: u32,
    pub(crate) value_kind: u32,
    pub(crate) unit_id: u32,
    pub(crate) value_u64: u64,
    pub(crate) provenance_utf8: *const u8,
    pub(crate) provenance_len: u32,
}

#[repr(C)]
pub(crate) struct GoalRecord {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) goal_id: u32,
    pub(crate) present: u32,
    pub(crate) value_primary: u64,
    pub(crate) value_secondary: u64,
}

#[repr(C)]
pub(crate) struct RequestRecord {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) platform_id: u32,
    pub(crate) intensity: u32,
    pub(crate) capabilities: *const Capability,
    pub(crate) capability_count: u32,
    pub(crate) goals: *const GoalRecord,
    pub(crate) goal_count: u32,
}

#[repr(C)]
pub(crate) struct ActionViewRecord {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) action_id: u32,
    pub(crate) parameter_data: *const u8,
    pub(crate) parameter_len: u32,
}

#[repr(C)]
pub(crate) struct ImpactViewRecord {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) dimension_id: u32,
    pub(crate) impact_value: u32,
}

#[repr(C)]
pub(crate) struct PlanViewRecord {
    pub(crate) abi_version: u32,
    pub(crate) struct_size: u32,
    pub(crate) rank: u32,
    pub(crate) score: u32,
    pub(crate) action_count: u32,
    pub(crate) actions: *const ActionViewRecord,
    pub(crate) impact_count: u32,
    pub(crate) impacts: *const ImpactViewRecord,
    pub(crate) evidence_count: u32,
    pub(crate) evidence_ids: *const u64,
    pub(crate) constraint_count: u32,
    pub(crate) constraint_ids: *const u32,
    pub(crate) uncertainty: u32,
    pub(crate) rollback_requirement: u32,
}

#[repr(C)]
pub(crate) struct PlanSet {
    _opaque: [u8; 0],
}

// SAFETY: These declarations match the public C header. They remain private to this module, whose
// safe wrappers establish valid layouts, pointer/count pairs, and handle lifetimes before calls.
unsafe extern "C" {
    fn endfield_optimizer_abi_version() -> u32;
    fn endfield_optimizer_plan(
        request: *const RequestRecord,
        out_plan_set: *mut *mut PlanSet,
    ) -> u32;
    fn endfield_optimizer_plan_count(plan_set: *const PlanSet, out_count: *mut u32) -> u32;
    fn endfield_optimizer_plan_get(
        plan_set: *const PlanSet,
        index: u32,
        out_view: *mut PlanViewRecord,
    ) -> u32;
    fn endfield_optimizer_plan_set_destroy(plan_set: *mut PlanSet) -> u32;
}

pub(crate) fn abi_version() -> u32 {
    // SAFETY: The function has no inputs, performs no state changes, and returns a fixed ABI value.
    unsafe { endfield_optimizer_abi_version() }
}

pub(crate) fn plan(request: &Request) -> Result<NonNull<PlanSet>, u32> {
    if request.capabilities.len() > MAX_CAPABILITIES || request.goals.len() > MAX_GOALS {
        return Err(STATUS_INVALID_ARGUMENT);
    }

    let mut capabilities = Vec::new();
    capabilities
        .try_reserve_exact(request.capabilities.len())
        .map_err(|_| STATUS_RESOURCE_EXHAUSTION)?;
    for capability in &request.capabilities {
        let (value_kind, unit_id, value_u64) = match capability.value {
            Some(measurement) => (1, measurement.unit.as_raw(), measurement.value),
            None => (0, 0, 0),
        };
        let provenance = capability
            .provenance
            .as_ref()
            .map_or(&[][..], |source| source.as_bytes());
        if provenance.len() > MAX_PROVENANCE_BYTES || provenance.len() > u32::MAX as usize {
            return Err(STATUS_INVALID_ARGUMENT);
        }
        let provenance_utf8 = if provenance.is_empty() {
            ptr::null()
        } else {
            provenance.as_ptr()
        };
        capabilities.push(Capability {
            abi_version: ABI_VERSION,
            struct_size: size_of_u32::<Capability>(),
            capability_id: capability.id.as_raw(),
            instance_id: capability.instance_id,
            availability: capability.availability.as_raw(),
            value_kind,
            unit_id,
            value_u64,
            provenance_utf8,
            provenance_len: provenance.len() as u32,
        });
    }

    let mut goals = Vec::new();
    goals
        .try_reserve_exact(request.goals.len())
        .map_err(|_| STATUS_RESOURCE_EXHAUSTION)?;
    for goal in &request.goals {
        let (goal_id, value_primary, value_secondary) = goal.to_raw();
        goals.push(GoalRecord {
            abi_version: ABI_VERSION,
            struct_size: size_of_u32::<GoalRecord>(),
            goal_id,
            present: 1,
            value_primary,
            value_secondary,
        });
    }

    let capability_count =
        u32::try_from(capabilities.len()).map_err(|_| STATUS_INVALID_ARGUMENT)?;
    let goal_count = u32::try_from(goals.len()).map_err(|_| STATUS_INVALID_ARGUMENT)?;
    let capability_pointer = if capabilities.is_empty() {
        ptr::null()
    } else {
        capabilities.as_ptr()
    };
    let goal_pointer = if goals.is_empty() {
        ptr::null()
    } else {
        goals.as_ptr()
    };
    let raw_request = RequestRecord {
        abi_version: ABI_VERSION,
        struct_size: size_of_u32::<RequestRecord>(),
        platform_id: request.platform.as_raw(),
        intensity: request.intensity.as_raw(),
        capabilities: capability_pointer,
        capability_count,
        goals: goal_pointer,
        goal_count,
    };
    let mut out_plan_set = ptr::null_mut();

    // SAFETY: All nested arrays and provenance buffers point into live Rust-owned request data or
    // local vectors, counts match those buffers, and C borrows them only for this call. The C
    // implementation copies no caller pointer into the returned plan set.
    let status = unsafe { endfield_optimizer_plan(&raw_request, &mut out_plan_set) };
    if status != STATUS_SUCCESS {
        if let Some(handle) = NonNull::new(out_plan_set) {
            // SAFETY: The C contract says a failed planning call must not return a handle. If a
            // faulty implementation does, this is the only pointer it returned and C owns it.
            unsafe {
                let _ = endfield_optimizer_plan_set_destroy(handle.as_ptr());
            }
        }
        return Err(status);
    }
    NonNull::new(out_plan_set).ok_or(STATUS_INTERNAL_FAILURE)
}

pub(crate) fn plan_count(plan_set: NonNull<PlanSet>) -> Result<u32, u32> {
    let mut count = 0;
    // SAFETY: PlanSet handles can only be obtained from a successful call to C planning and are
    // kept alive by the caller for this accessor call.
    let status = unsafe { endfield_optimizer_plan_count(plan_set.as_ptr(), &mut count) };
    if status == STATUS_SUCCESS {
        Ok(count)
    } else {
        Err(status)
    }
}

pub(crate) struct PlanMetadata {
    pub(crate) rank: u32,
    pub(crate) score: u32,
    pub(crate) action_count: u32,
    pub(crate) impact_count: u32,
    pub(crate) evidence_count: u32,
    pub(crate) constraint_count: u32,
    pub(crate) uncertainty: u32,
    pub(crate) rollback_requirement: u32,
}

pub(crate) fn plan_get(plan_set: NonNull<PlanSet>, index: u32) -> Result<PlanMetadata, u32> {
    let mut view = PlanViewRecord {
        abi_version: ABI_VERSION,
        struct_size: size_of_u32::<PlanViewRecord>(),
        rank: 0,
        score: 0,
        action_count: 0,
        actions: ptr::null(),
        impact_count: 0,
        impacts: ptr::null(),
        evidence_count: 0,
        evidence_ids: ptr::null(),
        constraint_count: 0,
        constraint_ids: ptr::null(),
        uncertainty: 0,
        rollback_requirement: 0,
    };
    // SAFETY: plan_set is a live C-owned handle and view is correctly sized caller-owned storage.
    let status = unsafe { endfield_optimizer_plan_get(plan_set.as_ptr(), index, &mut view) };
    if status != STATUS_SUCCESS {
        return Err(status);
    }
    if view.abi_version >> 16 != ABI_VERSION >> 16
        || view.struct_size < size_of_u32::<PlanViewRecord>()
        || (view.action_count > 0 && view.actions.is_null())
        || (view.impact_count > 0 && view.impacts.is_null())
        || (view.evidence_count > 0 && view.evidence_ids.is_null())
        || (view.constraint_count > 0 && view.constraint_ids.is_null())
        || view.rank == 0
        || view.score > 1_000_000
        || view.uncertainty > 3
        || view.rollback_requirement > 2
    {
        return Err(STATUS_INTERNAL_FAILURE);
    }
    Ok(PlanMetadata {
        rank: view.rank,
        score: view.score,
        action_count: view.action_count,
        impact_count: view.impact_count,
        evidence_count: view.evidence_count,
        constraint_count: view.constraint_count,
        uncertainty: view.uncertainty,
        rollback_requirement: view.rollback_requirement,
    })
}

pub(crate) fn destroy(plan_set: NonNull<PlanSet>) -> u32 {
    // SAFETY: This consumes the unique handle owned by PlanSet::drop. No other safe wrapper can
    // access or destroy this handle after the drop begins.
    unsafe { endfield_optimizer_plan_set_destroy(plan_set.as_ptr()) }
}

fn size_of_u32<T>() -> u32 {
    core::mem::size_of::<T>() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn abi_struct_sizes_fit_the_versioned_size_field() {
        assert!(size_of::<Capability>() <= u32::MAX as usize);
        assert!(size_of::<GoalRecord>() <= u32::MAX as usize);
        assert!(size_of::<RequestRecord>() <= u32::MAX as usize);
        assert!(size_of::<ActionViewRecord>() <= u32::MAX as usize);
        assert!(size_of::<ImpactViewRecord>() <= u32::MAX as usize);
        assert!(size_of::<PlanViewRecord>() <= u32::MAX as usize);
    }

    #[test]
    fn request_arrays_use_natural_c_alignment() {
        assert!(align_of::<Capability>() >= align_of::<u64>());
        assert!(align_of::<GoalRecord>() >= align_of::<u64>());
        assert!(align_of::<RequestRecord>() >= align_of::<*const Capability>());
    }
}
