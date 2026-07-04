use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_cli::commands::init::{InitOptions, InitSelections, RegistrySetup, run_init};
use wax_contract::LanguageId;
use wax_core::registry_memory::remember_design_system;

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
  "id": "react",
  "version": "0.2.0",
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

fn setup_remembered_react_design_system(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let ds_repo = root.join("acme-ds");
    fs::create_dir_all(ds_repo.join(".wax/registries")).expect("create ds registries dir");
    fs::write(
        ds_repo.join(".wax/registries/react.json"),
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
        "react": {
          "source": ".wax/registries/react.json"
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
    let state_path = wax_home.join("state.json");
    remember_design_system(&state_path, "acme", "Acme Design System", &ds_repo)
        .expect("remember design system");

    (ds_repo, wax_home, state_path)
}

#[test]
fn init_without_non_interactive_requires_tty() {
    let root = TestDir::new("init-non-tty");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("init")
        .arg("--no-install")
        .arg("--repo-root")
        .arg(&repo)
        .stdin(Stdio::null())
        .output()
        .expect("run wax init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wax init needs an interactive terminal"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("wax init --non-interactive --language <language-id>"),
        "stderr was: {stderr}"
    );
}

#[test]
fn init_rejects_existing_config_before_tty_check() {
    let root = TestDir::new("init-existing-config");
    let repo = root.path.join("repo");
    let config_path = repo.join(".wax/wax.config.json");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("create .wax");
    fs::write(&config_path, "{}\n").expect("write existing config");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("init")
        .arg("--no-install")
        .arg("--repo-root")
        .arg(&repo)
        .stdin(Stdio::null())
        .output()
        .expect("run wax init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wax config already exists"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("wax init needs an interactive terminal"),
        "existing config should be rejected before TTY check; stderr was: {stderr}"
    );
}

#[test]
fn init_remembered_design_system_writes_upstream_registry_config() {
    let root = TestDir::new("init-remembered-registry");
    let (_ds_repo, _wax_home, state_path) = setup_remembered_react_design_system(&root.path);
    let artifact_path = root.path.join("react.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-react");
    let registry_path = root.path.join("registry.json");
    fs::write(
        &registry_path,
        format!(
            r#"[{{"id":"react","version":"0.2.0","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index fixture");

    let app_repo = root.path.join("app");
    fs::create_dir_all(app_repo.join("src")).expect("create app src");

    run_init(
        InitOptions {
            non_interactive: false,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(file_url(&registry_path)),
            repo_root: app_repo.clone(),
            target_triple: Some("test-target".to_owned()),
            state_path: Some(state_path),
            scaffold_registries: false,
            interactive: Some(InitSelections {
                languages: vec![LanguageId::try_from("react").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("react").unwrap(),
                    vec![PathBuf::from("src")],
                )]),
                registry_setup: RegistrySetup::RememberedDesignSystem {
                    design_system_id: "acme".to_owned(),
                },
            }),
        },
        &mut Vec::new(),
    )
    .expect("init with remembered design system");

    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("wax config"),
    )
    .unwrap();
    assert_eq!(config["schema_version"], 2);
    assert_eq!(
        config["languages"]["react"]["roots"],
        serde_json::json!(["src"])
    );
    assert_eq!(
        config["languages"]["react"]["registry"]["source"],
        ".wax/registries/acme/react.json"
    );
    assert_eq!(
        config["languages"]["react"]["registry"]["upstream"],
        "acme/react"
    );
    assert!(app_repo.join(".wax/registries/acme/react.json").is_file());
}
