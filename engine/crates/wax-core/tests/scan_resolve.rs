use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, LanguageId, LanguageMetadata, MergedScan, Metrics, SCHEMA_VERSION, ScanFacts,
    ScanStatus,
};
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

fn write_repo_files(repo: &Path, registry_file: &Path) {
    write_repo_files_with_source(repo, &format!("file://{}", registry_file.display()));
}

fn write_repo_files_with_source(repo: &Path, source: &str) {
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

    write_lockfile_with_source(repo, source);
}

fn write_lockfile_with_source(repo: &Path, source: &str) {
    let lock = format!(
        r#"{{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "languages": {{
    "compose": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}
  }}
}}"#,
        source
    );
    fs::write(repo.join("wax.lock.json"), lock).unwrap();
}

fn write_pack_index(path: &Path) {
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
    let _guard = env_lock();
    let root = temp_dir("scan-resolve");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file);

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    let script = install_dir.join("compose-pack.sh");
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

    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./compose-pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}"#,
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let merged: MergedScan = Engine::scan_repo(&repo).expect("scan should pass");

    assert_eq!(merged.schema_version, SCHEMA_VERSION);
    assert_eq!(merged.languages.len(), 1);
    let compose_id = LanguageId::from_str("compose").unwrap();
    assert!(merged.languages.contains_key(&compose_id));
    assert_eq!(merged.languages[&compose_id].language.id, compose_id);
}

#[test]
fn scan_resolve_forwards_enabled_language_config_to_pack_request() {
    let _guard = env_lock();
    let root = temp_dir("scan-resolve-config");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_lockfile_with_source(&repo, &format!("file://{}", registry_file.display()));
    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "design_system_registry": "design-system.json",
      "roots": ["app/src", "design-system/src"]
    },
    {
      "id": "react",
      "enabled": false,
      "design_system_registry": "react-registry.json",
      "roots": ["web/src"]
    }
  ]
}"#,
    )
    .unwrap();

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    let captured_request = root.join("captured-request.json");
    let script = install_dir.join("compose-pack.sh");
    let wire = serde_json::json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": "compose",
        "facts": build_scan_facts("compose", "0.1.0")
    });
    let script_body = format!(
        "#!/bin/sh\nset -eu\ncat > \"$1\"\nprintf '%s\\n' '{}'\n",
        wire
    );
    fs::write(&script, script_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }

    let manifest = serde_json::json!({
        "id": "compose",
        "version": "0.1.0",
        "api_version": 1,
        "command": ["./compose-pack.sh", captured_request],
        "target": "x86_64-unknown-linux-gnu",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "ecosystem": "test",
        "parser_name": "fixture",
        "parser_version": "1.0.0"
    });
    fs::write(
        install_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    Engine::scan_repo(&repo).expect("scan should pass");

    let request: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(captured_request).unwrap()).unwrap();
    assert_eq!(request["type"], serde_json::json!("scan"));
    assert!(request["config"].get("id").is_none());
    assert!(request["config"].get("enabled").is_none());
    assert_eq!(
        request["config"]["design_system_registry"],
        serde_json::json!("design-system.json")
    );
    assert_eq!(
        request["config"]["roots"],
        serde_json::json!(["app/src", "design-system/src"])
    );
}

#[test]
fn scan_resolve_surfaces_missing_install_as_auto_install_required() {
    let _guard = env_lock();
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let err = Engine::scan_repo_with_options(
        &repo,
        ScanOptions {
            scan_concurrency: None,
            allow_auto_install: false,
        },
    )
    .expect_err("missing pack should be policy-blocked");
    let message = err.to_string();
    assert!(
        message.contains("run `wax language install`"),
        "unexpected error: {message}"
    );
}

#[test]
fn scan_resolve_ready_install_does_not_require_registry_access() {
    let _guard = env_lock();
    let root = temp_dir("scan-resolve-ready-no-registry");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    write_repo_files_with_source(&repo, "https://registry.example.invalid/index.json");

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    let script = install_dir.join("compose-pack.sh");
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

    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./compose-pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}"#,
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let merged: MergedScan = Engine::scan_repo(&repo).expect("ready install should scan");
    assert_eq!(merged.languages.len(), 1);
}

#[test]
fn scan_resolve_fetches_registry_only_for_missing_languages() {
    let _guard = env_lock();
    let root = temp_dir("scan-resolve-mixed-registry");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    { "id": "compose", "enabled": true },
    { "id": "react", "enabled": true }
  ]
}"#,
    )
    .unwrap();

    let react_registry = root.join("react-registry.json");
    fs::write(
        &react_registry,
        r#"[
  {
    "id": "react",
    "version": "1.0.0",
    "api_version": 1,
    "targets": {
      "x86_64-unknown-linux-gnu": {
        "url": "https://example.invalid/react-1.0.0.tgz",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      }
    }
  }
]"#,
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
      "source": "https://registry.example.invalid/compose.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }},
    "react": {{
      "version": "1.0.0",
      "api_version": 1,
      "source": "file://{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/react-1.0.0.tgz",
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "signature": null
      }}
    }}
  }}
}}"#,
        react_registry.display()
    );
    fs::write(repo.join("wax.lock.json"), lock).unwrap();

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    let script = install_dir.join("compose-pack.sh");
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
    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./compose-pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}"#,
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let err = Engine::scan_repo(&repo).expect_err("react digest drift should block scan");
    let EngineError::AutoInstallPolicyBlocked { errors } = err else {
        panic!("expected digest drift policy block, got {err:?}");
    };
    assert!(matches!(
        errors.as_slice(),
        [wax_core::auto_install::AutoInstallPolicyError::DigestDrift { language_id, .. }]
            if language_id.as_str() == "react"
    ));
}

#[test]
fn scan_resolve_no_auto_install_validates_missing_pack_index_before_required_error() {
    let _guard = env_lock();
    let root = temp_dir("scan-resolve-no-auto-install-validates-index");
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

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let err = Engine::scan_repo_with_options(
        &repo,
        ScanOptions {
            scan_concurrency: None,
            allow_auto_install: false,
        },
    )
    .expect_err("missing pack should require install after index validation");
    assert!(matches!(err, EngineError::AutoInstallRequired { .. }));
}

#[test]
fn scan_resolve_ignores_stale_installed_versions() {
    let _guard = env_lock();
    let root = temp_dir("scan-resolve-stale-installed");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file);

    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).unwrap();
    let script = install_dir.join("compose-pack.sh");
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
    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./compose-pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}"#,
    )
    .unwrap();

    let stale_install_dir = wax_home.join("langs/compose/0.0.1");
    fs::create_dir_all(&stale_install_dir).unwrap();

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "compose": {{
      "0.0.1": {{ "install_dir": "{}" }},
      "0.1.0": {{ "install_dir": "{}" }}
    }}
  }}
}}"#,
            stale_install_dir.display(),
            install_dir.display()
        ),
    )
    .unwrap();

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let merged: MergedScan = Engine::scan_repo(&repo).expect("locked install should scan");
    assert_eq!(merged.languages.len(), 1);
}
