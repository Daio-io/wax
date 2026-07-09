#![cfg(unix)]

use serde_json::{Map, Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use time::macros::datetime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanStatus, SourceLocation,
};
use wax_core::subprocess_lang::{
    LanguageError, LanguageExtractor, SubprocessLanguageExtractor, SubprocessLanguageManifest,
};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION, WireErrorCode};

#[test]
fn subprocess_protocol_parses_tagged_scan_facts_stdout() {
    let temp_dir = TestDir::new("protocol-success");
    let request_path = temp_dir.path().join("request.json");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let expected = sample_scan_facts();
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat > "$1"
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "scan_facts",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "facts": expected,
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            request_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });

    let facts = extractor.scan(sample_request()).unwrap();

    assert_eq!(facts, expected);
    let request_json: Value =
        serde_json::from_str(&fs::read_to_string(request_path).unwrap()).unwrap();
    assert_eq!(
        request_json,
        json!({
            "type": "scan",
            "api_version": WIRE_API_VERSION,
            "language_id": "compose",
            "repo_root": "/repo/root",
            "snapshot_id": "snap-123",
            "config": {"strict": true},
        })
    );
}

#[test]
fn subprocess_protocol_maps_structured_error_stdout_to_pack_failure() {
    let temp_dir = TestDir::new("protocol-wire-error");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let expected_diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: "scan_failed".to_owned(),
        message: "Button import could not be resolved".to_owned(),
        location: Some(SourceLocation {
            file: "src/app.tsx".to_owned(),
            line: 12,
            column: Some(7),
        }),
    };
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "error",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "code": "scan_failed",
                "message": "pack failed to scan",
                "diagnostics": [expected_diagnostic],
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::Wire {
            code,
            message,
            diagnostics,
        } if code == WireErrorCode::ScanFailed
            && message == "pack failed to scan"
            && diagnostics == vec![expected_diagnostic]
    ));
}

#[test]
fn subprocess_protocol_maps_wire_timeout_error() {
    let temp_dir = TestDir::new("protocol-wire-timeout");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "error",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "code": "timeout",
                "message": "scan took too long",
                "diagnostics": [],
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::WireTimeout {
            message,
            diagnostics
        } if message == "scan took too long" && diagnostics.is_empty()
    ));
}

#[test]
fn subprocess_protocol_rejects_unsupported_api_version() {
    let temp_dir = TestDir::new("protocol-unsupported-api-version");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "scan_facts",
                "api_version": 99,
                "language_id": "compose",
                "facts": {
                    "future_schema": true,
                },
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::UnsupportedApiVersion {
            found: 99,
            supported: WIRE_API_VERSION
        }
    ));
}

#[test]
fn subprocess_protocol_reports_malformed_stdout_as_protocol_failure() {
    let temp_dir = TestDir::new("protocol-malformed-stdout");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        r#"#!/bin/sh
cat >/dev/null
echo 'not json'
echo 'parser exploded' 1>&2
exit 7
"#,
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::ProcessFailed {
            stderr,
            ..
        } if stderr == "parser exploded"
    ));
}

#[test]
fn subprocess_protocol_accepts_large_success_stdout_without_fixed_cap() {
    let temp_dir = TestDir::new("protocol-large-stdout");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let expected = large_scan_facts();
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "scan_facts",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "facts": expected,
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let facts = extractor.scan(sample_request()).unwrap();

    assert_eq!(facts.snapshot_id, expected.snapshot_id);
    assert!(facts.snapshot_id.len() > 2 * 1024 * 1024);
}

#[test]
fn subprocess_protocol_maps_elapsed_timeout_to_language_timeout() {
    let temp_dir = TestDir::new("protocol-elapsed-timeout");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        r#"#!/bin/sh
cat >/dev/null
sleep 5
"#,
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_millis(25),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::Timeout {
            timeout
        } if timeout == Duration::from_millis(25)
    ));
}

fn sample_request() -> ScanRequest {
    ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::from_str("compose").unwrap(),
        repo_root: "/repo/root".to_owned(),
        snapshot_id: "snap-123".to_owned(),
        config: Map::from_iter([(String::from("strict"), Value::Bool(true))]),
    }
}

fn sample_scan_facts() -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::from_str("compose").unwrap(),
            version: "0.0.0".to_owned(),
            ecosystem: "jetpack-compose".to_owned(),
            parser_name: "tree-sitter-kotlin".to_owned(),
            parser_version: "0.3.8".to_owned(),
        },
        snapshot_id: "snap-123".to_owned(),
        scanned_at: datetime!(2026-05-16 12:00 UTC),
        status: ScanStatus::Complete,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![],
        diagnostics: vec![],
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            token_reference_ratio: None,
            parse_extract_ms: 12,
            files_scanned: 1,
        },
        counts: CountSummary::default(),
        symbol_usage_summary: vec![],
        design_system_tokens: vec![],
        token_sites: vec![],
        hardcoded_style_sites: vec![],
        token_usage_summary: vec![],
    }
}

fn large_scan_facts() -> ScanFacts {
    let mut facts = sample_scan_facts();
    facts.snapshot_id = format!("snap-{}", "x".repeat(2 * 1024 * 1024));
    facts
}

fn write_script(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("wax-core-subprocess-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
