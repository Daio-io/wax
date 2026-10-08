#![allow(dead_code)]

use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_contract::{LanguageId, SCHEMA_VERSION};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub struct EnvVarGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while mutating process environment variables"
    )]
    pub fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while restoring process environment variables"
    )]
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

pub struct TestDir {
    pub path: PathBuf,
}

impl TestDir {
    pub fn new(name: &str) -> Self {
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

pub fn summary_validator() -> jsonschema::Validator {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../wax-contract/schemas/scan-summary.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

pub fn assert_schema_valid_summary(value: &serde_json::Value) {
    let validator = summary_validator();
    assert!(
        validator.is_valid(value),
        "json summary failed schema validation: {value}"
    );
}

pub fn setup_scan_repo(
    name: &str,
    specs: &[(&str, &str, &str, &str, &str)],
) -> (TestDir, PathBuf, EnvVarGuard) {
    let root = TestDir::new(name);
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax-home fixture");

    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    let languages = specs
        .iter()
        .map(|(language, ..)| *language)
        .collect::<Vec<_>>();
    write_repo_files(&repo, &registry_file, &languages, None);
    write_installed_packs(&wax_home, specs);

    let wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);
    (root, repo, wax_home_guard)
}

pub fn run_scan(repo: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wax"));
    command.args(["scan", "--repo-root"]).arg(repo);
    command.args(["--concurrency", "1"]);
    command.args(extra_args);
    command.output().expect("spawn wax scan")
}

pub fn write_repo_files(
    repo: &Path,
    registry_file: &Path,
    languages: &[&str],
    outputs_json: Option<&str>,
) {
    write_default_registry(repo, languages);
    let languages_json = languages
        .iter()
        .map(|language| format!(r#"    "{language}": {{}}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    let outputs_block = outputs_json
        .map(|value| format!(",\n  \"outputs\": {value}"))
        .unwrap_or_default();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{
{languages_json}
  }}{outputs_block}
}}"#
        ),
    )
    .expect("write wax config");

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
        repo.join(".wax/wax.lock.json"),
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

pub fn write_grouped_repo_files(repo: &Path, registry_file: &Path) {
    write_default_registry(repo, &["compose", "react"]);
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "compose": {
      "roots": {
        "mobile": ["mobile/src"]
      }
    },
    "react": {
      "roots": {
        "web": ["web/src"]
      }
    }
  }
}"#,
    )
    .expect("write grouped config");
    fs::create_dir_all(repo.join("mobile/src")).expect("mobile root");
    fs::create_dir_all(repo.join("web/src")).expect("web root");

    let languages = ["compose", "react"];
    let registry_entries = registry_lock_entries(repo, &languages);
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
        repo.join(".wax/wax.lock.json"),
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

pub fn write_pack_index(path: &Path) {
    let manifests = ["compose", "react", "swift"]
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "version": "0.1.0",
                "api_version": 1,
                "targets": {
                    "x86_64-unknown-linux-gnu": {
                        "url": format!("https://example.invalid/{id}-0.1.0.tgz"),
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
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

/// Optional override for installed pack `usage_sites` / related fact fields.
///
/// When set, replaces the default invocation fixture for every language in
/// [`write_installed_packs_with_usage_sites`].
#[derive(Clone)]
pub struct PackUsageSitesOverride {
    /// Usage sites emitted by the fixture pack.
    pub usage_sites: serde_json::Value,
    /// Local component inventory paired with those sites.
    pub local_components: serde_json::Value,
    /// Count summary aligned with the override sites.
    pub counts: serde_json::Value,
    /// Metrics aligned with the override sites.
    pub metrics: serde_json::Value,
}

/// Local + unresolved sites without `registry_symbol` (no usage edges expected).
#[must_use]
pub fn no_registry_symbol_usage_override() -> PackUsageSitesOverride {
    PackUsageSitesOverride {
        usage_sites: serde_json::json!([
            {
                "id": "site-local",
                "location": { "file": "src/local.kt", "line": 2 },
                "symbol": "LocalButton",
                "match_status": "local",
                "callee_origin": "local",
                "resolution_evidence": { "kind": "local_same_file" },
                "local_definition_id": "local-1"
            },
            {
                "id": "site-unresolved",
                "location": { "file": "src/unknown.kt", "line": 1 },
                "symbol": "Mystery",
                "match_status": "unresolved",
                "callee_origin": "unknown",
                "resolution_evidence": { "kind": "no_matching_definition" }
            }
        ]),
        local_components: serde_json::json!([{
            "id": "local-1",
            "symbol": "LocalButton",
            "location": { "file": "src/local.kt", "line": 1 }
        }]),
        counts: serde_json::json!({
            "registry": {
                "component_count": 0,
                "used_component_count": 0,
                "resolved_raw_invocation_count": 0,
                "candidate_raw_invocation_count": 0
            },
            "definitions": {
                "local_definition_count": 1,
                "invoked_local_definition_count": 1,
                "unused_local_definition_count": 0
            },
            "raw_invocations": {
                "total": 2,
                "resolved": 0,
                "local": 1,
                "candidate": 0,
                "unresolved": 1
            },
            "adoption": {
                "eligible_invocation_count": 2,
                "adopted_invocation_count": 0,
                "non_adopted_invocation_count": 2,
                "adoption_excluded_invocation_count": 0
            },
            "parent_scopes": {
                "total": 0,
                "with_resolved_invocations": 0,
                "with_local_invocations": 0,
                "with_unresolved_invocations": 0
            },
            "invocation_origins": {
                "registry": 0,
                "local": 1,
                "framework": 0,
                "external": 0,
                "application": 0,
                "unknown": 1
            }
        }),
        metrics: serde_json::json!({
            "invocation_adoption_ratio": 0.0,
            "registry_resolution_ratio": 0.0,
            "parse_extract_ms": 5,
            "files_scanned": 1
        }),
    }
}

pub fn write_installed_packs(wax_home: &Path, specs: &[(&str, &str, &str, &str, &str)]) {
    write_installed_packs_with_usage_sites(wax_home, specs, None);
}

pub fn write_installed_packs_with_usage_sites(
    wax_home: &Path,
    specs: &[(&str, &str, &str, &str, &str)],
    usage_override: Option<PackUsageSitesOverride>,
) {
    let mut state_entries = Vec::new();
    for (language, status, invocation_fixture, error_code, error_message) in specs {
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
        let (usage_sites, local_components, counts, metrics) =
            if let Some(override_facts) = usage_override.as_ref() {
                (
                    override_facts.usage_sites.clone(),
                    override_facts.local_components.clone(),
                    override_facts.counts.clone(),
                    override_facts.metrics.clone(),
                )
            } else if *invocation_fixture == "null" {
                (
                    serde_json::json!([]),
                    serde_json::json!([]),
                    serde_json::json!({
                        "registry": {
                            "component_count": 0,
                            "used_component_count": 0,
                            "resolved_raw_invocation_count": 0,
                            "candidate_raw_invocation_count": 0
                        },
                        "definitions": {
                            "local_definition_count": 0,
                            "invoked_local_definition_count": 0,
                            "unused_local_definition_count": 0
                        },
                        "raw_invocations": {
                            "total": 0,
                            "resolved": 0,
                            "local": 0,
                            "candidate": 0,
                            "unresolved": 0
                        },
                        "adoption": {
                            "eligible_invocation_count": 0,
                            "adopted_invocation_count": 0,
                            "non_adopted_invocation_count": 0,
                            "adoption_excluded_invocation_count": 0
                        },
                        "parent_scopes": {
                            "total": 0,
                            "with_resolved_invocations": 0,
                            "with_local_invocations": 0,
                            "with_unresolved_invocations": 0
                        },
                        "invocation_origins": {
                            "registry": 0,
                            "local": 0,
                            "framework": 0,
                            "external": 0,
                            "application": 0,
                            "unknown": 0
                        }
                    }),
                    serde_json::json!({
                        "invocation_adoption_ratio": null,
                        "registry_resolution_ratio": null,
                        "parse_extract_ms": 5,
                        "files_scanned": 1
                    }),
                )
            } else {
                (
                    serde_json::json!([
                        {
                            "id": "site-1",
                            "location": { "file": "src/a.tsx", "line": 1 },
                            "symbol": "Button",
                            "match_status": "resolved",
                            "registry_symbol": "button",
                            "callee_origin": "registry",
                            "resolution_evidence": { "kind": "registry_name_only_legacy" }
                        },
                        {
                            "id": "site-2",
                            "location": { "file": "src/b.tsx", "line": 1 },
                            "symbol": "Button",
                            "match_status": "candidate",
                            "registry_symbol": "button",
                            "callee_origin": "registry",
                            "resolution_evidence": { "kind": "registry_import_missing" }
                        }
                    ]),
                    serde_json::json!([]),
                    serde_json::json!({
                        "registry": {
                            "component_count": 0,
                            "used_component_count": 1,
                            "resolved_raw_invocation_count": 1,
                            "candidate_raw_invocation_count": 1
                        },
                        "definitions": {
                            "local_definition_count": 0,
                            "invoked_local_definition_count": 0,
                            "unused_local_definition_count": 0
                        },
                        "raw_invocations": {
                            "total": 2,
                            "resolved": 1,
                            "local": 0,
                            "candidate": 1,
                            "unresolved": 0
                        },
                        "adoption": {
                            "eligible_invocation_count": 1,
                            "adopted_invocation_count": 1,
                            "non_adopted_invocation_count": 0,
                            "adoption_excluded_invocation_count": 0
                        },
                        "parent_scopes": {
                            "total": 0,
                            "with_resolved_invocations": 0,
                            "with_local_invocations": 0,
                            "with_unresolved_invocations": 0
                        },
                        "invocation_origins": {
                            "registry": 2,
                            "local": 0,
                            "framework": 0,
                            "external": 0,
                            "application": 0,
                            "unknown": 0
                        }
                    }),
                    serde_json::json!({
                        "invocation_adoption_ratio": 1.0,
                        "registry_resolution_ratio": 0.5,
                        "parse_extract_ms": 5,
                        "files_scanned": 1
                    }),
                )
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
            "local_components": local_components,
            "usage_sites": usage_sites,
            "diagnostics": diagnostics,
            "metrics": metrics,
            "counts": counts
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

/// Asserts that requesting a deferred `--output` format fails with the standard message.
pub fn assert_deferred_format(format: &str) {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        &format!("scan-artifact-deferred-{format}"),
        &[("compose", "complete", "0.5", "", "")],
    );

    let flag = format!("{format}=.wax/out/out.dat");
    let output = run_scan(&repo, &["--output", &flag]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(&format!("output format `{format}` is not implemented yet")),
        "unexpected stderr: {stderr}"
    );
}

pub fn write_committed_scan_repo_with_upstream(app_repo: &Path) {
    fs::create_dir_all(app_repo.join(".wax/registries/acme")).expect("create registries dir");
    fs::write(
        app_repo.join(".wax/registries/acme/react.json"),
        r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
    )
    .expect("write app registry");
    fs::write(
        app_repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  }
}
"#,
    )
    .expect("write app config");
    fs::write(
        app_repo.join(".wax/wax.lock.json"),
        r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {},
  "languages": {}
}
"#,
    )
    .expect("write app lockfile");
}
