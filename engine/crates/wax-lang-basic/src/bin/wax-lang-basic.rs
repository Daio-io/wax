use clap::Parser;
#[cfg(test)]
use std::io::{BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, ScanRequest, WIRE_API_VERSION, WireErrorCode, WirePackHandler,
    WirePackResponse, pack_language_id, require_stdio, scan_facts_response, serve_stdio,
    wire_error_response,
};
#[cfg(test)]
use wax_lang_api::{WireServerError, serve_one};
use wax_lang_basic::{BasicLanguage, BasicScanError};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-basic")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    require_stdio(cli.stdio, "wax-lang-basic");
    Ok(serve_stdio(&BasicWireHandler(BasicLanguage::new()))?)
}

#[cfg(test)]
fn run_stdio_with_reader<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
) -> Result<(), WireServerError> {
    serve_one(reader, writer, &BasicWireHandler(BasicLanguage::new()))
}

struct BasicWireHandler(BasicLanguage);

impl WirePackHandler for BasicWireHandler {
    fn language_id(&self) -> LanguageId {
        pack_language_id("basic")
    }

    fn scan(&self, request: ScanRequest) -> WirePackResponse {
        match self.0.scan(&request) {
            Ok(facts) => scan_facts_response(&request, facts),
            Err(err) => {
                let code = match &err {
                    BasicScanError::InvalidConfig(_) | BasicScanError::InvalidLanguageId(_) => {
                        WireErrorCode::ConfigInvalid
                    }
                    _ => WireErrorCode::ScanFailed,
                };
                wire_error_response(
                    request.api_version,
                    request.language_id,
                    code,
                    err.to_string(),
                )
            }
        }
    }

    fn discover(&self, request: DiscoverRequest) -> WirePackResponse {
        let message = format!(
            "{} does not support registry discovery yet",
            request.language_id
        );
        wire_error_response(
            WIRE_API_VERSION,
            request.language_id,
            WireErrorCode::DiscoverUnsupported,
            message,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::run_stdio_with_reader;
    use std::io::Cursor;
    use wax_lang_api::{WireErrorCode, WirePackResponse};

    #[test]
    fn invalid_json_returns_tagged_error_response() {
        let input = Cursor::new("{not json}\n");
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error {
                api_version,
                language_id,
                code,
                ..
            } => {
                assert_eq!(api_version, 1);
                assert_eq!(language_id.as_str(), "basic");
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn scan_error_echoes_request_language_id() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"compose\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-1\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error {
                language_id, code, ..
            } => {
                assert_eq!(language_id.as_str(), "compose");
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn valid_scan_response_keeps_request_and_snapshot() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"basic\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-42\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::ScanFacts {
                language_id, facts, ..
            } => {
                assert_eq!(language_id.as_str(), "basic");
                assert_eq!(facts.snapshot_id, "snap-42");
            }
            other => panic!("expected scan_facts response, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_api_version_on_discover_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"discover\",\"api_version\":2,\"language_id\":\"basic\",\"repo_root\":\"/tmp/repo\",\"roots\":[\"src\"]}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let response: WirePackResponse =
            serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
        match response {
            WirePackResponse::Error {
                api_version,
                language_id,
                code,
                ..
            } => {
                assert_eq!(api_version, 1);
                assert_eq!(language_id.as_str(), "basic");
                assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn discover_request_returns_discover_unsupported() {
        let input = Cursor::new(
            "{\"type\":\"discover\",\"api_version\":1,\"language_id\":\"basic\",\"repo_root\":\"/tmp/repo\",\"roots\":[\"src\"]}\n",
        );
        let mut output = Vec::new();
        run_stdio_with_reader(input, &mut output).unwrap();

        let response: WirePackResponse =
            serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
        match response {
            WirePackResponse::Error { code, .. } => {
                assert_eq!(code, WireErrorCode::DiscoverUnsupported);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn malformed_registry_returns_config_invalid_wire_code() {
        let fixture_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/invalid-registry");
        let request = format!(
            "{{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"basic\",\"repo_root\":\"{}\",\"snapshot_id\":\"snap-bad-registry\",\"config\":{{\"registry\":\"design-system/registry.json\",\"roots\":[\"app/src\"]}}}}",
            fixture_root.display()
        );
        let input = Cursor::new(format!("{request}\n"));
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error { code, .. } => {
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }
}
