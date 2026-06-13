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
                            symbols: result.symbols,
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
    let code = match &err {
        SwiftScanError::InvalidConfig(_) => WireErrorCode::ConfigInvalid,
        SwiftScanError::ParserInitFailed(_) => WireErrorCode::ParserInitFailed,
        _ => WireErrorCode::ScanFailed,
    };
    WirePackResponse::Error {
        api_version,
        language_id,
        code,
        message: err.to_string(),
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
        SwiftDiscoverError::ParseFailed(_) | SwiftDiscoverError::Io { .. } => {
            WireErrorCode::ScanFailed
        }
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
