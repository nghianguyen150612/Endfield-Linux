use endfield_unix_domain::{Architecture, ValidationError};

/// Maps the architecture returned by the running kernel's `uname` interface.
/// This deliberately does not inspect `std::env::consts::ARCH`, which describes
/// the executable's build target rather than the running kernel.
pub fn runtime_architecture(machine: &str) -> Result<Architecture, ValidationError> {
    Architecture::parse(machine)
}
