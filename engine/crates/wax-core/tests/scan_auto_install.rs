use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_core::paths::WaxHome;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use wax_contract::LanguageId;
use wax_core::config::lockfile::{load_lockfile, save_lockfile};
use wax_core::global_state::GlobalState;
use wax_core::{Engine, EngineError, ScanOptions};

struct Fixture {
    repo: PathBuf,
    wax_home: PathBuf,
    digest: String,
}

fn fixture(name: &str) -> Fixture {
    fixture_with_registry_url(name, None)
}

fn fixture_with_registry_url(name: &str, registry_artifact_url: Option<&str>) -> Fixture {
    let root = temp_dir(name);
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(repo.join(".wax")).unwrap();

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", fixture_script().as_bytes(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);
    let artifact_path = root.join("compose-0.1.0.tgz");
    fs::write(&artifact_path, artifact_bytes).unwrap();

    let registry_path = root.join("registry.json");
    let artifact_url = format!("file://{}", artifact_path.display());
    let registry_artifact_url = registry_artifact_url.unwrap_or(&artifact_url);
    fs::write(
        &registry_path,
        format!(
            r#"[
  {{
    "id": "compose",
    "version": "0.1.0",
    "api_version": 1,
    "targets": {{
      "x86_64-unknown-linux-gnu": {{
        "url": "{registry_artifact_url}",
        "sha256": "{digest}"
      }}
    }}
  }}
]"#
        ),
    )
    .unwrap();

    let design_registry =
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#;
    fs::write(repo.join(".wax/compose.registry.json"), design_registry).unwrap();
    let design_registry_sha256 = sha256_hex(design_registry.as_bytes());

    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {"compose": {}}
}"#,
    )
    .unwrap();

    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "registries": {{
    "compose": {{
      "source": ".wax/compose.registry.json",
      "sha256": "{design_registry_sha256}"
    }}
  }},
  "languages": {{
    "compose": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file://{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "{artifact_url}",
        "sha256": "{digest}",
        "signature": null
      }}
    }}
  }}
}}"#,
            registry_path.display()
        ),
    )
    .unwrap();

    Fixture {
        repo,
        wax_home,
        digest,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn gzip_tar(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let gz = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = tar::Builder::new(gz);
        for (path, body, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(body.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            tar.append(&header, *body).unwrap();
        }
        tar.finish().unwrap();
    }
    buffer
}

fn fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"compose","facts":{"schema_version":4,"language":{"id":"compose","version":"0.1.0","ecosystem":"compose","parser_name":"compose","parser_version":"0.1.0"},"snapshot_id":"snap-compose","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":0,"files_scanned":0},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0}}}}
JSON
"#
}

fn read_state(path: &Path) -> GlobalState {
    let raw = fs::read_to_string(path).expect("state file should exist");
    serde_json::from_str(&raw).expect("state file should parse")
}

#[test]
fn scan_auto_install_uses_explicit_home_for_state_and_pack() {
    let fixture = fixture("scan-auto-install");
    let merged = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            wax_home: Some(WaxHome::new(&fixture.wax_home)),
            ..ScanOptions::default()
        },
    )
    .expect("default scan should auto-install");

    let compose = LanguageId::try_from("compose").unwrap();
    assert_eq!(merged.languages[&compose].language.id, compose);

    let state = read_state(&fixture.wax_home.join("state.json"));
    let installed = &state.installed_languages[&compose]["0.1.0"];
    assert_eq!(
        installed.install_dir,
        fixture.wax_home.join("langs/compose/0.1.0")
    );
    assert!(installed.install_dir.join("manifest.json").exists());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(installed.install_dir.join("manifest.json")).unwrap()
        )
        .unwrap()["sha256"],
        fixture.digest
    );
}

#[test]
fn scan_auto_install_recovers_leftover_pack_directory() {
    let fixture = fixture("scan-auto-install-reuse");
    let options = || ScanOptions {
        wax_home: Some(WaxHome::new(&fixture.wax_home)),
        ..ScanOptions::default()
    };
    Engine::scan_repo_with_options(&fixture.repo, options()).unwrap();

    let state_path = fixture.wax_home.join("state.json");
    fs::remove_file(&state_path).unwrap();
    fs::remove_file(fixture.repo.with_file_name("compose-0.1.0.tgz")).unwrap();

    let merged = Engine::scan_repo_with_options(&fixture.repo, options()).unwrap();

    let compose = LanguageId::try_from("compose").unwrap();
    assert_eq!(merged.languages[&compose].language.id, compose);
    let state = read_state(&state_path);
    assert_eq!(
        state.installed_languages[&compose]["0.1.0"].install_dir,
        fixture.wax_home.join("langs/compose/0.1.0")
    );
}

#[test]
fn scan_auto_install_rejects_leftover_pack_with_different_digest() {
    let fixture = fixture("scan-auto-install-reused-drift");
    let options = || ScanOptions {
        wax_home: Some(WaxHome::new(&fixture.wax_home)),
        ..ScanOptions::default()
    };
    Engine::scan_repo_with_options(&fixture.repo, options()).unwrap();

    fs::remove_file(fixture.wax_home.join("state.json")).unwrap();
    let manifest_path = fixture.wax_home.join("langs/compose/0.1.0/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["sha256"] = "0".repeat(64).into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let error = Engine::scan_repo_with_options(&fixture.repo, options())
        .expect_err("reused pack must match the lockfile pin");

    assert!(matches!(
        error,
        EngineError::InstalledArtifactMismatch { .. }
    ));
}

#[test]
fn scan_auto_install_disabled_returns_required_error_without_installing() {
    let fixture = fixture("scan-auto-install-disabled");

    let err = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            wax_home: Some(WaxHome::new(&fixture.wax_home)),
            scan_concurrency: None,
            allow_auto_install: false,
            ..ScanOptions::default()
        },
    )
    .expect_err("disabled auto-install should fail fast");

    assert!(matches!(err, EngineError::AutoInstallRequired { .. }));
    assert!(!fixture.wax_home.join("state.json").exists());
}

#[test]
fn scan_auto_install_disabled_does_not_fetch_registry_index() {
    let fixture = fixture("scan-auto-install-disabled-offline");

    let lockfile_path = fixture.repo.join(".wax/wax.lock.json");
    let mut lockfile = load_lockfile(&lockfile_path).unwrap();
    lockfile.languages.get_mut("compose").unwrap().source =
        "https://example.invalid/unreachable-index.json".to_owned();
    save_lockfile(&lockfile_path, &lockfile).unwrap();

    let err = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            wax_home: Some(WaxHome::new(&fixture.wax_home)),
            scan_concurrency: None,
            allow_auto_install: false,
            ..ScanOptions::default()
        },
    )
    .expect_err("disabled auto-install should fail without registry fetch");

    assert!(
        matches!(err, EngineError::AutoInstallRequired { .. }),
        "expected AutoInstallRequired, got: {err}"
    );
}

#[test]
fn scan_auto_install_downloads_from_lockfile_url_not_registry_url() {
    let fixture = fixture_with_registry_url(
        "scan-auto-install-lock-url",
        Some("https://example.invalid/registry-url-must-not-be-fetched.tgz"),
    );

    let merged = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            wax_home: Some(WaxHome::new(&fixture.wax_home)),
            ..ScanOptions::default()
        },
    )
    .expect("scan should install from lockfile URL and pass");

    let compose = LanguageId::try_from("compose").unwrap();
    assert_eq!(merged.languages[&compose].language.id, compose);
}
