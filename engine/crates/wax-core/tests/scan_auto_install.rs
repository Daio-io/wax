use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use wax_contract::LanguageId;
use wax_core::global_state::GlobalState;
use wax_core::{Engine, EngineError, ScanOptions};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

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

    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    { "id": "compose", "enabled": true }
  ]
}"#,
    )
    .unwrap();

    fs::write(
        repo.join("wax.lock.json"),
        format!(
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
{"type":"scan_facts","api_version":1,"language_id":"compose","facts":{"schema_version":1,"language":{"id":"compose","version":"0.1.0","ecosystem":"compose","parser_name":"compose","parser_version":"0.1.0"},"snapshot_id":"snap-compose","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"parse_extract_ms":0,"files_scanned":0,"adoption_coverage_ratio":null},"counts":{"design_system_component_count":0,"local_component_count":0,"usage_site_count":0,"resolved_count":0,"candidate_count":0}}}
JSON
"#
}

fn read_state(path: &Path) -> GlobalState {
    let raw = fs::read_to_string(path).expect("state file should exist");
    serde_json::from_str(&raw).expect("state file should parse")
}

#[test]
fn scan_auto_install_installs_missing_pack_then_runs_scan() {
    let _guard = env_lock();
    let fixture = fixture("scan-auto-install");
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    let merged = Engine::scan_repo(&fixture.repo).expect("default scan should auto-install");

    let compose = LanguageId::try_from("compose").unwrap();
    assert_eq!(merged.languages[&compose].language.id, compose);

    let state = read_state(&fixture.wax_home.join("state.json"));
    let installed = &state.installed_languages[&compose]["0.1.0"];
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
fn scan_auto_install_disabled_returns_required_error_without_installing() {
    let _guard = env_lock();
    let fixture = fixture("scan-auto-install-disabled");
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    let err = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            scan_concurrency: None,
            allow_auto_install: false,
        },
    )
    .expect_err("disabled auto-install should fail fast");

    assert!(matches!(err, EngineError::AutoInstallRequired { .. }));
    assert!(!fixture.wax_home.join("state.json").exists());
}

#[test]
fn scan_auto_install_disabled_does_not_fetch_registry_index() {
    let _guard = env_lock();
    let fixture = fixture("scan-auto-install-disabled-offline");
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    let lockfile_path = fixture.repo.join("wax.lock.json");
    let mut lockfile: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&lockfile_path).unwrap()).unwrap();
    lockfile["languages"]["compose"]["source"] =
        serde_json::json!("https://example.invalid/unreachable-index.json");
    fs::write(
        &lockfile_path,
        format!("{}\n", serde_json::to_string_pretty(&lockfile).unwrap()),
    )
    .unwrap();

    let err = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            scan_concurrency: None,
            allow_auto_install: false,
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
    let _guard = env_lock();
    let fixture = fixture_with_registry_url(
        "scan-auto-install-lock-url",
        Some("https://example.invalid/registry-url-must-not-be-fetched.tgz"),
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    let merged =
        Engine::scan_repo(&fixture.repo).expect("scan should install from lockfile URL and pass");

    let compose = LanguageId::try_from("compose").unwrap();
    assert_eq!(merged.languages[&compose].language.id, compose);
}
