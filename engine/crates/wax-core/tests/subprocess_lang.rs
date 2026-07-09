#![cfg(unix)]

use serde_json::{Map, Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use time::macros::datetime;
use wax_contract::{
    CountSummary, LanguageId, LanguageMetadata, Metrics, SCHEMA_VERSION, ScanFacts, ScanStatus,
};
use wax_core::subprocess_lang::{
    LanguageCancellationToken, LanguageError, LanguageExtractor, SubprocessLanguageExtractor,
    SubprocessLanguageManifest,
};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};

#[test]
fn subprocess_timeout_covers_inherited_stdout_after_child_exit() {
    let temp_dir = TestDir::new("inherited-stdout");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
(sleep 5) &
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "scan_facts",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "facts": sample_scan_facts(),
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_millis(50),
    });

    let err = extractor.scan(sample_request()).unwrap_err();

    assert!(matches!(
        err,
        LanguageError::Timeout {
            timeout
        } if timeout == Duration::from_millis(50)
    ));
}

#[test]
fn subprocess_drains_stderr_while_writing_stdin() {
    let temp_dir = TestDir::new("stderr-before-stdin");
    let script_path = temp_dir.path().join("mock-pack.sh");
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
dd if=/dev/zero bs=1024 count=512 1>&2 2>/dev/null
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "scan_facts",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "facts": sample_scan_facts(),
            }))
            .unwrap()
        ),
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![script_path.to_string_lossy().into_owned()],
        timeout: Duration::from_secs(5),
    });

    let facts = extractor.scan(large_sample_request()).unwrap();

    assert_eq!(facts.snapshot_id, "snap-123");
}

#[test]
fn subprocess_maps_engine_cancellation_through_trait_object() {
    let temp_dir = TestDir::new("cancelled");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let pid_path = temp_dir.path().join("pid");
    write_script(
        &script_path,
        r#"#!/bin/sh
echo "$$" > "$1"
cat >/dev/null
sleep 5
"#,
    );

    let extractor: Box<dyn LanguageExtractor + Send> = Box::new(SubprocessLanguageExtractor::new(
        SubprocessLanguageManifest {
            command: vec![
                script_path.to_string_lossy().into_owned(),
                pid_path.to_string_lossy().into_owned(),
            ],
            timeout: Duration::from_secs(5),
        },
    ));
    let cancellation = LanguageCancellationToken::new();
    let scan_cancellation = cancellation.clone();
    let scan_thread = std::thread::spawn(move || {
        extractor.scan_with_cancellation(sample_request(), &scan_cancellation)
    });

    wait_for_file(&pid_path);
    cancellation.cancel();

    let err = scan_thread.join().unwrap().unwrap_err();

    assert!(matches!(err, LanguageError::Cancelled));
    assert_process_exited(fs::read_to_string(pid_path).unwrap().trim());
}

#[test]
fn concrete_subprocess_extractor_can_be_cancelled() {
    let temp_dir = TestDir::new("concrete-cancelled");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let pid_path = temp_dir.path().join("pid");
    write_script(
        &script_path,
        r#"#!/bin/sh
echo "$$" > "$1"
cat >/dev/null
sleep 5
"#,
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            pid_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });
    let cancellation = LanguageCancellationToken::new();
    let scan_cancellation = cancellation.clone();
    let scan_thread = std::thread::spawn(move || {
        extractor.scan_with_cancellation(sample_request(), &scan_cancellation)
    });

    wait_for_file(&pid_path);
    cancellation.cancel();

    let err = scan_thread.join().unwrap().unwrap_err();

    assert!(matches!(err, LanguageError::Cancelled));
    assert_process_exited(fs::read_to_string(pid_path).unwrap().trim());
}

#[test]
fn subprocess_cancellation_sends_sigterm_before_sigkill() {
    let temp_dir = TestDir::new("sigterm-before-sigkill");
    let script_path = temp_dir.path().join("mock-pack.sh");
    let pid_path = temp_dir.path().join("pid");
    let term_path = temp_dir.path().join("terminated");
    write_script(
        &script_path,
        r#"#!/bin/sh
trap 'echo term > "$2"; exit 0' TERM
echo "$$" > "$1"
cat >/dev/null
sleep 5
"#,
    );

    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            pid_path.to_string_lossy().into_owned(),
            term_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });
    let cancellation = LanguageCancellationToken::new();
    let scan_cancellation = cancellation.clone();
    let scan_thread = std::thread::spawn(move || {
        extractor.scan_with_cancellation(sample_request(), &scan_cancellation)
    });

    wait_for_file(&pid_path);
    cancellation.cancel();

    let err = scan_thread.join().unwrap().unwrap_err();

    assert!(matches!(err, LanguageError::Cancelled));
    assert_eq!(fs::read_to_string(term_path).unwrap().trim(), "term");
    assert_process_exited(fs::read_to_string(pid_path).unwrap().trim());
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

fn large_sample_request() -> ScanRequest {
    let mut request = sample_request();
    request.config.insert(
        "payload".to_owned(),
        Value::String("x".repeat(2 * 1024 * 1024)),
    );
    request
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

fn wait_for_file(path: &Path) {
    for _ in 0..500 {
        if path.metadata().is_ok_and(|metadata| metadata.len() > 0) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn assert_process_exited(pid: &str) {
    let pid = pid.parse::<i32>().unwrap();
    for _ in 0..100 {
        let is_running = unsafe { libc::kill(pid, 0) == 0 };
        if !is_running {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("process {pid} was still running after cancellation");
}
