use std::io::Write;
use std::process::{Command, Stdio};
use wax_lang_api::{WIRE_API_VERSION, WirePackResponse};

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
