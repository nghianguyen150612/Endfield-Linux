/// Kernel identity returned by the native running-kernel interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawKernelIdentity {
    pub system_name: String,
    pub release: String,
    pub machine: String,
}
