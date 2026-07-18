use std::{
    cell::RefCell,
    io::{self, Cursor, Write},
};
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, ScanRequest, WIRE_API_VERSION, WireErrorCode, WirePackHandler,
    WirePackResponse, WireServerError, serve_one,
};

struct RecordingHandler {
    calls: RefCell<Vec<&'static str>>,
}

impl RecordingHandler {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl WirePackHandler for RecordingHandler {
    fn language_id(&self) -> LanguageId {
        LanguageId::try_from("test").expect("hardcoded test id must be valid")
    }

    fn scan(&self, request: ScanRequest) -> WirePackResponse {
        self.calls.borrow_mut().push("scan");
        WirePackResponse::Error {
            api_version: request.api_version,
            language_id: request.language_id,
            code: WireErrorCode::InternalError,
            message: format!("scan {}", request.snapshot_id),
            diagnostics: Vec::new(),
        }
    }

    fn discover(&self, request: DiscoverRequest) -> WirePackResponse {
        self.calls.borrow_mut().push("discover");
        WirePackResponse::Error {
            api_version: request.api_version,
            language_id: request.language_id,
            code: WireErrorCode::InternalError,
            message: format!("discover {}", request.roots.join(",")),
            diagnostics: Vec::new(),
        }
    }
}

#[test]
fn skips_blank_lines_before_dispatching_a_valid_request() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "\n  \t\n{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"snapshot_id\":\"snap-1\",\"config\":{}}\n",
    );
    let mut output = Vec::new();

    serve_one(input, &mut output, &handler).unwrap();

    assert_eq!(*handler.calls.borrow(), ["scan"]);
    let response = response_from_output(&output);
    assert_error_message(response, "scan snap-1");
}

#[test]
fn eof_without_a_request_returns_an_invalid_request_response() {
    let handler = RecordingHandler::new();
    let mut output = Vec::new();

    serve_one(Cursor::new("\n \t\n"), &mut output, &handler).unwrap();

    assert!(handler.calls.borrow().is_empty());
    let response = response_from_output(&output);
    assert_error_code(response, "test", WireErrorCode::ConfigInvalid);
}

#[test]
fn malformed_json_returns_an_invalid_request_response() {
    let handler = RecordingHandler::new();
    let mut output = Vec::new();

    serve_one(Cursor::new("{not json}\n"), &mut output, &handler).unwrap();

    assert!(handler.calls.borrow().is_empty());
    let response = response_from_output(&output);
    assert_error_code(response, "test", WireErrorCode::ConfigInvalid);
}

#[test]
fn unsupported_api_version_returns_an_error_without_dispatching() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "{\"type\":\"discover\",\"api_version\":2,\"language_id\":\"other\",\"repo_root\":\"/repo\",\"roots\":[]}\n",
    );
    let mut output = Vec::new();

    serve_one(input, &mut output, &handler).unwrap();

    assert!(handler.calls.borrow().is_empty());
    let response = response_from_output(&output);
    assert_error_code(response, "other", WireErrorCode::ApiVersionUnsupported);
}

#[test]
fn dispatches_a_valid_scan_request() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"snapshot_id\":\"snap-2\",\"config\":{}}\n",
    );
    let mut output = Vec::new();

    serve_one(input, &mut output, &handler).unwrap();

    assert_eq!(*handler.calls.borrow(), ["scan"]);
    assert_error_message(response_from_output(&output), "scan snap-2");
}

#[test]
fn dispatches_a_valid_discover_request() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "{\"type\":\"discover\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"roots\":[\"src\"]}\n",
    );
    let mut output = Vec::new();

    serve_one(input, &mut output, &handler).unwrap();

    assert_eq!(*handler.calls.borrow(), ["discover"]);
    assert_error_message(response_from_output(&output), "discover src");
}

#[test]
fn emits_one_response_with_one_trailing_newline() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"snapshot_id\":\"snap-3\",\"config\":{}}\n{\"type\":\"discover\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"roots\":[]}\n",
    );
    let mut output = Vec::new();

    serve_one(input, &mut output, &handler).unwrap();

    assert_eq!(*handler.calls.borrow(), ["scan"]);
    assert!(output.ends_with(b"\n"));
    assert!(!output.ends_with(b"\n\n"));
    assert_eq!(std::str::from_utf8(&output).unwrap().lines().count(), 1);
}

#[test]
fn flush_failure_is_returned_as_a_wire_server_error() {
    let handler = RecordingHandler::new();
    let input = Cursor::new(
        "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"test\",\"repo_root\":\"/repo\",\"snapshot_id\":\"snap-4\",\"config\":{}}\n",
    );
    let mut output = FlushFails::default();

    let error = serve_one(input, &mut output, &handler).unwrap_err();

    assert!(matches!(error, WireServerError::Flush { .. }));
}

fn response_from_output(output: &[u8]) -> WirePackResponse {
    serde_json::from_slice(output).expect("output must be one wire response")
}

fn assert_error_code(response: WirePackResponse, language_id: &str, code: WireErrorCode) {
    match response {
        WirePackResponse::Error {
            api_version,
            language_id: actual_language_id,
            code: actual_code,
            ..
        } => {
            assert_eq!(api_version, WIRE_API_VERSION);
            assert_eq!(actual_language_id.as_str(), language_id);
            assert_eq!(actual_code, code);
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

fn assert_error_message(response: WirePackResponse, message: &str) {
    match response {
        WirePackResponse::Error {
            message: actual_message,
            ..
        } => assert_eq!(actual_message, message),
        other => panic!("expected error response, got {other:?}"),
    }
}

#[derive(Default)]
struct FlushFails(Vec<u8>);

impl Write for FlushFails {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}
