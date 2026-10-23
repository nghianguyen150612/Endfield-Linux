#![deny(unsafe_code)]

mod raw;
mod safe;

pub use safe::{
    Availability, Capability, CapabilityId, Error, Goal, GoalPreference, Intensity, Measurement,
    Optimizer, PlanSet, PlanView, Platform, Request, Unit, abi_version, validate_abi_compatibility,
};

/// The ABI version implemented by this Rust wrapper.
pub const ABI_VERSION: u32 = 0x0001_0000;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn rust_layout_matches_the_published_64_bit_c_abi() {
        if cfg!(target_pointer_width = "64") {
            assert_eq!(size_of::<raw::Capability>(), 56);
            assert_eq!(align_of::<raw::Capability>(), 8);
            assert_eq!(size_of::<raw::GoalRecord>(), 32);
            assert_eq!(align_of::<raw::GoalRecord>(), 8);
            assert_eq!(size_of::<raw::RequestRecord>(), 48);
            assert_eq!(align_of::<raw::RequestRecord>(), 8);
            assert_eq!(size_of::<raw::ActionViewRecord>(), 32);
            assert_eq!(align_of::<raw::ActionViewRecord>(), 8);
            assert_eq!(size_of::<raw::ImpactViewRecord>(), 16);
            assert_eq!(align_of::<raw::ImpactViewRecord>(), 4);
            assert_eq!(size_of::<raw::PlanViewRecord>(), 88);
            assert_eq!(align_of::<raw::PlanViewRecord>(), 8);
            assert_eq!(offset_of!(raw::Capability, value_u64), 32);
            assert_eq!(offset_of!(raw::Capability, provenance_utf8), 40);
            assert_eq!(offset_of!(raw::GoalRecord, value_primary), 16);
            assert_eq!(offset_of!(raw::RequestRecord, capabilities), 16);
            assert_eq!(offset_of!(raw::RequestRecord, goals), 32);
            assert_eq!(offset_of!(raw::PlanViewRecord, actions), 24);
            assert_eq!(offset_of!(raw::PlanViewRecord, evidence_ids), 56);
            assert_eq!(offset_of!(raw::PlanViewRecord, uncertainty), 80);
        }
    }
}
