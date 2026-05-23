use clap::Parser;
use std::io::{self, BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{WIRE_API_VERSION, WireErrorCode, WireScanRequest, WireScanResponse};
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

    if !cli.stdio {
        eprintln!("usage: wax-lang-basic --stdio");
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
                    language_id: basic_language_id(),
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

        let basic = BasicLanguage::new();
        let response = match basic.scan(&scan_request) {
            Ok(facts) => WireScanResponse::ScanFacts {
                api_version,
                language_id,
                facts: Box::new(facts),
            },
            Err(err) => {
                let code = match &err {
                    BasicScanError::InvalidConfig(_) => WireErrorCode::ConfigInvalid,
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

fn basic_language_id() -> LanguageId {
    LanguageId::try_from("basic").expect("hardcoded basic id must be valid")
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
    fn valid_scan_response_keeps_request_and_snapshot() {
        let input = Cursor::new(
            "{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"basic\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-42\",\"config\":{}}\n",
        );
        let mut output = Vec::new();

        run_stdio_with_reader(input, &mut output).unwrap();

        let line = std::str::from_utf8(&output).unwrap().trim();
        let response: WireScanResponse = serde_json::from_str(line).unwrap();
        match response {
            WireScanResponse::ScanFacts {
                language_id, facts, ..
            } => {
                assert_eq!(language_id.as_str(), "basic");
                assert_eq!(facts.snapshot_id, "snap-42");
            }
            other => panic!("expected scan_facts response, got {other:?}"),
        }
    }
}
