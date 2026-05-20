use clap::Parser;
use std::io::{self, BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{WIRE_API_VERSION, WireErrorCode, WireScanRequest, WireScanResponse};
use wax_lang_compose::ComposeLanguage;

#[derive(Debug, Parser)]
#[command(name = "wax-lang-compose")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if !cli.stdio {
        eprintln!("usage: wax-lang-compose --stdio");
        std::process::exit(2);
    }

    run_stdio()
}

fn run_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line_result in stdin.lock().lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let request: WireScanRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(_) => continue,
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
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
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

        let compose = ComposeLanguage::new();
        let response = match compose.scan(&scan_request) {
            Ok(facts) => WireScanResponse::ScanFacts {
                api_version,
                language_id,
                facts: Box::new(facts),
            },
            Err(err) => {
                let compose_id =
                    LanguageId::try_from("compose").expect("hardcoded compose id must be valid");
                WireScanResponse::Error {
                    api_version,
                    language_id: compose_id,
                    code: WireErrorCode::ScanFailed,
                    message: err.to_string(),
                    diagnostics: Vec::new(),
                }
            }
        };

        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        return Ok(());
    }

    Ok(())
}
