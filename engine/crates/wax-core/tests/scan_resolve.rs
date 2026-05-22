use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, LanguageId, LanguageMetadata, MergedScan, Metrics, SCHEMA_VERSION, ScanFacts,
    ScanStatus,
};
use wax_core::Engine;

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn build_scan_facts(language: &str, version: &str) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::from_str(language).unwrap(),
            version: version.to_owned(),
            ecosystem: "test".to_owned(),
            parser_name: "fixture".to_owned(),
            parser_version: "1.0.0".to_owned(),
        },
        snapshot_id: "snap-1".to_owned(),
        scanned_at: OffsetDateTime::UNIX_EPOCH,
        status: ScanStatus::Complete,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: Vec::new(),
        metrics: Metrics {
            parse_extract_ms: 0,
            files_scanned: 0,
            adoption_coverage_ratio: None,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
        },
    }
}

fn write_repo_files(repo: &PathBuf, registry_file: &PathBuf) {
    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    { "id": "compose", "enabled": true },
    { "id": "react", "enabled": false }
  ]
}"#,
    )
    .unwrap();

    let lock = format!(
        r#"{{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "languages": {{
    "compose": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file://{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}
  }}
}}"#,
        registry_file.display()
    );
    fs::write(repo.join("wax.lock.json"), lock).unwrap();
}

fn write_pack_index(path: &PathBuf) {
    fs::write(
        path,
        r#"[
  {
    "id": "compose",
    "version": "0.1.0",
    "api_version": 1,
    "targets": {
      "x86_64-unknown-linux-gnu": {
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }
    }
  }
]"#,
    )
    .unwrap();
}

#[test]
fn scan_resolve_runs_enabled_language_and_merges_results() {
    let root = temp_dir("scan-resolve");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file);

    let script = root.join("compose-pack.sh");
    let wire = serde_json::json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": "compose",
        "facts": build_scan_facts("compose", "0.1.0")
    });
    let script_body = format!("#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{}'\n", wire);
    fs::write(&script, script_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    fs::write(
        install_dir.join("manifest.json"),
        format!(
            r#"{{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["{}"],
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#,
            script.display()
        ),
    )
    .unwrap();

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "compose": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}
  }}
}}"#,
            install_dir.display()
        ),
    )
    .unwrap();

    // SAFETY: tests run in-process and intentionally scope WAX_HOME per test case.
    unsafe { std::env::set_var("WAX_HOME", &wax_home) };
    let merged: MergedScan = Engine::scan_repo(&repo).expect("scan should pass");

    assert_eq!(merged.schema_version, SCHEMA_VERSION);
    assert_eq!(merged.languages.len(), 1);
    let compose_id = LanguageId::from_str("compose").unwrap();
    assert!(merged.languages.contains_key(&compose_id));
    assert_eq!(merged.languages[&compose_id].language.id, compose_id);
}

#[test]
fn scan_resolve_surfaces_missing_install_as_auto_install_required() {
    let root = temp_dir("scan-resolve-missing");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file);
    fs::write(
        wax_home.join("state.json"),
        "{\"installed_languages\":{}}\n",
    )
    .unwrap();

    // SAFETY: tests run in-process and intentionally scope WAX_HOME per test case.
    unsafe { std::env::set_var("WAX_HOME", &wax_home) };
    let err = Engine::scan_repo(&repo).expect_err("missing pack should be policy-blocked");
    let message = err.to_string();
    assert!(
        message.contains("requires auto-install"),
        "unexpected error: {message}"
    );
}
