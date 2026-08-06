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
use wax_lang_react::{ReactDiscoverError, ReactLanguage, ReactScanError, RegistryErrorKind};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-react")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    require_stdio(cli.stdio, "wax-lang-react");
    Ok(serve_stdio(&ReactWireHandler(ReactLanguage::new()))?)
}

#[cfg(test)]
fn run_stdio_with_reader<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
) -> Result<(), WireServerError> {
    serve_one(reader, writer, &ReactWireHandler(ReactLanguage::new()))
}

struct ReactWireHandler(ReactLanguage);

impl WirePackHandler for ReactWireHandler {
    fn language_id(&self) -> LanguageId {
        pack_language_id("react")
    }

    fn scan(&self, request: ScanRequest) -> WirePackResponse {
        match self.0.scan(&request) {
            Ok(facts) => scan_facts_response(&request, facts),
            Err(err) => {
                let code = match &err {
                    ReactScanError::InvalidConfig(_) => WireErrorCode::ConfigInvalid,
                    ReactScanError::Registry(err) => match err.kind() {
                        RegistryErrorKind::NotFound => WireErrorCode::RegistryNotFound,
                        RegistryErrorKind::Invalid => WireErrorCode::ScanFailed,
                    },
                    ReactScanError::Parse(_) => WireErrorCode::ScanFailed,
                    ReactScanError::Io { .. } => WireErrorCode::ScanFailed,
                    ReactScanError::InvalidLanguageId(_) => WireErrorCode::ScanFailed,
                    ReactScanError::InvalidFacts(_) => WireErrorCode::ScanFailed,
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
        match self.0.discover(&request) {
            Ok(result) => {
                discover_symbols_response(&request, result.components, result.diagnostics)
            }
            Err(err) => discover_error_response(request.api_version, request.language_id, err),
        }
    }
}

fn discover_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: ReactDiscoverError,
) -> WirePackResponse {
    let code = match &err {
        ReactDiscoverError::InvalidLanguageId(_) | ReactDiscoverError::MissingRoot(_) => {
            WireErrorCode::ConfigInvalid
        }
        ReactDiscoverError::Io { .. } => WireErrorCode::ScanFailed,
    };
    wire_error_response(api_version, language_id, code, err.to_string())
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
                assert_eq!(language_id.as_str(), "react");
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_api_version_on_discover_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"discover\",\"api_version\":2,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"roots\":[\"src\"]}\n",
        );
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
                assert_eq!(language_id.as_str(), "react");
                assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_api_version_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":2,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-bad-version\",\"config\":{}}\n",
        );
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
                assert_eq!(language_id.as_str(), "react");
                assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
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
                assert_eq!(code, WireErrorCode::ScanFailed);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn invalid_scan_config_maps_to_config_invalid_wire_error() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-invalid-config\",\"config\":{\"roots\":[\"src\"]}}\n",
        );
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

    #[test]
    fn invalid_registry_maps_to_scan_failed_wire_error() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let registry_dir = temp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).expect("registry dir should be created");
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[]}"#,
        )
        .expect("registry fixture should be written");

        let request = serde_json::json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "react",
            "repo_root": temp.path().to_string_lossy(),
            "snapshot_id": "snap-invalid-registry",
            "config": {
                "registry": "design-system/registry.json",
                "roots": ["src"]
            }
        });
        let input = Cursor::new(format!("{request}\n"));
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error { code, message, .. } => {
                assert_eq!(code, WireErrorCode::ScanFailed);
                assert!(message.contains("invalid react registry"));
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn missing_registry_maps_to_registry_not_found_wire_error() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("src dir should be created");
        std::fs::write(src_dir.join("App.tsx"), "export {}").expect("source fixture");

        let request = serde_json::json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "react",
            "repo_root": temp.path().to_string_lossy(),
            "snapshot_id": "snap-missing-registry",
            "config": {
                "registry": "design-system/registry.json",
                "roots": ["src"]
            }
        });
        let input = Cursor::new(format!("{request}\n"));
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::Error { code, message, .. } => {
                assert_eq!(code, WireErrorCode::RegistryNotFound);
                assert!(message.contains("react registry not found"));
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn valid_scan_response_keeps_request_and_snapshot() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-42\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WirePackResponse = serde_json::from_str(line).unwrap();
        match response {
            WirePackResponse::ScanFacts {
                api_version,
                language_id,
                facts,
            } => {
                assert_eq!(api_version, 1);
                assert_eq!(language_id.as_str(), "react");
                assert_eq!(facts.language.id.as_str(), "react");
                assert_eq!(facts.snapshot_id, "snap-42");
            }
            other => panic!("expected scan_facts response, got {other:?}"),
        }
    }
}
