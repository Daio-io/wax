use clap::Parser;
use std::io::{self, BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, DiscoverRequestType, ScanRequestType, WIRE_API_VERSION, WireErrorCode,
    WirePackRequest, WirePackResponse,
};
use wax_lang_swift::{SwiftDiscoverError, SwiftLanguage, SwiftScanError};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-swift")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if !cli.stdio {
        eprintln!("usage: wax-lang-swift --stdio");
        std::process::exit(2);
    }

    run_stdio()
}

fn run_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    run_stdio_with_reader(stdin.lock(), &mut stdout)
}

fn run_stdio_with_reader<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    for line_result in reader.lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let request: WirePackRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = WirePackResponse::Error {
                    api_version: WIRE_API_VERSION,
                    language_id: swift_language_id(),
                    code: WireErrorCode::ConfigInvalid,
                    message: format!("invalid pack request JSON: {err}"),
                    diagnostics: Vec::new(),
                };
                serde_json::to_writer(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
                writer.flush()?;
                return Ok(());
            }
        };

        let response = match request {
            WirePackRequest::Scan {
                api_version,
                language_id,
                repo_root,
                snapshot_id,
                config,
            } => {
                if api_version != WIRE_API_VERSION {
                    WirePackResponse::Error {
                        api_version: WIRE_API_VERSION,
                        language_id,
                        code: WireErrorCode::ApiVersionUnsupported,
                        message: format!(
                            "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
                        ),
                        diagnostics: Vec::new(),
                    }
                } else {
                    let scan_request = wax_lang_api::ScanRequest {
                        request_type: ScanRequestType::Scan,
                        api_version,
                        language_id: language_id.clone(),
                        repo_root,
                        snapshot_id,
                        config,
                    };
                    let swift = SwiftLanguage::new();
                    match swift.scan(&scan_request) {
                        Ok(facts) => WirePackResponse::ScanFacts {
                            api_version,
                            language_id,
                            facts: Box::new(facts),
                        },
                        Err(err) => scan_error_response(api_version, language_id, err),
                    }
                }
            }
            WirePackRequest::Discover {
                api_version,
                language_id,
                repo_root,
                roots,
            } => {
                if api_version != WIRE_API_VERSION {
                    WirePackResponse::Error {
                        api_version: WIRE_API_VERSION,
                        language_id,
                        code: WireErrorCode::ApiVersionUnsupported,
                        message: format!(
                            "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
                        ),
                        diagnostics: Vec::new(),
                    }
                } else {
                    let discover_request = DiscoverRequest {
                        request_type: DiscoverRequestType::Discover,
                        api_version,
                        language_id: language_id.clone(),
                        repo_root,
                        roots,
                    };
                    let swift = SwiftLanguage::new();
                    match swift.discover(&discover_request) {
                        Ok(result) => WirePackResponse::DiscoverSymbols {
                            api_version,
                            language_id,
                            symbols: wax_lang_api::DiscoveredRegistrySymbol::symbol_names(
                                &result.components,
                            ),
                            components: result.components,
                            diagnostics: result.diagnostics,
                        },
                        Err(err) => discover_error_response(api_version, language_id, err),
                    }
                }
            }
        };

        serde_json::to_writer(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        return Ok(());
    }

    Ok(())
}

fn scan_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: SwiftScanError,
) -> WirePackResponse {
    let (code, message) = match &err {
        SwiftScanError::InvalidConfig(_) => (WireErrorCode::ConfigInvalid, err.to_string()),
        SwiftScanError::ParserInitFailed(_) => (WireErrorCode::ParserInitFailed, err.to_string()),
        SwiftScanError::RegistryNotFound(_) => (WireErrorCode::RegistryNotFound, err.to_string()),
        _ => (WireErrorCode::ScanFailed, err.to_string()),
    };
    WirePackResponse::Error {
        api_version,
        language_id,
        code,
        message,
        diagnostics: Vec::new(),
    }
}

fn discover_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: SwiftDiscoverError,
) -> WirePackResponse {
    let code = match &err {
        SwiftDiscoverError::InvalidLanguageId(_) | SwiftDiscoverError::MissingRoot(_) => {
            WireErrorCode::ConfigInvalid
        }
        SwiftDiscoverError::ParserInitFailed(_) => WireErrorCode::ParserInitFailed,
        SwiftDiscoverError::Io { .. } => WireErrorCode::ScanFailed,
    };
    WirePackResponse::Error {
        api_version,
        language_id,
        code,
        message: err.to_string(),
        diagnostics: Vec::new(),
    }
}

fn swift_language_id() -> LanguageId {
    LanguageId::try_from("swift").expect("hardcoded swift id must be valid")
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
                assert_eq!(language_id.as_str(), "swift");
                assert_eq!(code, WireErrorCode::ConfigInvalid);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn wrong_language_id_echoes_request_language_id() {
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
    fn unsupported_api_version_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":2,\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-bad-version\",\"config\":{}}\n",
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
                assert_eq!(language_id.as_str(), "swift");
                assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_api_version_on_discover_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"discover\",\"api_version\":2,\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"roots\":[\"src\"]}\n",
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
                assert_eq!(language_id.as_str(), "swift");
                assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn invalid_scan_config_maps_to_config_invalid_wire_error() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-invalid-config\",\"config\":{\"roots\":[\"src\"]}}\n",
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
    fn scan_response_preserves_snapshot_id() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-42\",\"config\":{}}\n",
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
                assert_eq!(language_id.as_str(), "swift");
                assert_eq!(facts.language.id.as_str(), "swift");
                assert_eq!(facts.snapshot_id, "snap-42");
            }
            other => panic!("expected scan_facts response, got {other:?}"),
        }
    }
}
