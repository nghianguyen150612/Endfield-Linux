use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

use endfield_unix_ipc::{
    Client, ClientError, DaemonLifecycle, DaemonStatus, ErrorCode, IpcResponse, LocalSocketServer,
    Request, RequestId, RequestType, ResponsePayload, ResponseType, RuntimePaths, read_frame,
    write_frame,
};

#[test]
fn real_unix_socket_client_ping_status_malformed_request_and_reconnect() {
    let runtime = tempfile::tempdir().expect("isolated runtime directory");
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("runtime paths");
    let server = LocalSocketServer::bind_at(paths.clone()).expect("bind local service");
    let shutdown = server.shutdown_handle();
    let socket_path = paths.socket_path().to_path_buf();
    let worker = thread::spawn(move || {
        while let Some(stream) = server.accept().expect("accept client or shutdown") {
            let _ = endfield_unix_ipc::handle_connection(
                stream,
                nix::unistd::Uid::effective().as_raw(),
                Instant::now(),
                || DaemonStatus::running("0.1.0", "0.1.0"),
            );
        }
        server
            .close()
            .expect("remove owned socket and release lock");
    });

    let client = Client::at_socket_path(&socket_path, Duration::from_secs(2));
    client.ping().expect("authenticated ping/pong");
    let status = client.status().expect("validated daemon status");
    assert_eq!(status.lifecycle, DaemonLifecycle::Running);
    assert!(status.ipc_available);

    let mut raw = UnixStream::connect(&socket_path).expect("raw local client");
    let malformed = br#"{"protocol_major":1,"protocol_minor":0,"request_id":"raw","message_type":"ping","payload":{},"uid":0}"#;
    write_frame(&mut raw, malformed).expect("send strict-schema violation");
    let response_bytes = read_frame(&mut raw).expect("typed malformed-request response");
    let response: IpcResponse =
        serde_json::from_slice(&response_bytes).expect("typed error envelope");
    assert_eq!(response.message_type, ResponseType::Error);
    assert_eq!(
        response.payload,
        ResponsePayload::Error {
            code: ErrorCode::MalformedRequest
        }
    );

    client
        .ping()
        .expect("server remains healthy after malformed request");

    let newer_major = serde_json::json!({
        "protocol_major": 2,
        "protocol_minor": 0,
        "request_id": "major-mismatch",
        "message_type": "ping",
        "payload": {}
    });
    let response = raw_exchange(&socket_path, &serde_json::to_vec(&newer_major).unwrap());
    assert_eq!(response.message_type, ResponseType::Error);
    assert_eq!(
        response.payload,
        ResponsePayload::Error {
            code: ErrorCode::UnsupportedProtocolVersion
        }
    );

    let unknown = Request::new(
        RequestType::Unknown,
        RequestId::new("unknown-operation").expect("bounded id"),
    );
    let response = raw_exchange(&socket_path, &serde_json::to_vec(&unknown).unwrap());
    assert_eq!(
        response.payload,
        ResponsePayload::Error {
            code: ErrorCode::UnsupportedOperation
        }
    );
    shutdown.request_shutdown().expect("wake listener");
    worker.join().expect("server worker exits");
    assert!(!socket_path.exists());
}

#[test]
fn client_rejects_mismatched_response_id_type_and_protocol() {
    for mismatch in 0..3 {
        let runtime = tempfile::tempdir().expect("isolated runtime directory");
        let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("runtime paths");
        paths
            .prepare_server_directory()
            .expect("private application directory");
        let listener =
            std::os::unix::net::UnixListener::bind(paths.socket_path()).expect("test listener");
        let socket_path = paths.socket_path().to_path_buf();
        let response_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let request_bytes = read_frame(&mut stream).expect("read client request");
            let request: Request =
                serde_json::from_slice(&request_bytes).expect("typed client request");
            let (request_id, response_type, payload, major) = match mismatch {
                0 => (
                    RequestId::new("wrong-id").expect("valid id"),
                    ResponseType::Pong,
                    ResponsePayload::Pong,
                    1,
                ),
                1 => (
                    request.request_id,
                    ResponseType::Status,
                    ResponsePayload::Status(DaemonStatus::running("0.1.0", "0.1.0")),
                    1,
                ),
                _ => (
                    request.request_id,
                    ResponseType::Pong,
                    ResponsePayload::Pong,
                    2,
                ),
            };
            let mut response = IpcResponse::new(request_id, response_type, payload);
            response.protocol_major = major;
            let bytes = serde_json::to_vec(&response).expect("serialize fake response");
            write_frame(&mut stream, &bytes).expect("write fake response");
        });
        let client = Client::at_socket_path(&socket_path, Duration::from_secs(1));
        let error = if mismatch == 2 {
            ClientError::ProtocolMismatch
        } else {
            ClientError::InvalidResponse
        };
        assert_eq!(client.ping(), Err(error));
        response_thread.join().expect("fake responder exits");
    }
}

#[test]
fn client_enforces_one_overall_request_deadline() {
    let runtime = tempfile::tempdir().expect("isolated runtime directory");
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("runtime paths");
    paths
        .prepare_server_directory()
        .expect("private application directory");
    let listener =
        std::os::unix::net::UnixListener::bind(paths.socket_path()).expect("test listener");
    let socket_path = paths.socket_path().to_path_buf();
    let server_thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let _ = read_frame(&mut stream).expect("read client request");
        thread::sleep(Duration::from_millis(150));
        let _ = stream.write_all(&[0, 0, 0, 2, b'{', b'}']);
    });
    let client = Client::at_socket_path(socket_path, Duration::from_millis(40));
    assert_eq!(
        client.ping(),
        Err(endfield_unix_ipc::ClientError::RequestTimeout)
    );
    server_thread.join().expect("test server exits");
}

#[test]
fn server_deadline_is_not_extended_by_a_client_trickling_frame_bytes() {
    let runtime = tempfile::tempdir().expect("isolated runtime directory");
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("runtime paths");
    let server = LocalSocketServer::bind_at(paths.clone()).expect("bind local service");
    let socket_path = paths.socket_path().to_path_buf();
    let server_thread = thread::spawn(move || {
        let stream = server
            .accept()
            .expect("accept slow client")
            .expect("server remains active");
        let result = endfield_unix_ipc::handle_connection(
            stream,
            nix::unistd::Uid::effective().as_raw(),
            Instant::now(),
            || DaemonStatus::running("0.1.0", "0.1.0"),
        );
        assert!(matches!(
            result,
            Err(endfield_unix_ipc::ServerConnectionError::RequestTimeout)
        ));
        server.close().expect("close server after slow client");
    });

    let mut client = UnixStream::connect(socket_path).expect("connect slow client");
    let started = Instant::now();
    for _ in 0..3 {
        client
            .write_all(&[0])
            .expect("trickle one frame-header byte");
        thread::sleep(Duration::from_millis(600));
    }
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("bound disconnect observation");
    let mut byte = [0u8; 1];
    assert_eq!(client.read(&mut byte).expect("observe server close"), 0);
    assert!(started.elapsed() >= Duration::from_millis(1_200));
    assert!(started.elapsed() < Duration::from_secs(2));
    server_thread.join().expect("slow-client server exits");
}

fn raw_exchange(socket_path: &std::path::Path, payload: &[u8]) -> IpcResponse {
    let mut stream = UnixStream::connect(socket_path).expect("connect raw protocol client");
    write_frame(&mut stream, payload).expect("write raw protocol request");
    let bytes = read_frame(&mut stream).expect("read protocol response");
    serde_json::from_slice(&bytes).expect("decode protocol response")
}
