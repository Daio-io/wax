use clap::Parser;
use std::io::{self, BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{WIRE_API_VERSION, WireErrorCode, WireScanRequest, WireScanResponse};
use wax_lang_react::{ReactLanguage, ReactScanError};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-react")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if !cli.stdio {
        eprintln!("usage: wax-lang-react --stdio");
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

        let request: WireScanRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = WireScanResponse::Error {
                    api_version: WIRE_API_VERSION,
                    language_id: react_language_id(),
                    code: WireErrorCode::ConfigInvalid,
                    message: format!("invalid scan request JSON: {err}"),
                    diagnostics: Vec::new(),
                };
                serde_json::to_writer(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
                writer.flush()?;
                return Ok(());
            }
        };

        let WireScanRequest::Scan {
            api_version,
            language_id,
            repo_root,
            snapshot_id,
            config,
        } = request;

        if api_version != WIRE_API_VERSION {
            let response = WireScanResponse::Error {
                api_version: WIRE_API_VERSION,
                language_id,
                code: WireErrorCode::ApiVersionUnsupported,
                message: format!(
                    "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
                ),
                diagnostics: Vec::new(),
            };
            serde_json::to_writer(&mut *writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            return Ok(());
        }

        let scan_request = wax_lang_api::ScanRequest {
            request_type: wax_lang_api::ScanRequestType::Scan,
            api_version,
            language_id: language_id.clone(),
            repo_root,
            snapshot_id,
            config,
        };

        let react = ReactLanguage::new();
        let response = match react.scan(&scan_request) {
            Ok(facts) => WireScanResponse::ScanFacts {
                api_version,
                language_id,
                facts: Box::new(facts),
            },
            Err(err) => {
                let code = match &err {
                    ReactScanError::InvalidConfig(_) => WireErrorCode::ConfigInvalid,
                    ReactScanError::RegistryInvalid(_) => WireErrorCode::ScanFailed,
                    _ => WireErrorCode::ScanFailed,
                };
                WireScanResponse::Error {
                    api_version,
                    language_id,
                    code,
                    message: err.to_string(),
                    diagnostics: Vec::new(),
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

fn react_language_id() -> LanguageId {
    LanguageId::try_from("react").expect("hardcoded react id must be valid")
}

#[cfg(test)]
mod tests {
    use super::run_stdio_with_reader;
    use std::io::Cursor;
    use wax_lang_api::{WireErrorCode, WireScanResponse};

    #[test]
    fn invalid_json_returns_tagged_error_response() {
        let input = Cursor::new("{not json}\n");
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::Error {
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
    fn unsupported_api_version_returns_tagged_error_response() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":2,\"language_id\":\"react\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-bad-version\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::Error {
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
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::Error {
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
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::Error { code, .. } => {
                assert_eq!(code, WireErrorCode::ConfigInvalid);
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
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::ScanFacts {
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
