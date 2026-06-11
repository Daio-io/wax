use std::io::Write;
use std::process::{Command, Stdio};

use wax_contract::ScanStatus;
use wax_lang_api::{WireErrorCode, WirePackResponse, WireScanResponse};

#[test]
fn discover_request_returns_discover_unsupported() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"discover","api_version":1,"language_id":"react","repo_root":"/tmp/repo","roots":["src"]}
"#,
            )
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::Error { code, .. } => {
            assert_eq!(code, WireErrorCode::DiscoverUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_emits_complete_scan_facts_for_small_fixture() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        let repo_root = format!("{}/tests/fixtures/small", env!("CARGO_MANIFEST_DIR"));
        let request = serde_json::json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "react",
            "repo_root": repo_root,
            "snapshot_id": "snap-cli",
            "config": {
                "design_system_registry": "design-system/registry.json",
                "roots": ["src"],
                "packages": {
                    "@acme/design-system": {
                        "exports": {
                            ".": "src/ds/index.ts"
                        }
                    }
                }
            }
        });
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
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
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(facts.snapshot_id, "snap-cli");
            assert_eq!(facts.status, ScanStatus::Complete);
            assert_eq!(facts.language.id.as_str(), "react");
            assert_eq!(facts.language.parser_name, "swc");
            assert_eq!(
                facts.language.parser_version,
                wax_lang_react::SWC_PARSER_VERSION,
                "parser_version must track the pinned SWC crate version"
            );
            assert_eq!(facts.counts.usage_site_count, 5);
            assert_eq!(facts.counts.resolved_count, 5);
            assert_eq!(facts.counts.design_system_component_count, 2);
            assert_eq!(facts.counts.local_component_count, 4);
            assert_eq!(facts.metrics.files_scanned, 6);
            assert!(
                facts
                    .usage_sites
                    .iter()
                    .any(|site| site.symbol == "PrimaryBtn"
                        && site.registry_symbol.as_deref() == Some("PrimaryButton")),
                "alias usage must resolve to PrimaryButton"
            );
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_emits_one_scan_facts_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"scan","api_version":1,"language_id":"react","repo_root":"/tmp/repo","snapshot_id":"snap-cli","config":{}}
"#,
            )
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
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
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(facts.language.id.as_str(), "react");
            assert_eq!(facts.snapshot_id, "snap-cli");
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_returns_partial_scan_facts_for_parse_failures() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let registry_dir = temp.path().join("design-system");
    std::fs::create_dir_all(&registry_dir).expect("registry dir should be created");
    std::fs::write(
        registry_dir.join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
    )
    .expect("registry fixture should be written");
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir should be created");
    std::fs::write(src_dir.join("App.tsx"), "export {}").expect("valid source fixture");
    std::fs::write(
        src_dir.join("Broken.tsx"),
        "export function Broken() { return <button><span></button>; }",
    )
    .expect("invalid source fixture");

    let request = serde_json::json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().to_string_lossy(),
        "snapshot_id": "snap-partial",
        "config": {
            "design_system_registry": "design-system/registry.json",
            "roots": ["src"]
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
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
            assert_eq!(language_id.as_str(), "react");
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
fn stdio_cli_returns_registry_not_found_for_missing_registry() {
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
            "design_system_registry": "design-system/registry.json",
            "roots": ["src"]
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WireScanResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WireScanResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::RegistryNotFound);
            assert!(message.contains("react registry not found"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}
