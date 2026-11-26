use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use wax_cli::cli::{ConfiglessScanArgs, ScanStdoutFormat};
use wax_cli::commands::scan::{
    EphemeralScanSelections, ScanCommandError, ScanCommandOptions, ScanOutputRequest,
    repo_relative_dir_has_entries, repo_relative_path_exists, run_scan_cli,
};
use wax_contract::LanguageId;
use wax_core::paths::WaxHome;
use wax_core::registry_cache::RegistrySourceSpec;
use wax_core::registry_memory::{add_remembered_registry, remember_design_system};

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

fn host_target() -> &'static str {
    if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(
        target_arch = "x86_64",
        target_os = "linux",
        target_env = "gnu"
    )) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(
        target_arch = "aarch64",
        target_os = "linux",
        target_env = "gnu"
    )) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(
        target_arch = "x86_64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(
        target_arch = "aarch64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        "aarch64-pc-windows-msvc"
    } else {
        "unsupported-target"
    }
}

fn setup_binary_scan_pack(root: &Path) -> (PathBuf, String) {
    let artifact_path = root.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    install_basic_scan_fixture_pack(&wax_home, &digest, wax_contract::ScanStatus::Complete);
    let installed_manifest = wax_home.join("langs/basic/0.1.0/manifest.json");
    let manifest = fs::read_to_string(&installed_manifest)
        .expect("read installed manifest")
        .replace("test-target", host_target());
    fs::write(installed_manifest, manifest).expect("update installed manifest target");
    let pack_index = root.join("pack-index.json");
    fs::write(
        &pack_index,
        format!(
            r#"[{{"id":"basic","version":"0.1.0","api_version":1,"targets":{{"{}":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            host_target(),
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index fixture");
    (wax_home, file_url(&pack_index))
}

fn serve_registry_once(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind registry fixture");
    let address = listener.local_addr().expect("registry fixture address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept registry request");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("read registry request");
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .expect("write registry response headers");
        stream
            .write_all(&body)
            .expect("write registry response body");
    });
    (format!("http://{address}/basic.registry.json"), handle)
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn install_basic_scan_fixture_pack(
    wax_home: &Path,
    sha256: &str,
    status: wax_contract::ScanStatus,
) {
    use time::macros::datetime;
    use wax_contract::{
        CountSummary, LanguageId, LanguageMetadata, Metrics, SCHEMA_VERSION, ScanFacts,
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
        status,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![],
        diagnostics: vec![],
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
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
    status: wax_contract::ScanStatus,
) -> (PathBuf, PathBuf) {
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
    install_basic_scan_fixture_pack(&wax_home, pack_sha256, status);
    let state_path = wax_home.join("state.json");
    remember_design_system(&state_path, "acme", "Acme Design System", &ds_repo)
        .expect("remember design system");

    (ds_repo, wax_home)
}

#[test]
fn scan_without_config_non_tty_suggests_init() {
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
fn configless_flags_require_design_system_language_and_root() {
    let root = TestDir::new("configless-required-flags");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");

    for supplied_args in [
        vec!["--design-system", "acme"],
        vec!["--language", "basic"],
        vec!["--root", "src"],
        vec!["--design-system", "acme", "--language", "basic"],
        vec!["--design-system", "acme", "--root", "src"],
        vec!["--language", "basic", "--root", "src"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wax"))
            .arg("scan")
            .arg("--repo-root")
            .arg(&repo)
            .args(&supplied_args)
            .stdin(Stdio::null())
            .output()
            .expect("run wax scan");

        assert!(
            !output.status.success(),
            "partial configless arguments unexpectedly succeeded: {supplied_args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("required"),
            "partial configless arguments should report required peers; args={supplied_args:?}, stderr={stderr}"
        );
    }
}

#[test]
fn configless_direct_conversion_returns_typed_incomplete_selection() {
    let error = EphemeralScanSelections::from_cli(ConfiglessScanArgs {
        design_system: Some("acme".to_owned()),
        language: None,
        roots: vec![PathBuf::from("src")],
        tag: None,
        offline: false,
    })
    .expect_err("incomplete direct selection should fail");

    assert!(matches!(
        error,
        ScanCommandError::IncompleteConfiglessSelection { missing }
            if missing == vec!["--language"]
    ));
}

#[test]
fn configless_flags_conflict_with_root_group() {
    let root = TestDir::new("configless-root-group-conflict");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args([
            "--design-system",
            "acme",
            "--language",
            "basic",
            "--root",
            "src",
            "--root-group",
            "production",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run wax scan");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "stderr was: {stderr}"
    );
}

#[test]
fn configless_local_registry_scan_non_tty() {
    let root = TestDir::new("configless-local-registry");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    install_basic_scan_fixture_pack(&wax_home, &digest, wax_contract::ScanStatus::Complete);
    let installed_manifest = wax_home.join("langs/basic/0.1.0/manifest.json");
    let manifest = fs::read_to_string(&installed_manifest)
        .expect("read installed manifest")
        .replace("test-target", host_target());
    fs::write(installed_manifest, manifest).expect("update installed manifest target");
    let pack_index = root.path.join("pack-index.json");
    fs::write(
        &pack_index,
        format!(
            r#"[{{"id":"basic","version":"0.1.0","api_version":1,"targets":{{"{}":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            host_target(),
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index fixture");
    let registry = root.path.join("basic.registry.json");
    fs::write(
        &registry,
        r#"{"schema_version":1,"language":"basic","components":[]}"#,
    )
    .expect("write registry fixture");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create scan root");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_PACK_INDEX", file_url(&pack_index))
        .env("WAX_HOME", &wax_home)
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args(["--language", "basic", "--root", "src", "--design-system"])
        .arg(&registry)
        .args(["--no-auto-install", "--strict"])
        .stdin(Stdio::null())
        .output()
        .expect("run configless local scan");

    assert!(
        output.status.success(),
        "stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(repo_relative_path_exists(
        &repo,
        ".wax/out/scan-merged.json"
    ));
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.config.json"));
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.lock.json"));
    assert!(!repo_relative_dir_has_entries(&repo, ".wax/registries"));
}

#[test]
fn configless_selection_rejected_when_config_exists() {
    let root = TestDir::new("configless-config-conflict");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join(".wax")).expect("create config directory");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{"schema_version":2,"languages":[]}"#,
    )
    .expect("write committed config");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args([
            "--language",
            "basic",
            "--root",
            "src",
            "--design-system",
            "acme",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run conflicting scan");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with committed repository config"),
        "stderr was: {stderr}"
    );
}

#[test]
fn configless_roots_use_config_equivalent_validation() {
    let root = TestDir::new("configless-root-validation");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");
    let absolute = root.path.join("absolute");
    let cases = [
        ("", "a value is required"),
        (
            absolute.to_str().expect("utf-8 path"),
            "must be repo-relative",
        ),
        ("../src", "must not contain parent-directory segments"),
    ];

    for (scan_root, expected) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_wax"))
            .arg("scan")
            .arg("--repo-root")
            .arg(&repo)
            .args([
                "--language",
                "basic",
                "--root",
                scan_root,
                "--design-system",
                "acme",
            ])
            .stdin(Stdio::null())
            .output()
            .expect("run scan with invalid root");

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "root={scan_root:?}, stderr was: {stderr}"
        );
    }
}

#[test]
fn configless_http_then_offline_cached_scan() {
    let root = TestDir::new("configless-http-offline");
    let (wax_home, pack_index) = setup_binary_scan_pack(&root.path);
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create scan root");
    let (registry_url, server) =
        serve_registry_once(br#"{"schema_version":1,"language":"basic","components":[]}"#.to_vec());

    let first = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_PACK_INDEX", &pack_index)
        .env("WAX_HOME", &wax_home)
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args([
            "--language",
            "basic",
            "--root",
            "src",
            "--design-system",
            &registry_url,
            "--no-auto-install",
            "--strict",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run online HTTP scan");
    server.join().expect("join registry fixture");
    assert!(
        first.status.success(),
        "online stderr was: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_PACK_INDEX", &pack_index)
        .env("WAX_HOME", &wax_home)
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args([
            "--language",
            "basic",
            "--root",
            "src",
            "--design-system",
            &registry_url,
            "--offline",
            "--no-auto-install",
            "--strict",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run cached offline HTTP scan");

    assert!(
        second.status.success(),
        "offline stderr was: {}",
        String::from_utf8_lossy(&second.stderr)
    );
}

#[test]
fn configless_offline_cache_miss_is_typed() {
    let root = TestDir::new("configless-offline-miss");
    let (wax_home, pack_index) = setup_binary_scan_pack(&root.path);
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create scan root");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_PACK_INDEX", &pack_index)
        .env("WAX_HOME", &wax_home)
        .arg("scan")
        .arg("--repo-root")
        .arg(&repo)
        .args([
            "--language",
            "basic",
            "--root",
            "src",
            "--design-system",
            "http://127.0.0.1:9/unseen.registry.json",
            "--offline",
            "--no-auto-install",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run offline cache miss scan");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("offline_cache_miss"),
        "stderr was: {stderr}"
    );
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.config.json"));
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.lock.json"));
    assert!(!repo_relative_dir_has_entries(&repo, ".wax/registries"));
}

#[test]
fn ephemeral_scan_does_not_write_repo_config_or_registries() {
    let root = TestDir::new("scan-ephemeral");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let (_ds_repo, wax_home) = setup_remembered_basic_design_system(
        &root.path,
        &digest,
        wax_contract::ScanStatus::Complete,
    );
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

    assert!(!repo_relative_path_exists(&repo, ".wax/wax.config.json"));
    assert!(!repo_relative_path_exists(&repo, ".wax/wax.lock.json"));
    assert!(!repo_relative_dir_has_entries(&repo, ".wax/registries"));

    run_scan_cli(
        ScanCommandOptions {
            presentation: wax_cli::presentation::Presentation::Plain,
            repo_root: repo.clone(),
            strict: true,
            allow_auto_install: false,
            scan_concurrency: None,
            root_group: None,
            format: wax_cli::cli::ScanStdoutFormat::Summary,
            compact: false,
            cli_outputs: Vec::new(),
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system: "acme".to_owned(),
                tag: None,
                offline: false,
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

#[test]
fn ephemeral_json_summary_stdout_omits_init_hint() {
    let root = TestDir::new("scan-ephemeral-json-summary");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let (_ds_repo, wax_home) = setup_remembered_basic_design_system(
        &root.path,
        &digest,
        wax_contract::ScanStatus::Complete,
    );
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
    let mut output = Vec::new();

    run_scan_cli(
        ScanCommandOptions {
            presentation: wax_cli::presentation::Presentation::Plain,
            repo_root: repo.clone(),
            strict: true,
            allow_auto_install: false,
            scan_concurrency: None,
            root_group: None,
            format: ScanStdoutFormat::JsonSummary,
            compact: false,
            cli_outputs: Vec::new(),
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system: "acme".to_owned(),
                tag: None,
                offline: false,
            }),
        },
        &mut output,
    )
    .expect("ephemeral json-summary scan");

    let stdout = String::from_utf8(output).unwrap();
    assert!(
        !stdout.contains("run `wax init`"),
        "init hint must stay on Summary only, got: {stdout}"
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["repo_root"], ".");
    assert!(repo_relative_path_exists(
        &repo,
        ".wax/out/scan-merged.json"
    ));
}

#[test]
fn ephemeral_output_writes_json_summary_under_repo_root() {
    let root = TestDir::new("scan-ephemeral-output-artifact");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let (_ds_repo, wax_home) = setup_remembered_basic_design_system(
        &root.path,
        &digest,
        wax_contract::ScanStatus::Complete,
    );
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
    let mut output = Vec::new();

    run_scan_cli(
        ScanCommandOptions {
            presentation: wax_cli::presentation::Presentation::Plain,
            repo_root: repo.clone(),
            strict: true,
            allow_auto_install: false,
            scan_concurrency: None,
            root_group: None,
            format: ScanStdoutFormat::Quiet,
            compact: false,
            cli_outputs: vec![ScanOutputRequest {
                format: "json-summary".to_owned(),
                path: PathBuf::from(".wax/out/scan-summary.json"),
            }],
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system: "acme".to_owned(),
                tag: None,
                offline: false,
            }),
        },
        &mut output,
    )
    .expect("ephemeral artifact scan");

    assert!(repo_relative_path_exists(
        &repo,
        ".wax/out/scan-summary.json"
    ));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.join(".wax/out/scan-summary.json")).unwrap())
            .unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["scan_path"], ".wax/out/scan-merged.json");
}

#[test]
fn ephemeral_strict_scan_writes_output_before_failing_on_partial_result() {
    let root = TestDir::new("scan-ephemeral-strict-partial");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let (_ds_repo, wax_home) = setup_remembered_basic_design_system(
        &root.path,
        &digest,
        wax_contract::ScanStatus::Partial,
    );
    let registry_path = root.path.join("registry.json");
    fs::write(
        &registry_path,
        format!(
            r#"[{{"id":"basic","version":"0.1.0","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path), digest
        ),
    )
    .expect("write pack index fixture");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create scan roots");
    let mut output = Vec::new();
    let error = run_scan_cli(
        ScanCommandOptions {
            presentation: wax_cli::presentation::Presentation::Plain,
            repo_root: repo.clone(),
            strict: true,
            allow_auto_install: false,
            scan_concurrency: None,
            root_group: None,
            format: wax_cli::cli::ScanStdoutFormat::Summary,
            compact: false,
            cli_outputs: Vec::new(),
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system: "acme".to_owned(),
                tag: None,
                offline: false,
            }),
        },
        &mut output,
    )
    .expect_err("strict partial ephemeral scan should fail");
    assert!(error.to_string().contains("partial: [\"basic\"]"));
    assert!(repo_relative_path_exists(
        &repo,
        ".wax/out/scan-merged.json"
    ));
}

#[test]
fn ephemeral_scan_uses_source_backed_remembered_registry_without_publisher_checkout() {
    let root = TestDir::new("scan-ephemeral-source-backed");
    let artifact_path = root.path.join("basic.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-basic");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    install_basic_scan_fixture_pack(&wax_home, &digest, wax_contract::ScanStatus::Complete);
    let state_path = wax_home.join("state.json");
    let source = root.path.join("basic-registry.json");
    fs::write(
        &source,
        r#"{"schema_version":1,"language":"basic","components":[{"id":"button","symbol":"Button"}]}"#,
    )
    .expect("write source registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: source.clone(),
            language: None,
        },
    )
    .expect("remember source-backed registry");
    fs::remove_file(&source).expect("remove original source so only cache remains");

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

    run_scan_cli(
        ScanCommandOptions {
            presentation: wax_cli::presentation::Presentation::Plain,
            repo_root: repo.clone(),
            strict: true,
            allow_auto_install: false,
            scan_concurrency: None,
            root_group: None,
            format: wax_cli::cli::ScanStdoutFormat::Summary,
            compact: false,
            cli_outputs: Vec::new(),
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: Some(file_url(&registry_path)),
            target_triple: Some("test-target".to_owned()),
            ephemeral: Some(EphemeralScanSelections {
                languages: vec![LanguageId::try_from("basic").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("basic").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                design_system: "acme".to_owned(),
                tag: None,
                offline: false,
            }),
        },
        &mut Vec::new(),
    )
    .expect("source-backed ephemeral scan");

    assert!(
        !repo_relative_path_exists(&repo, ".wax/wax.config.json"),
        "ephemeral scan must not write wax config"
    );
    assert!(
        !repo_relative_dir_has_entries(&repo, ".wax/registries"),
        "ephemeral scan must not write committed registries"
    );
    assert!(repo_relative_path_exists(
        &repo,
        ".wax/out/scan-merged.json"
    ));
}

#[test]
fn strict_scans_use_each_repos_relative_registry_with_shared_home() {
    let root = TestDir::new("relative-registries");
    let (wax_home, pack_index) = setup_binary_scan_pack(&root.path);
    let script = wax_home.join("langs/basic/0.1.0/wax-lang-basic");
    let original = fs::read_to_string(&script).unwrap();
    let response = original
        .split("cat <<'JSON'\n")
        .nth(1)
        .unwrap()
        .split("\nJSON")
        .next()
        .unwrap();
    fs::write(&script, format!(r##"#!/usr/bin/env python3
import json, os, sys
request = json.load(sys.stdin)
path = request['config']['registry']
with open(os.path.join(request['repo_root'], path)) as registry_file:
    component = json.load(registry_file)['components'][0]
response = json.loads({response:?})
response['facts']['design_system_components'] = [{{'id': component['id'], 'symbol': component['symbol'], 'registry_symbol': component['symbol']}}]
response['facts']['counts']['registry']['component_count'] = 1
print(json.dumps(response))
"##)).unwrap();
    for name in ["Alpha", "Beta"] {
        let repo = root.path.join(name);
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("basic.json"), format!(r#"{{"schema_version":1,"language":"basic","components":[{{"id":"{name}","symbol":"{name}"}}]}}"#)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_wax"))
            .env("WAX_PACK_INDEX", &pack_index)
            .env("WAX_HOME", &wax_home)
            .arg("scan")
            .arg("--repo-root")
            .arg(&repo)
            .args([
                "--language",
                "basic",
                "--root",
                "src",
                "--design-system",
                "./basic.json",
                "--no-auto-install",
                "--strict",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let merged: serde_json::Value =
            serde_json::from_slice(&fs::read(repo.join(".wax/out/scan-merged.json")).unwrap())
                .unwrap();
        assert_eq!(
            merged["languages"]["basic"]["design_system_components"][0]["id"],
            name
        );
    }
}
