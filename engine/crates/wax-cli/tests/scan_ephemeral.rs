use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_cli::commands::scan::{
    EphemeralScanSelections, ScanCommandOptions, repo_relative_dir_has_entries,
    repo_relative_path_exists, run_scan_cli,
};
use wax_contract::LanguageId;
use wax_core::registry_memory::remember_design_system;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-cli-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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

fn write_pack_artifact(path: &Path, pack_name: &str) -> String {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};
    use tar::Builder;

    let mut archive = Vec::new();
    {
        let encoder = GzEncoder::new(&mut archive, Compression::default());
        let mut builder = Builder::new(encoder);
        let manifest = format!(
            r#"{{
  "id": "basic",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["{pack_name}"]
}}"#
        );
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "manifest.json", manifest.as_bytes())
            .expect("append manifest");
        builder.into_inner().expect("finish archive");
    }
    fs::write(path, archive).expect("write pack artifact");
    let digest = Sha256::digest(fs::read(path).expect("read artifact"));
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn install_basic_scan_fixture_pack(wax_home: &Path, sha256: &str) {
    use time::macros::datetime;
    use wax_contract::{
        CountSummary, LanguageId, LanguageMetadata, Metrics, SCHEMA_VERSION, ScanFacts, ScanStatus,
    };

    let install_dir = wax_home.join("langs/basic/0.1.0");
    fs::create_dir_all(&install_dir).expect("create language install dir");
    let script = install_dir.join("wax-lang-basic");
    let facts = ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("basic").expect("basic language id"),
            version: "0.1.0".to_owned(),
            ecosystem: "test".to_owned(),
            parser_name: "fixture".to_owned(),
            parser_version: "1.0.0".to_owned(),
        },
        snapshot_id: "snap-basic".to_owned(),
        scanned_at: datetime!(2020-01-01 00:00 UTC),
        status: ScanStatus::Complete,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![],
        diagnostics: vec![],
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            token_reference_ratio: None,
            parse_extract_ms: 0,
            files_scanned: 0,
        },
        counts: CountSummary::default(),
        symbol_usage_summary: vec![],
        design_system_tokens: vec![],
        token_sites: vec![],
        hardcoded_style_sites: vec![],
        token_usage_summary: vec![],
    };
    let response = serde_json::json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": "basic",
        "facts": facts,
    });
    fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{}\nJSON\n",
            serde_json::to_string(&response).expect("serialize scan facts")
        ),
    )
    .expect("write scan fixture script");
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script)
            .expect("read script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set script executable");
    }
    fs::write(
        install_dir.join("manifest.json"),
        format!(
            r#"{{
  "id": "basic",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./wax-lang-basic"],
  "target": "test-target",
  "sha256": "{sha256}",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}
"#
        ),
    )
    .expect("write fixture manifest");
    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "basic": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}
  }}
}}"#,
            install_dir.display()
        ),
    )
    .expect("write wax global state");
}

fn setup_remembered_basic_design_system(
    root: &Path,
    pack_sha256: &str,
) -> (PathBuf, PathBuf, PathBuf) {
    let ds_repo = root.join("acme-ds");
    fs::create_dir_all(ds_repo.join(".wax/registries")).expect("create ds registries dir");
    fs::write(
        ds_repo.join(".wax/registries/basic.json"),
        r#"{"schema_version":1,"components":[]}"#,
    )
    .expect("write ds registry");
    fs::write(
        ds_repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "basic": {
          "source": ".wax/registries/basic.json"
        }
      }
    }
  }
}
"#,
    )
    .expect("write ds config");

    let wax_home = root.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    install_basic_scan_fixture_pack(&wax_home, pack_sha256);
    let state_path = wax_home.join("state.json");
    remember_design_system(&state_path, "acme", "Acme Design System", &ds_repo)
        .expect("remember design system");

    (ds_repo, wax_home, state_path)
}

#[test]
fn scan_without_config_non_tty_suggests_init() {
    let _guard = env_lock();
    let root = TestDir::new("scan-non-tty");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .stdin(Stdio::null())
        .output()
        .expect("run wax scan");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("wax init"), "stderr was: {stderr}");
}

#[test]
fn ephemeral_scan_does_not_write_repo_config_or_registries() {
    let _guard = env_lock();
    let root = TestDir::new("scan-ephemeral");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let (_ds_repo, wax_home, state_path) =
        setup_remembered_basic_design_system(&root.path, &digest);
    let registry_path = root.path.join("registry.json");
    fs::write(
        &registry_path,
        format!(
            r#"[{{"id":"basic","version":"0.1.0","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index fixture");

    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create scan roots");
    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);

    assert!(!repo_relative_path_exists(&repo, ".wax/wax.config.json"));
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.lock.json"));
    assert!(!repo_relative_dir_has_entries(&repo, ".wax/registries"));

    run_scan_cli(
        ScanCommandOptions {
            repo_root: repo.clone(),
            allow_auto_install: false,
            scan_concurrency: None,
            state_path: Some(state_path),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system_id: "acme".to_owned(),
            }),
        },
        &mut Vec::new(),
    )
    .expect("ephemeral scan");

    assert!(
        !repo_relative_path_exists(&repo, ".wax/wax.config.json"),
        "ephemeral scan must not write wax config"
    );
    assert!(
        !repo_relative_path_exists(&repo, ".wax/wax.lock.json"),
        "ephemeral scan must not write wax lockfile"
    );
    assert!(
        !repo_relative_dir_has_entries(&repo, ".wax/registries"),
        "ephemeral scan must not write committed registries"
    );
    assert!(
        repo_relative_path_exists(&repo, ".wax/out/scan-merged.json"),
        "ephemeral scan should write scan output"
    );
}
