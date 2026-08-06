use clap::Parser;
#[cfg(test)]
use std::io::{BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, ScanRequest, WireErrorCode, WirePackHandler, WirePackResponse,
    discover_symbols_response, pack_language_id, require_stdio, scan_facts_response, serve_stdio,
    wire_error_response,
};
#[cfg(test)]
use wax_lang_api::{WireServerError, serve_one};
use wax_lang_compose::{ComposeDiscoverError, ComposeLanguage, ComposeScanError};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-compose")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    require_stdio(cli.stdio, "wax-lang-compose");
    Ok(serve_stdio(&ComposeWireHandler(ComposeLanguage::new()))?)
}

#[cfg(test)]
fn run_stdio_with_reader<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
) -> Result<(), WireServerError> {
    serve_one(reader, writer, &ComposeWireHandler(ComposeLanguage::new()))
}

struct ComposeWireHandler(ComposeLanguage);

impl WirePackHandler for ComposeWireHandler {
    fn language_id(&self) -> LanguageId {
        pack_language_id("compose")
    }

    fn scan(&self, request: ScanRequest) -> WirePackResponse {
        match self.0.scan(&request) {
            Ok(facts) => scan_facts_response(&request, facts),
            Err(err) => scan_error_response(request.api_version, request.language_id, err),
        }
    }

    fn discover(&self, request: DiscoverRequest) -> WirePackResponse {
        match self.0.discover(&request) {
            Ok(result) => {
                discover_symbols_response(&request, result.components, result.diagnostics)
            }
            Err(err) => discover_error_response(request.api_version, request.language_id, err),
        }
    }
}

fn scan_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: ComposeScanError,
) -> WirePackResponse {
    let code = match &err {
        ComposeScanError::InvalidConfig(_) => WireErrorCode::ConfigInvalid,
        ComposeScanError::ParserInitFailed(_) => WireErrorCode::ParserInitFailed,
        _ => WireErrorCode::ScanFailed,
    };
    wire_error_response(api_version, language_id, code, err.to_string())
}

fn discover_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: ComposeDiscoverError,
) -> WirePackResponse {
    let code = match &err {
        ComposeDiscoverError::InvalidLanguageId(_) | ComposeDiscoverError::MissingRoot(_) => {
            WireErrorCode::ConfigInvalid
        }
        ComposeDiscoverError::ParserInitFailed(_) => WireErrorCode::ParserInitFailed,
        ComposeDiscoverError::Io { .. } => WireErrorCode::ScanFailed,
    };
    wire_error_response(api_version, language_id, code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{run_stdio_with_reader, scan_error_response};
    use std::io::Cursor;
    use wax_lang_api::{WIRE_API_VERSION, WireErrorCode, WirePackResponse, pack_language_id};

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
                assert_eq!(language_id.as_str(), "compose");
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn scan_error_echoes_request_language_id() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-1\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error {
                language_id, code, ..
            } => {
                assert_eq!(language_id.as_str(), "react");
                assert_eq!(code, WireErrorCode::ScanFailed);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn parser_init_failed_maps_to_correct_wire_error_code() {
        let err = wax_lang_compose::ComposeScanError::ParserInitFailed("test".to_owned());
        let response = scan_error_response(WIRE_API_VERSION, pack_language_id("compose"), err);
        match response {
            WirePackResponse::Error { code, .. } => {
                assert_eq!(code, WireErrorCode::ParserInitFailed);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn valid_scan_response_keeps_request_and_snapshot() {
        let repo_root = format!("{}/tests/fixtures/small", env!("CARGO_MANIFEST_DIR"));
        let input = Cursor::new(format!(
            "{{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"compose\",\"repo_root\":\"{repo_root}\",\"snapshot_id\":\"snap-42\",\"config\":{{\"registry\":\"design-system/registry.json\",\"roots\":[\"app/src/main/kotlin\"]}}}}\n"
        ));
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::ScanFacts {
                language_id, facts, ..
            } => {
                assert_eq!(language_id.as_str(), "compose");
                assert_eq!(facts.snapshot_id, "snap-42");
            }
            other => panic!("expected scan_facts response, got {other:?}"),
        }
    }
}
