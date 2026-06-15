use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_contract::{LanguageId, SCHEMA_VERSION};

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

#[test]
fn scan_command_prints_full_summary_and_writes_output() {
    let _guard = env_lock();
    let root = TestDir::new("scan-command-summary");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax-home fixture");

    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file, &["compose", "react", "swift"]);
    write_installed_packs(
        &wax_home,
        &[
            ("compose", "complete", "0.5", "", ""),
            ("react", "partial", "null", "PACK_TIMEOUT", "timed out"),
            ("swift", "failed", "null", "PACK_CRASH", "process exited"),
        ],
    );

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--repo-root"])
        .arg(&repo)
        .args(["--concurrency", "1"])
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
        stdout.contains(&format!("scan output: {}", output_path.display())),
        "expected output path in summary, got: {stdout}"
    );
    assert!(stdout.contains("compose: complete (50.0%)"));
    assert!(stdout.contains("react: partial"));
    assert!(stdout.contains("swift: failed"));
    assert!(stdout.contains("failure diagnostics (up to 5):"));
    assert!(stdout.contains("PACK_TIMEOUT: timed out"));
    assert!(stdout.contains("PACK_CRASH: process exited"));
}

#[test]
fn scan_command_no_auto_install_fails_when_pack_missing() {
    let _guard = env_lock();
    let root = TestDir::new("scan-command-no-auto-install");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax-home fixture");

    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file, &["compose"]);
    fs::write(
        wax_home.join("state.json"),
        "{ \"installed_languages\": {} }\n",
    )
    .unwrap();

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--repo-root"])
        .arg(&repo)
        .arg("--no-auto-install")
        .output()
        .expect("spawn wax scan --no-auto-install");

    assert!(
        !output.status.success(),
        "expected non-zero exit, stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(
        stderr.contains("run `wax language install` or enable auto-install"),
        "expected no-auto-install guidance, got: {stderr}"
    );
}

#[test]
fn scan_command_default_auto_installs_missing_pack() {
    let _guard = env_lock();
    let root = TestDir::new("scan-command-auto-install");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax-home fixture");

    let artifact_path = root.path.join("compose-0.1.0.tgz");
    let digest = write_pack_artifact(&artifact_path);
    let registry_file = root.path.join("registry.json");
    write_pack_index_with_artifacts(
        &registry_file,
        &[(
            "compose",
            &format!("file://{}", artifact_path.display()),
            &digest,
        )],
    );
    write_repo_files_with_resolved_artifacts(
        &repo,
        &registry_file,
        &[(
            "compose",
            &format!("file://{}", artifact_path.display()),
            &digest,
        )],
    );

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax scan");

    assert!(
        output.status.success(),
        "expected auto-install to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(wax_home.join("state.json")).unwrap()).unwrap();
    assert!(
        state["installed_languages"]["compose"]["0.1.0"]["install_dir"]
            .as_str()
            .is_some(),
        "compose should be installed in global state"
    );
}

fn write_repo_files(repo: &Path, registry_file: &Path, languages: &[&str]) {
    write_default_registry(repo, languages);
    let languages_json = languages
        .iter()
        .map(|language| format!(r#"    {{ "id": "{language}", "enabled": true }}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo.join(".waxrc"),
        format!(
            r#"{{
  "schema_version": 1,
  "languages": [
{languages_json}
  ]
}}"#
        ),
    )
    .expect("write .waxrc");

    let registry_entries = registry_lock_entries(repo, languages);
    let lock_entries = languages
        .iter()
        .map(|language| {
            format!(
                r#"    "{language}": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file://{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/{language}-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}"#,
                registry_file.display()
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo.join("wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "registries": {{
{registry_entries}
  }},
  "languages": {{
{lock_entries}
  }}
}}"#
        ),
    )
    .expect("write lockfile");
}

fn write_pack_index(path: &Path) {
    write_pack_index_with_artifacts(
        path,
        &[
            (
                "compose",
                "https://example.invalid/compose-0.1.0.tgz",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "react",
                "https://example.invalid/react-0.1.0.tgz",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "swift",
                "https://example.invalid/swift-0.1.0.tgz",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        ],
    );
}

fn write_pack_index_with_artifacts(path: &Path, entries: &[(&str, &str, &str)]) {
    let manifests = entries
        .iter()
        .map(|(id, url, sha256)| {
            serde_json::json!({
                "id": id,
                "version": "0.1.0",
                "api_version": 1,
                "targets": {
                    "x86_64-unknown-linux-gnu": {
                        "url": url,
                        "sha256": sha256
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&manifests).unwrap()),
    )
    .expect("write pack index");
}

fn write_repo_files_with_resolved_artifacts(
    repo: &Path,
    registry_file: &Path,
    entries: &[(&str, &str, &str)],
) {
    let languages = entries.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
    write_default_registry(repo, &languages);
    let languages_json = languages
        .iter()
        .map(|language| format!(r#"    {{ "id": "{language}", "enabled": true }}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo.join(".waxrc"),
        format!(
            r#"{{
  "schema_version": 1,
  "languages": [
{languages_json}
  ]
}}"#
        ),
    )
    .expect("write .waxrc");

    let registry_entries = registry_lock_entries(repo, &languages);
    let lock_entries = entries
        .iter()
        .map(|(language, artifact_url, sha256)| {
            format!(
                r#"    "{language}": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file://{}",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "{artifact_url}",
        "sha256": "{sha256}",
        "signature": null
      }}
    }}"#,
                registry_file.display()
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo.join("wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "registries": {{
{registry_entries}
  }},
  "languages": {{
{lock_entries}
  }}
}}"#
        ),
    )
    .expect("write lockfile");
}

const DEFAULT_REGISTRY_JSON: &str =
    r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#;

fn language_registry_relative_path(language: &str) -> String {
    format!(".wax/{language}.registry.json")
}

fn write_default_registry(repo: &Path, languages: &[&str]) {
    fs::create_dir_all(repo.join(".wax")).expect("create .wax dir");
    for language in languages {
        fs::write(
            repo.join(language_registry_relative_path(language)),
            DEFAULT_REGISTRY_JSON,
        )
        .expect("write default registry");
    }
}

fn registry_lock_entries(repo: &Path, languages: &[&str]) -> String {
    languages
        .iter()
        .map(|language| {
            let path = language_registry_relative_path(language);
            let registry_sha256 = file_sha256(&repo.join(&path));
            format!(
                r#"    "{language}": {{
      "source": "{path}",
      "sha256": "{registry_sha256}"
    }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",\n")
}

fn file_sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).expect("read registry file"))
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn write_pack_artifact(path: &Path) -> String {
    let script = r#"#!/bin/sh
set -eu
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"compose","facts":{"schema_version":1,"language":{"id":"compose","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-compose","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"parse_extract_ms":0,"files_scanned":0,"adoption_coverage_ratio":null},"counts":{"design_system_component_count":0,"local_component_count":0,"usage_site_count":0,"resolved_count":0,"candidate_count":0}}}
JSON
"#;
    let artifact = gzip_tar(&[("wax-lang-compose", script.as_bytes(), 0o755)]);
    fs::write(path, &artifact).expect("write artifact");
    Sha256::digest(&artifact)
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

fn write_installed_packs(wax_home: &Path, specs: &[(&str, &str, &str, &str, &str)]) {
    let mut state_entries = Vec::new();
    for (language, status, adoption_coverage_ratio, error_code, error_message) in specs {
        let install_dir = wax_home.join(format!("langs/{language}/0.1.0"));
        fs::create_dir_all(&install_dir).expect("create install dir");

        let diagnostics = if error_code.is_empty() {
            serde_json::json!([])
        } else {
            serde_json::json!([{
                "severity": "error",
                "code": error_code,
                "message": error_message
            }])
        };
        let (usage_sites, usage_site_count, resolved_count, candidate_count) =
            if *adoption_coverage_ratio == "null" {
                (serde_json::json!([]), 0, 0, 0)
            } else {
                (
                    serde_json::json!([
                        {
                            "id": "site-1",
                            "location": { "file": "src/a.tsx", "line": 1 },
                            "symbol": "Button",
                            "match_status": "resolved",
                            "registry_symbol": "button"
                        },
                        {
                            "id": "site-2",
                            "location": { "file": "src/b.tsx", "line": 1 },
                            "symbol": "Button",
                            "match_status": "candidate",
                            "registry_symbol": "button"
                        }
                    ]),
                    2,
                    1,
                    1,
                )
            };
        let adoption_coverage_ratio = if *adoption_coverage_ratio == "null" {
            serde_json::Value::Null
        } else {
            serde_json::json!(adoption_coverage_ratio.parse::<f64>().unwrap())
        };
        let facts = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "language": {
                "id": LanguageId::try_from(*language).unwrap(),
                "version": "0.1.0",
                "ecosystem": "test",
                "parser_name": "fixture",
                "parser_version": "1.0.0"
            },
            "snapshot_id": format!("snap-{language}"),
            "scanned_at": "1970-01-01T00:00:00Z",
            "status": status,
            "design_system_components": [],
            "local_components": [],
            "usage_sites": usage_sites,
            "diagnostics": diagnostics,
            "metrics": {
                "parse_extract_ms": 5,
                "files_scanned": 1,
                "adoption_coverage_ratio": adoption_coverage_ratio
            },
            "counts": {
                "design_system_component_count": 0,
                "local_component_count": 0,
                "usage_site_count": usage_site_count,
                "resolved_count": resolved_count,
                "candidate_count": candidate_count,
            }
        });
        let wire = serde_json::json!({
            "type": "scan_facts",
            "api_version": 1,
            "language_id": language,
            "facts": facts
        });
        let script = install_dir.join("pack.sh");
        fs::write(
            &script,
            format!(
                r#"#!/bin/sh
set -eu
cat >/dev/null
cat <<JSON
{}
JSON
"#,
                wire
            ),
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
            format!(
                r#"{{
  "id": "{language}",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#
            ),
        )
        .expect("write pack manifest");

        state_entries.push(format!(
            r#"    "{language}": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}"#,
            install_dir.display()
        ));
    }

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
{}
  }}
}}"#,
            state_entries.join(",\n")
        ),
    )
    .expect("write state.json");
}
