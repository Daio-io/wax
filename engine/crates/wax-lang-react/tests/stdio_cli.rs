use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wax_contract::ScanStatus;
use wax_lang_api::{WireErrorCode, WirePackResponse};

#[test]
fn stdio_cli_emits_discover_symbols_for_fixture_roots() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo_root.display().to_string(),
        "roots": ["design-system/src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols { symbols, .. } => {
            assert!(symbols.contains(&"Button".to_owned()));
            assert!(symbols.contains(&"InlineMemo".to_owned()));
            assert!(symbols.contains(&"InlineRef".to_owned()));
            assert!(symbols.contains(&"MemoButton".to_owned()));
            assert!(!symbols.contains(&"PrivateBadge".to_owned()));
            assert!(!symbols.contains(&"lowerBadge".to_owned()));
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}

#[test]
fn stdio_cli_returns_config_invalid_for_missing_discover_root() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
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
fn stdio_cli_returns_config_invalid_for_bad_discover_language_id() {
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
            assert!(message.contains("invalid react language id"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn stdio_cli_returns_discover_symbols_with_parse_failure_diagnostic() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let src = temp.path().join("src");
    fs::create_dir_all(&src).expect("src dir should be created");
    fs::write(src.join("Broken.tsx"), "export const Broken = (")
        .expect("broken fixture should be written");

    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().display().to_string(),
        "roots": ["src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols {
            symbols,
            diagnostics,
            ..
        } => {
            assert!(symbols.is_empty());
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code, "parse_failed");
            assert_eq!(
                diagnostics[0]
                    .location
                    .as_ref()
                    .map(|location| location.file.as_str()),
                Some("src/Broken.tsx")
            );
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}

fn run_stdio_request(request: &Value) -> String {
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
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout must be valid UTF-8")
}

#[test]
fn stdio_cli_returns_api_version_unsupported_for_bad_discover_version() {
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
                br#"{"type":"discover","api_version":2,"language_id":"react","repo_root":"/tmp/repo","roots":["src"]}
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
                "registry": "design-system/registry.json",
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
            assert_eq!(facts.counts.raw_invocations.total, 5);
            assert_eq!(facts.counts.raw_invocations.resolved, 5);
            assert_eq!(facts.counts.registry.component_count, 2);
            assert_eq!(facts.counts.definitions.local_definition_count, 4);
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
            "registry": "design-system/registry.json",
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
            "registry": "design-system/registry.json",
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
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::RegistryNotFound);
            assert!(message.contains("react registry not found"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}
