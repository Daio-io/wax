use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wax_lang_api::{WirePackResponse, WireScanResponse};

fn run_stdio_request(request: &Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-compose"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-compose");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        let input = format!("{}\n", request);
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write stdio request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-compose exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout must be valid UTF-8")
}

#[test]
fn stdio_cli_emits_one_scan_facts_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-compose"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-compose");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        let repo_root = format!("{}/tests/fixtures/small", env!("CARGO_MANIFEST_DIR"));
        let input = format!(
            "{{\"type\":\"scan\",\"api_version\":1,\"language_id\":\"compose\",\"repo_root\":\"{repo_root}\",\"snapshot_id\":\"snap-cli\",\"config\":{{\"design_system_registry\":\"design-system/registry.json\",\"roots\":[\"app/src/main/kotlin\"]}}}}\n"
        );
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-compose exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WireScanResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WireScanResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "compose");
            assert_eq!(facts.snapshot_id, "snap-cli");
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_emits_discover_symbols_for_fixture_roots() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");

    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "compose",
        "repo_root": repo_root.display().to_string(),
        "roots": ["design-system/src/main/kotlin"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols { symbols, .. } => {
            assert!(symbols.contains(&"PrimaryButton".to_owned()));
            assert!(!symbols.contains(&"PrivateButton".to_owned()));
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}
