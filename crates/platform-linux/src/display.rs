//! Linux-specific display, output, session, and precise mode contracts.

use endfield_unix_domain::{DisplayConnection, DisplayInfo, InstanceId, Resolution};

pub const LINUX_DISPLAY_REPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_DISPLAY_OUTPUTS: usize = 64;
pub const MAX_DISPLAY_MODES: usize = 128;
pub const MAX_DRM_CONNECTORS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayProtocol {
    Wayland,
    X11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionTypeHint {
    Wayland,
    X11,
    Mixed,
    Headless,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifiedDisplayProtocol {
    Wayland,
    X11,
    Both,
    None,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderStatus {
    NotAdvertised,
    NotAttempted,
    Connected,
    RejectedRemote,
    InvalidEndpoint,
    AuthenticationFailed,
    PermissionDenied,
    TimedOut,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendErrorKind {
    Unavailable,
    AuthenticationFailed,
    PermissionDenied,
    TimedOut,
    Malformed,
    Unsupported,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendError {
    pub kind: BackendErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputInventoryStatus {
    Enumerated,
    ProtocolUnavailable,
    NotAttempted,
    Failed,
    Inconsistent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAssessment {
    /// Classification of bounded environment hints. It is not proof of a live session.
    pub hint: SessionTypeHint,
    pub xdg_session_type: Option<SessionTypeHint>,
    pub wayland_endpoint: ProviderStatus,
    pub x11_endpoint: ProviderStatus,
    pub verified_protocol: VerifiedDisplayProtocol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayProtocolFacts {
    /// Live connection result, independent of output-protocol support.
    pub status: ProviderStatus,
    pub inventory: OutputInventoryStatus,
    /// `None` means enumeration did not establish a count.
    pub output_count: Option<usize>,
    pub protocol_version: Option<(u32, u32)>,
    pub optional_logical_output_protocol: Option<bool>,
    pub screen_geometry: Option<(u32, u32)>,
    pub diagnostic: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositorAssessment {
    /// Sanitized desktop/session names advertised by the environment.
    pub advertised_desktops: Vec<String>,
    /// P014 does not infer identity from an environment variable or generic protocol globals.
    pub verified_identity: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOutputState {
    Active,
    ConnectedInactive,
    Disconnected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOutputSource {
    Wayland,
    X11RandR,
}

/// An exact rational number of refreshes per second, reduced to lowest terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExactRefreshRate {
    pub numerator_hz: u64,
    pub denominator: u64,
}

impl ExactRefreshRate {
    pub fn new(numerator_hz: u64, denominator: u64) -> Option<Self> {
        if numerator_hz == 0 || denominator == 0 {
            return None;
        }
        let divisor = gcd(numerator_hz, denominator);
        Some(Self {
            numerator_hz: numerator_hz / divisor,
            denominator: denominator / divisor,
        })
    }

    pub fn from_millihertz(millihertz: u32) -> Option<Self> {
        Self::new(u64::from(millihertz), 1_000)
    }

    pub fn exact_whole_hertz(self) -> Option<u64> {
        (self.numerator_hz % self.denominator == 0).then_some(self.numerator_hz / self.denominator)
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDisplayMode {
    pub width_pixels: u32,
    pub height_pixels: u32,
    pub refresh_rate: Option<ExactRefreshRate>,
    pub current: bool,
    pub preferred: bool,
    pub interlaced: Option<bool>,
    pub doublescan: Option<bool>,
}

impl LinuxDisplayMode {
    pub(crate) fn is_valid(&self) -> bool {
        self.width_pixels > 0
            && self.height_pixels > 0
            && self
                .refresh_rate
                .is_none_or(|rate| rate.numerator_hz > 0 && rate.denominator > 0)
    }

    pub(crate) fn resolution(&self) -> Option<Resolution> {
        Resolution::new(self.width_pixels, self.height_pixels).ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaleFactor {
    pub numerator: u32,
    pub denominator: u32,
}

impl ScaleFactor {
    pub fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        let divisor = gcd(u64::from(numerator), u64::from(denominator)) as u32;
        Some(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDisplayOutput {
    pub info: DisplayInfo,
    pub source: DisplayOutputSource,
    pub state: DisplayOutputState,
    pub modes: Vec<LinuxDisplayMode>,
    pub logical_geometry: Option<LogicalGeometry>,
    pub scale: Option<ScaleFactor>,
    pub rotation_degrees: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorState {
    Connected,
    Disconnected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorEnabled {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrmConnector {
    /// Sanitized connector label, for example `eDP-1`; never an EDID identifier.
    pub label: String,
    pub state: ConnectorState,
    pub enabled: ConnectorEnabled,
    /// `None` means the sysfs mode list was unavailable or invalid; an empty
    /// list means the source returned an empty, readable value.
    pub advertised_modes: Option<Vec<Resolution>>,
    pub connection: DisplayConnection,
    /// Set only after the connector's validated sysfs device relationship matches a P013 DRM node.
    pub gpu_instance: Option<InstanceId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxDisplayCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendDisplayOutput {
    pub name: Option<String>,
    pub state: DisplayOutputState,
    pub modes: Vec<LinuxDisplayMode>,
    pub primary: Option<bool>,
    pub logical_geometry: Option<LogicalGeometry>,
    pub scale: Option<ScaleFactor>,
    pub rotation_degrees: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendObservation {
    pub inventory_status: OutputInventoryStatus,
    pub outputs: Vec<BackendDisplayOutput>,
    pub optional_logical_output_protocol: bool,
    pub protocol_version: Option<(u32, u32)>,
    pub screen_geometry: Option<(u32, u32)>,
    pub diagnostic: Option<&'static str>,
}
