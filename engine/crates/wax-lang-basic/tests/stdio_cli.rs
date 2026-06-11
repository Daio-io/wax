use std::io::Write;
use std::process::{Command, Stdio};

use wax_lang_api::{WireErrorCode, WirePackResponse};

#[test]
fn stdio_cli_returns_discover_unsupported_for_discover_request() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-basic"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-basic");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"discover","api_version":1,"language_id":"basic","repo_root":"/tmp/repo","roots":["src"]}
"#,
            )
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-basic exited with {:?}; stderr: {}",
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
fn stdio_cli_returns_api_version_unsupported_for_bad_discover_version() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-basic"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-basic");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"discover","api_version":2,"language_id":"basic","repo_root":"/tmp/repo","roots":["src"]}
"#,
            )
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-basic exited with {:?}; stderr: {}",
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
            assert_eq!(language_id.as_str(), "basic");
            assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}
