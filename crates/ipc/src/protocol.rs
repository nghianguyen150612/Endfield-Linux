use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::MAX_REQUEST_ID_BYTES;

pub const SUPPORTED_PROTOCOL: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(String);

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid request identifier")]
pub struct RequestIdError;

impl RequestId {
    pub fn new(value: impl Into<String>) -> Result<Self, RequestIdError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_REQUEST_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(RequestIdError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    Ping,
    GetStatus,
    Unknown,
}

impl<'de> Deserialize<'de> for RequestType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "ping" => Self::Ping,
            "get_status" => Self::GetStatus,
            _ => Self::Unknown,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyPayload {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub request_id: RequestId,
    pub message_type: RequestType,
    pub payload: EmptyPayload,
}

impl Request {
    pub fn new(message_type: RequestType, request_id: RequestId) -> Self {
        Self {
            protocol_major: SUPPORTED_PROTOCOL.major,
            protocol_minor: SUPPORTED_PROTOCOL.minor,
            request_id,
            message_type,
            payload: EmptyPayload {},
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLifecycle {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementedCapability {
    LocalIpc,
    Ping,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnimplementedCapability {
    Telemetry,
    GameSessionSupervision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusCode {
    Ok,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonStatus {
    pub service: String,
    pub daemon_package_version: String,
    pub application_package_version: String,
    pub lifecycle: DaemonLifecycle,
    pub ipc_available: bool,
    pub status_code: StatusCode,
    pub implemented_capabilities: Vec<ImplementedCapability>,
    pub unimplemented_capabilities: Vec<UnimplementedCapability>,
}

impl DaemonStatus {
    pub fn running(daemon_version: &str, application_version: &str) -> Self {
        Self::for_lifecycle(
            DaemonLifecycle::Running,
            daemon_version,
            application_version,
        )
    }

    pub fn for_lifecycle(
        lifecycle: DaemonLifecycle,
        daemon_version: &str,
        application_version: &str,
    ) -> Self {
        Self {
            service: "endfield-unixd".to_owned(),
            daemon_package_version: daemon_version.to_owned(),
            application_package_version: application_version.to_owned(),
            lifecycle,
            ipc_available: true,
            status_code: StatusCode::Ok,
            implemented_capabilities: vec![
                ImplementedCapability::LocalIpc,
                ImplementedCapability::Ping,
                ImplementedCapability::Status,
            ],
            unimplemented_capabilities: vec![
                UnimplementedCapability::Telemetry,
                UnimplementedCapability::GameSessionSupervision,
            ],
        }
    }

    pub fn validate(&self) -> bool {
        self.service == "endfield-unixd"
            && valid_package_version(&self.daemon_package_version)
            && valid_package_version(&self.application_package_version)
            && self.lifecycle == DaemonLifecycle::Running
            && self.ipc_available
            && self.status_code == StatusCode::Ok
            && self.implemented_capabilities
                == [
                    ImplementedCapability::LocalIpc,
                    ImplementedCapability::Ping,
                    ImplementedCapability::Status,
                ]
            && self.unimplemented_capabilities
                == [
                    UnimplementedCapability::Telemetry,
                    UnimplementedCapability::GameSessionSupervision,
                ]
    }
}

fn valid_package_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedRequest,
    UnsupportedProtocolVersion,
    UnsupportedOperation,
    AuthenticationFailure,
    ServerBusy,
    RequestTimeout,
    ServiceShuttingDown,
    InternalFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ResponsePayload {
    Pong,
    Status(DaemonStatus),
    Error { code: ErrorCode },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcResponse {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub request_id: RequestId,
    pub message_type: ResponseType,
    pub payload: ResponsePayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseType {
    Pong,
    Status,
    Error,
}

impl IpcResponse {
    pub fn new(
        request_id: RequestId,
        message_type: ResponseType,
        payload: ResponsePayload,
    ) -> Self {
        Self {
            protocol_major: SUPPORTED_PROTOCOL.major,
            protocol_minor: SUPPORTED_PROTOCOL.minor,
            request_id,
            message_type,
            payload,
        }
    }

    pub fn validate_shape(&self) -> bool {
        matches!(
            (&self.message_type, &self.payload),
            (ResponseType::Pong, ResponsePayload::Pong)
                | (ResponseType::Status, ResponsePayload::Status(_))
                | (ResponseType::Error, ResponsePayload::Error { .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_contract_uses_an_independent_version_and_typed_envelopes() {
        assert_eq!(SUPPORTED_PROTOCOL, ProtocolVersion { major: 1, minor: 0 });
        let request = Request::new(
            RequestType::Ping,
            RequestId::new("0000000000000001").expect("valid request id"),
        );
        let json = serde_json::to_string(&request).expect("serialize request");
        assert_eq!(
            json,
            r#"{"protocol_major":1,"protocol_minor":0,"request_id":"0000000000000001","message_type":"ping","payload":{}}"#
        );
        assert!(serde_json::from_str::<Request>(&json).is_ok());
        assert!(serde_json::from_str::<Request>(
            r#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"ping","payload":{},"uid":0}"#
        )
        .is_err());
    }

    #[test]
    fn request_ids_are_bounded_ascii_identifiers_not_credentials() {
        assert!(RequestId::new("x".repeat(MAX_REQUEST_ID_BYTES)).is_ok());
        assert!(RequestId::new("x".repeat(MAX_REQUEST_ID_BYTES + 1)).is_err());
        assert!(RequestId::new("../private").is_err());
        assert!(RequestId::new("").is_err());
    }

    #[test]
    fn unknown_operation_is_recognized_as_unsupported_and_payload_remains_strict() {
        let request: Request = serde_json::from_str(
            r#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"stop_daemon","payload":{}}"#,
        )
        .expect("unknown operation remains rejectable");
        assert_eq!(request.message_type, RequestType::Unknown);
        assert!(serde_json::from_str::<Request>(
            r#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"ping","payload":{"extra":true}}"#
        )
        .is_err());
    }

    #[test]
    fn status_contract_has_only_implemented_read_only_capabilities() {
        let status = DaemonStatus::running("0.1.0", "0.1.0");
        assert!(status.validate());
        let encoded = serde_json::to_value(&status).expect("serialize status");
        assert_eq!(encoded["lifecycle"], "running");
        assert_eq!(encoded["ipc_available"], true);
        assert_eq!(
            encoded["implemented_capabilities"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}
