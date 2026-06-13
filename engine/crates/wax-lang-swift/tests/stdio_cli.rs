use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wax_contract::ScanStatus;
use wax_lang_api::{WIRE_API_VERSION, WireErrorCode, WirePackResponse};

fn run_stdio_request(request: &Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write stdio request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout must be valid UTF-8")
}

#[test]
fn stdio_scan_with_empty_config_returns_swift_scaffold_facts() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    let request = format!(
        "{{\"type\":\"scan\",\"api_version\":{WIRE_API_VERSION},\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-swift-scaffold\",\"config\":{{}}}}\n"
    );
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let response: WirePackResponse =
        serde_json::from_slice(&output.stdout).expect("parse response");
    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, WIRE_API_VERSION);
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(facts.language.id.as_str(), "swift");
            assert_eq!(facts.language.ecosystem, "swiftui");
            assert_eq!(facts.language.parser_name, "tree-sitter-swift");
            assert_eq!(facts.snapshot_id, "snap-swift-scaffold");
            assert_eq!(facts.counts.usage_site_count, 0);
            assert!(
                facts
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "swift_scaffold")
            );
        }
        other => panic!("expected scan facts, got {other:?}"),
    }
}

#[test]
fn stdio_discover_returns_symbols() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover");
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    let request = format!(
        "{{\"type\":\"discover\",\"api_version\":{WIRE_API_VERSION},\"language_id\":\"swift\",\"repo_root\":\"{}\",\"roots\":[\"design-system/Sources\"]}}\n",
        repo_root.display()
    );
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");

    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let response: WirePackResponse = serde_json::from_slice(&output.stdout).unwrap();
    match response {
        WirePackResponse::DiscoverSymbols {
            language_id,
            symbols,
            ..
        } => {
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(symbols, vec!["Badge", "PackageCard", "PrimaryButton"]);
        }
        other => panic!("expected discover symbols, got {other:?}"),
    }
}

#[test]
fn stdio_cli_emits_one_scan_facts_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        let repo_root = format!("{}/tests/fixtures/small", env!("CARGO_MANIFEST_DIR"));
        let request = json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "swift",
            "repo_root": repo_root,
            "snapshot_id": "snap-cli",
            "config": {
                "design_system_registry": "design-system/registry.json",
                "roots": ["app/Sources"]
            }
        });
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(facts.snapshot_id, "snap-cli");
            assert_eq!(facts.status, ScanStatus::Complete);
            assert_eq!(facts.language.parser_name, "tree-sitter-swift");
            assert_eq!(facts.counts.usage_site_count, 6);
            assert_eq!(facts.counts.resolved_count, 6);
            assert_eq!(facts.counts.design_system_component_count, 2);
            assert_eq!(facts.counts.local_component_count, 5);
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_scan_reports_partial_facts_for_parse_failure() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let registry_dir = temp.path().join("design-system");
    fs::create_dir_all(&registry_dir).expect("registry dir should be created");
    fs::write(
        registry_dir.join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton","targets":["swift"]}]}"#,
    )
    .expect("registry fixture should be written");
    let src_dir = temp.path().join("Sources/App");
    fs::create_dir_all(&src_dir).expect("src dir should be created");
    fs::write(
        src_dir.join("Valid.swift"),
        "import SwiftUI\nstruct ValidView: View { var body: some View { Text(\"ok\") } }\n",
    )
    .expect("valid source fixture");
    fs::write(src_dir.join("Broken.swift"), "struct BrokenView {")
        .expect("invalid source fixture");

    let request = json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "swift",
        "repo_root": temp.path().to_string_lossy(),
        "snapshot_id": "snap-partial",
        "config": {
            "design_system_registry": "design-system/registry.json",
            "roots": ["Sources/App"]
        }
    });

    let stdout = run_stdio_request(&request);
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(facts.snapshot_id, "snap-partial");
            assert_eq!(facts.status, ScanStatus::Partial);
            assert_eq!(facts.metrics.files_scanned, 2);
            assert!(
                facts
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "parse_failed"),
                "expected parse_failed diagnostic, got: {:?}",
                facts.diagnostics
            );
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_scan_missing_registry_returns_registry_not_found() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let src_dir = temp.path().join("Sources/App");
    fs::create_dir_all(&src_dir).expect("src dir should be created");
    fs::write(
        src_dir.join("Valid.swift"),
        "import SwiftUI\nstruct ValidView: View { var body: some View { Text(\"ok\") } }\n",
    )
    .expect("source fixture");

    let request = json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "swift",
        "repo_root": temp.path().to_string_lossy(),
        "snapshot_id": "snap-missing-registry",
        "config": {
            "design_system_registry": "design-system/registry.json",
            "roots": ["Sources/App"]
        }
    });

    let stdout = run_stdio_request(&request);
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::RegistryNotFound);
            assert!(message.contains("swift registry not found"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn unsupported_api_version_on_scan_returns_tagged_error_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"scan","api_version":2,"language_id":"swift","repo_root":"/tmp/repo","snapshot_id":"snap-bad-version","config":{}}
"#,
            )
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

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
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn unsupported_api_version_on_discover_returns_tagged_error_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"discover","api_version":2,"language_id":"swift","repo_root":"/tmp/repo","roots":["src"]}
"#,
            )
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

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
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn invalid_json_returns_tagged_error_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(b"{not json}\n")
            .expect("failed to write invalid request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

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
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_discover_missing_root_returns_config_invalid() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "swift",
        "repo_root": temp.path().display().to_string(),
        "roots": ["missing-subdir"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::ConfigInvalid);
            assert!(message.contains("discovery root does not exist"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn stdio_discover_wrong_language_id_returns_config_invalid() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "compose",
        "repo_root": temp.path().display().to_string(),
        "roots": ["src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::ConfigInvalid);
            assert!(message.contains("invalid swift language id"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn stdio_discover_parse_failure_returns_scan_failed() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "swift",
        "repo_root": repo_root.display().to_string(),
        "roots": ["broken/Sources"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::ScanFailed);
            assert!(message.contains("failed to parse"));
            assert!(message.contains("Broken.swift"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}
