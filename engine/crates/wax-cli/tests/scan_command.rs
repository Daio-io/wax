use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[test]
fn scan_command_runs_and_writes_output() {
    let _guard = env_lock();
    let root = temp_dir("scan-command");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax-home fixture");

    let registry_file = root.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file);
    write_installed_pack(&wax_home);

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let _wax_lang_index = EnvVarGuard::set(
        "WAX_LANG_INDEX",
        format!("file://{}", registry_file.display()),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax scan");

    assert!(
        output.status.success(),
        "wax scan exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let output_path = repo.join(".wax/out/scan-merged.json");
    assert!(output_path.exists(), "missing scan output file");

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(
        stdout.contains("scan output:"),
        "expected summary output, got: {stdout}"
    );
    assert!(
        stdout.contains("compose: complete"),
        "expected language status line, got: {stdout}"
    );
}

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wax-cli-{name}-{nonce}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn write_repo_files(repo: &Path, registry_file: &Path) {
    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    { "id": "compose", "enabled": true }
  ]
}"#,
    )
    .expect("write .waxrc");

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
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}
  }}
}}"#,
            registry_file.display()
        ),
    )
    .expect("write lockfile");
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
    .expect("write pack index");
}

fn write_installed_pack(wax_home: &Path) {
    let install_dir = wax_home.join("langs/compose/0.1.0");
    fs::create_dir_all(&install_dir).expect("create install dir");

    let script = install_dir.join("pack.sh");
    fs::write(
        &script,
        r#"#!/bin/sh
set -eu
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"compose","facts":{"schema_version":1,"language":{"id":"compose","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-compose","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"parse_extract_ms":5,"files_scanned":1,"adoption_coverage_ratio":null},"counts":{"design_system_component_count":0,"local_component_count":0,"usage_site_count":0,"resolved_count":0,"candidate_count":0}}}
JSON
"#,
    )
    .expect("write pack script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set executable bit");
    }

    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}"#,
    )
    .expect("write pack manifest");

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
    .expect("write state.json");
}
