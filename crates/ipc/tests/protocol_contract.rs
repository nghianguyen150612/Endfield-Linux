use endfield_unix_ipc::{
    DaemonStatus, ErrorCode, IpcResponse, ProtocolVersion, Request, RequestId, RequestType,
    ResponsePayload, ResponseType, SUPPORTED_PROTOCOL,
};

#[test]
fn v1_envelopes_have_explicit_version_ids_and_message_types() {
    assert_eq!(SUPPORTED_PROTOCOL, ProtocolVersion { major: 1, minor: 0 });
    let request = Request::new(
        RequestType::GetStatus,
        RequestId::new("contract-1").expect("bounded request id"),
    );
    let request_json = serde_json::to_value(request).expect("request serializes");
    assert_eq!(request_json["protocol_major"], 1);
    assert_eq!(request_json["protocol_minor"], 0);
    assert_eq!(request_json["message_type"], "get_status");
    assert!(request_json.get("uid").is_none());

    let response = IpcResponse::new(
        RequestId::new("contract-1").expect("bounded request id"),
        ResponseType::Status,
        ResponsePayload::Status(DaemonStatus::running("0.1.0", "0.1.0")),
    );
    let response_json = serde_json::to_value(response).expect("response serializes");
    assert_eq!(response_json["protocol_major"], 1);
    assert_eq!(response_json["message_type"], "status");
    assert_eq!(response_json["payload"]["status"]["lifecycle"], "running");
}

#[test]
fn unknown_operations_missing_fields_and_extra_fields_are_rejected_or_typed() {
    let unknown = br#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"stop_daemon","payload":{}}"#;
    let request: Request = serde_json::from_slice(unknown).expect("unknown operation is typed");
    assert_eq!(request.message_type, RequestType::Unknown);

    let missing =
        br#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"ping"}"#;
    assert!(serde_json::from_slice::<Request>(missing).is_err());

    let extra = br#"{"protocol_major":1,"protocol_minor":0,"request_id":"x","message_type":"ping","payload":{},"uid":0}"#;
    assert!(serde_json::from_slice::<Request>(extra).is_err());
}

#[test]
fn request_ids_and_integer_fields_reject_invalid_and_overflow_values() {
    assert!(RequestId::new("bad/id").is_err());
    assert!(RequestId::new("x".repeat(65)).is_err());
    let maximum_u16 = br#"{"protocol_major":65535,"protocol_minor":65535,"request_id":"x","message_type":"ping","payload":{}}"#;
    assert!(serde_json::from_slice::<Request>(maximum_u16).is_ok());
    let overflow = br#"{"protocol_major":65536,"protocol_minor":0,"request_id":"x","message_type":"ping","payload":{}}"#;
    assert!(serde_json::from_slice::<Request>(overflow).is_err());
}

#[test]
fn typed_error_codes_are_stable_and_snake_case() {
    let response = IpcResponse::new(
        RequestId::new("x").expect("valid id"),
        ResponseType::Error,
        ResponsePayload::Error {
            code: ErrorCode::UnsupportedProtocolVersion,
        },
    );
    let value = serde_json::to_value(response).expect("serialize error response");
    assert!(value.to_string().contains("unsupported_protocol_version"));
}
