use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wax_contract::LanguageId;
use wax_core::{Engine, ScanOptions};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while mutating process environment variables, which keeps env access serialized inside this test binary"
    )]
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
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
        reason = "these tests hold ENV_LOCK while restoring process environment variables, which keeps env access serialized inside this test binary"
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

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

struct ScanFixture {
    repo: PathBuf,
    wax_home: PathBuf,
    log_dir: PathBuf,
}

fn fixture(
    name: &str,
    languages: &[&str],
    scan_concurrency: Option<u32>,
    barrier_target: Option<usize>,
) -> ScanFixture {
    let root = temp_dir(name);
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    let log_dir = root.join("scan-log");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(log_dir.join("active"), "0\n").unwrap();
    fs::write(log_dir.join("max"), "0\n").unwrap();

    write_waxrc(&repo, languages, scan_concurrency);
    write_default_registry(&repo, languages);
    write_lockfile(&repo, languages);
    write_installed_packs(&wax_home, languages, &log_dir, barrier_target);

    ScanFixture {
        repo,
        wax_home,
        log_dir,
    }
}

const DEFAULT_REGISTRY_JSON: &str =
    r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#;

fn language_registry_relative_path(language: &str) -> String {
    format!(".wax/{language}.registry.json")
}

fn write_default_registry(repo: &Path, languages: &[&str]) {
    fs::create_dir_all(repo.join(".wax")).unwrap();
    for language in languages {
        fs::write(
            repo.join(language_registry_relative_path(language)),
            DEFAULT_REGISTRY_JSON,
        )
        .unwrap();
    }
}

fn write_waxrc(repo: &Path, languages: &[&str], scan_concurrency: Option<u32>) {
    let engine = scan_concurrency
        .map(|value| format!(r#", "engine": {{ "scan_concurrency": {value} }}"#))
        .unwrap_or_default();
    let languages = languages
        .iter()
        .map(|language| format!(r#"    "{language}": {{}}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2{engine},
  "languages": {{
{languages}
  }}
}}"#
        ),
    )
    .unwrap();
}

fn write_lockfile(repo: &Path, languages: &[&str]) {
    let registry_entries = languages
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
        .join(",\n");
    let entries = languages
        .iter()
        .map(|language| {
            format!(
                r#"    "{language}": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "https://registry.example.invalid/{language}.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/{language}-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}"#
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
{entries}
  }}
}}"#
        ),
    )
    .unwrap();
}

fn file_sha256(path: &Path) -> String {
    let digest = Sha256::digest(fs::read(path).unwrap());
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn write_installed_packs(
    wax_home: &Path,
    languages: &[&str],
    log_dir: &Path,
    barrier_target: Option<usize>,
) {
    let barrier_languages = barrier_target
        .map(|target| {
            let mut sorted = languages.to_vec();
            sorted.sort_unstable();
            sorted.truncate(target);
            sorted
        })
        .unwrap_or_default();

    let mut state_entries = Vec::new();
    for language in languages {
        let language_barrier = if barrier_languages.contains(language) {
            barrier_target.unwrap_or_default()
        } else {
            0
        };
        let install_dir = wax_home.join(format!("langs/{language}/0.1.0"));
        fs::create_dir_all(&install_dir).unwrap();
        let script = install_dir.join("pack.sh");
        fs::write(&script, fixture_script()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }
        fs::write(
            install_dir.join("manifest.json"),
            format!(
                r#"{{
  "id": "{language}",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh", "{language}", "{}", "0.2", "{language_barrier}"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#,
                log_dir.display()
            ),
        )
        .unwrap();
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
    .unwrap();
}

fn write_installed_pack(
    wax_home: &Path,
    language: &str,
    log_dir: &Path,
    script_body: &str,
    args: &[&str],
) -> PathBuf {
    let install_dir = wax_home.join(format!("langs/{language}/0.1.0"));
    fs::create_dir_all(&install_dir).unwrap();
    let script = install_dir.join("pack.sh");
    fs::write(&script, script_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }

    let args = args
        .iter()
        .map(|arg| format!(r#", "{arg}""#))
        .collect::<String>();
    fs::write(
        install_dir.join("manifest.json"),
        format!(
            r#"{{
  "id": "{language}",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh", "{language}", "{}"{args}],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#,
            log_dir.display()
        ),
    )
    .unwrap();

    install_dir
}

fn fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
log_dir="$2"
delay="$3"
barrier_target="$4"
lock_dir="$log_dir/lock"

acquire_lock() {
  while ! mkdir "$lock_dir" 2>/dev/null; do
    sleep 0.01
  done
}

release_lock() {
  rmdir "$lock_dir"
}

acquire_lock
active="$(cat "$log_dir/active")"
active=$((active + 1))
printf '%s\n' "$active" > "$log_dir/active"
max="$(cat "$log_dir/max")"
if [ "$active" -gt "$max" ]; then
  printf '%s\n' "$active" > "$log_dir/max"
fi
printf 'start:%s\n' "$language" >> "$log_dir/events"
release_lock

if [ "$barrier_target" -gt 1 ]; then
  attempts=0
  while [ "$attempts" -lt 200 ]; do
    acquire_lock
    active="$(cat "$log_dir/active")"
    release_lock
    if [ "$active" -ge "$barrier_target" ]; then
      break
    fi
    attempts=$((attempts + 1))
    sleep 0.01
  done
fi

sleep "$delay"
cat >/dev/null

acquire_lock
active="$(cat "$log_dir/active")"
active=$((active - 1))
printf '%s\n' "$active" > "$log_dir/active"
printf 'end:%s\n' "$language" >> "$log_dir/events"
release_lock

cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":3,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-1","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":0,"files_scanned":0},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0}}}}
JSON
"#
}

fn failing_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
log_dir="$2"
printf 'start:%s\n' "$language" >> "$log_dir/events"
attempts=0
while [ "$attempts" -lt 500 ]; do
  if grep -q '^start:sleeper$' "$log_dir/events" 2>/dev/null; then
    break
  fi
  attempts=$((attempts + 1))
  sleep 0.01
done
if ! grep -q '^start:sleeper$' "$log_dir/events" 2>/dev/null; then
  printf 'sleeper did not start\n' >&2
  exit 43
fi
printf 'fail:%s\n' "$language" >> "$log_dir/events"
printf 'fixture failure\n' >&2
exit 42
"#
}

fn sleeping_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
log_dir="$2"
delay="$3"
printf 'start:%s\n' "$language" >> "$log_dir/events"
sleep "$delay"
cat >/dev/null
printf 'end:%s\n' "$language" >> "$log_dir/events"
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":3,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-1","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":0,"files_scanned":0},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0}}}}
JSON
"#
}

fn max_concurrency(log_dir: &Path) -> u32 {
    fs::read_to_string(log_dir.join("max"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn language_order(scan: &wax_contract::MergedScan) -> Vec<String> {
    scan.languages
        .keys()
        .map(LanguageId::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn scan_repo_uses_default_waxrc_scan_concurrency() {
    let _guard = env_lock();
    let fixture = fixture(
        "scan-concurrency-default",
        &["compose", "react", "vue"],
        None,
        Some(2),
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    Engine::scan_repo(&fixture.repo).expect("scan should pass");

    assert_eq!(max_concurrency(&fixture.log_dir), 2);
}

#[test]
fn scan_repo_options_can_override_waxrc_scan_concurrency_to_serial() {
    let _guard = env_lock();
    let fixture = fixture(
        "scan-concurrency-override",
        &["compose", "react", "vue"],
        Some(3),
        None,
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            scan_concurrency: Some(1),
            ..ScanOptions::default()
        },
    )
    .expect("scan should pass");

    assert_eq!(max_concurrency(&fixture.log_dir), 1);
}

#[test]
fn scan_concurrency_runs_enabled_languages_in_parallel_with_bounded_concurrency() {
    let _guard = env_lock();
    let fixture = fixture(
        "scan-concurrency-bounded",
        &["compose", "react", "swift", "vue"],
        Some(3),
        Some(3),
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    Engine::scan_repo(&fixture.repo).expect("scan should pass");

    assert_eq!(max_concurrency(&fixture.log_dir), 3);
}

#[test]
fn scan_concurrency_merges_parallel_results_in_language_id_order() {
    let _guard = env_lock();
    let fixture = fixture(
        "scan-concurrency-order",
        &["vue", "react", "compose"],
        Some(3),
        None,
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &fixture.wax_home);

    let scan = Engine::scan_repo(&fixture.repo).expect("scan should pass");

    assert_eq!(language_order(&scan), ["compose", "react", "vue"]);
}

#[test]
fn scan_concurrency_cancels_in_flight_scans_after_first_error() {
    let _guard = env_lock();
    let root = temp_dir("scan-concurrency-cancel");
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    let log_dir = root.join("scan-log");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&log_dir).unwrap();

    write_waxrc(&repo, &["fail", "sleeper"], Some(2));
    write_default_registry(&repo, &["fail", "sleeper"]);
    write_lockfile(&repo, &["fail", "sleeper"]);
    let fail_dir = write_installed_pack(&wax_home, "fail", &log_dir, failing_script(), &[]);
    let sleeper_dir =
        write_installed_pack(&wax_home, "sleeper", &log_dir, sleeping_script(), &["3"]);
    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "fail": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }},
    "sleeper": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}
  }}
}}"#,
            fail_dir.display(),
            sleeper_dir.display()
        ),
    )
    .unwrap();

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let err = Engine::scan_repo(&repo).expect_err("failing pack should fail the scan");

    assert!(
        err.to_string().contains("language subprocess"),
        "unexpected error: {err}"
    );
    let events = fs::read_to_string(log_dir.join("events")).unwrap();
    assert!(
        events.lines().any(|line| line == "start:sleeper"),
        "sleeper was not in flight before failure; events:\n{events}"
    );
    assert!(
        !events.lines().any(|line| line == "end:sleeper"),
        "sleeper completed instead of being cancelled; events:\n{events}"
    );
}
