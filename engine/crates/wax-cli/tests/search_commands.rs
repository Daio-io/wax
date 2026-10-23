use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

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
        reason = "these tests hold ENV_LOCK while mutating process environment variables"
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

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-cli-search-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn search_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../wax-contract/schemas/registry-search.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn detail_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../wax-contract/schemas/registry-detail.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn error_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../wax-contract/schemas/registry-lookup-error.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
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

fn write_registry(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create registry parent");
    }
    fs::write(path, body).expect("write registry");
}

fn remember_local_registry(wax_home: &Path, design_system_id: &str, registry_path: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "registry",
            "add",
            design_system_id,
            &registry_path.display().to_string(),
            "--language",
            "react",
            "--name",
            "Search Fixture",
        ])
        .env("WAX_HOME", wax_home)
        .output()
        .expect("spawn registry add");
    assert!(
        output.status.success(),
        "registry add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_wax(wax_home: &Path, repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(args)
        .args(["--repo-root"])
        .arg(repo)
        .env("WAX_HOME", wax_home)
        .output()
        .expect("spawn wax")
}

fn fixture_registry() -> &'static str {
    r##"{
  "schema_version": 1,
  "language": "react",
  "components": [{
    "id": "ds.button",
    "symbol": "Button",
    "package": "@acme/ui",
    "aliases": ["ActionButton"],
    "metadata": {
      "description": "Primary action control",
      "tags": ["control"]
    },
    "extracted": {
      "status": "complete",
      "documentation": "Use for primary actions."
    },
    "legacy_flag": true
  }],
  "tokens": [{
    "id": "color.primary",
    "key": "color.primary",
    "category": "color",
    "value": "#ff00aa",
    "metadata": {
      "description": "Brand primary color"
    }
  }]
}"##
}

#[test]
fn search_json_finds_fuzzy_component_and_keeps_stdout_schema_valid() {
    let _guard = env_lock();
    let root = TestDir::new("search-json");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let output = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "buton",
            "--design-system",
            "acme",
            "--format",
            "json",
        ],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let value: Value = serde_json::from_str(stdout.trim()).expect("stdout json");
    assert!(
        search_validator().is_valid(&value),
        "search response failed schema: {value}"
    );
    assert_eq!(value["results"][0]["id"], "ds.button");
    assert_eq!(value["results"][0]["kind"], "component");
    assert!(!stdout.contains("error:"));
    assert!(!stderr.contains('{'), "progress must not emit JSON objects");
}

#[test]
fn search_rejects_empty_query_and_zero_limit_with_typed_json_errors() {
    let _guard = env_lock();
    let root = TestDir::new("search-invalid");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let empty = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "   ",
            "--design-system",
            "acme",
            "--format",
            "json",
        ],
    );
    assert!(!empty.status.success());
    let empty_value: Value =
        serde_json::from_str(String::from_utf8(empty.stdout).unwrap().trim()).unwrap();
    assert!(error_validator().is_valid(&empty_value));
    assert_eq!(empty_value["code"], "invalid_argument");

    let zero = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "button",
            "--design-system",
            "acme",
            "--limit",
            "0",
            "--format",
            "json",
        ],
    );
    assert!(!zero.status.success());
    let zero_value: Value =
        serde_json::from_str(String::from_utf8(zero.stdout).unwrap().trim()).unwrap();
    assert!(error_validator().is_valid(&zero_value));
    assert_eq!(zero_value["code"], "invalid_argument");
}

#[test]
fn search_no_matches_is_success_with_empty_results() {
    let _guard = env_lock();
    let root = TestDir::new("search-empty");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let output = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "unrelated-query-with-no-match",
            "--design-system",
            "acme",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success());
    let value: Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert!(search_validator().is_valid(&value));
    assert!(value["results"].as_array().unwrap().is_empty());
}

#[test]
fn search_supports_kind_language_limit_and_offline() {
    let _guard = env_lock();
    let root = TestDir::new("search-flags");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let tokens = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "primary",
            "--design-system",
            "acme",
            "--kind",
            "token",
            "--language",
            "react",
            "--limit",
            "1",
            "--offline",
            "--format",
            "json",
        ],
    );
    assert!(
        tokens.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&tokens.stderr)
    );
    let value: Value =
        serde_json::from_str(String::from_utf8(tokens.stdout).unwrap().trim()).unwrap();
    assert!(search_validator().is_valid(&value));
    assert_eq!(value["filters"]["kind"], "token");
    assert_eq!(value["filters"]["language"], "react");
    assert_eq!(value["filters"]["limit"], 1);
    assert_eq!(value["results"].as_array().unwrap().len(), 1);
    assert_eq!(value["results"][0]["id"], "color.primary");
}

#[test]
fn component_and_token_detail_json_include_complete_sections() {
    let _guard = env_lock();
    let root = TestDir::new("detail-json");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let component = run_wax(
        &wax_home,
        &repo,
        &[
            "component",
            "ds.button",
            "--design-system",
            "acme",
            "--format",
            "json",
        ],
    );
    assert!(component.status.success());
    let component_value: Value =
        serde_json::from_str(String::from_utf8(component.stdout).unwrap().trim()).unwrap();
    assert!(detail_validator().is_valid(&component_value));
    assert_eq!(component_value["kind"], "component");
    assert_eq!(component_value["entry"]["legacy_flag"], true);
    assert_eq!(
        component_value["entry"]["metadata"]["description"],
        "Primary action control"
    );
    assert_eq!(
        component_value["entry"]["extracted"]["documentation"],
        "Use for primary actions."
    );

    let token = run_wax(
        &wax_home,
        &repo,
        &[
            "token",
            "color.primary",
            "--design-system",
            "acme",
            "--format",
            "json",
        ],
    );
    assert!(token.status.success());
    let token_value: Value =
        serde_json::from_str(String::from_utf8(token.stdout).unwrap().trim()).unwrap();
    assert!(detail_validator().is_valid(&token_value));
    assert_eq!(token_value["kind"], "token");
    assert_eq!(token_value["entry"]["value"], "#ff00aa");
}

#[test]
fn detail_reports_absence_and_ambiguity_as_typed_json_errors() {
    let _guard = env_lock();
    let root = TestDir::new("detail-errors");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let first = root.path.join("first.json");
    let second = root.path.join("second.json");
    write_registry(
        &first,
        r#"{"schema_version":1,"language":"react","components":[{"id":"shared","symbol":"One"}],"tokens":[]}"#,
    );
    write_registry(
        &second,
        r#"{"schema_version":1,"language":"compose","components":[{"id":"shared","symbol":"Two"}],"tokens":[]}"#,
    );
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "one", &first);
    let add_compose = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "registry",
            "add",
            "two",
            &second.display().to_string(),
            "--language",
            "compose",
            "--name",
            "Two",
        ])
        .env("WAX_HOME", &wax_home)
        .output()
        .unwrap();
    assert!(add_compose.status.success());

    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::copy(&first, repo.join("first.json")).unwrap();
    fs::copy(&second, repo.join("second.json")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {"source": "first.json"}
    },
    "compose": {
      "roots": ["src"],
      "registry": {"source": "second.json"}
    }
  }
}"#,
    )
    .unwrap();
    let first_bytes = fs::read(repo.join("first.json")).unwrap();
    let second_bytes = fs::read(repo.join("second.json")).unwrap();
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {{
    "react": {{
      "source": "first.json",
      "sha256": "{}"
    }},
    "compose": {{
      "source": "second.json",
      "sha256": "{}"
    }}
  }},
  "languages": {{}}
}}"#,
            sha256_hex(&first_bytes),
            sha256_hex(&second_bytes)
        ),
    )
    .unwrap();

    let missing = run_wax(
        &wax_home,
        &repo,
        &["component", "missing", "--format", "json"],
    );
    assert!(!missing.status.success());
    let missing_value: Value =
        serde_json::from_str(String::from_utf8(missing.stdout).unwrap().trim()).unwrap();
    assert!(error_validator().is_valid(&missing_value));
    assert_eq!(missing_value["code"], "entry_not_found");

    let ambiguous = run_wax(
        &wax_home,
        &repo,
        &["component", "shared", "--format", "json"],
    );
    assert!(!ambiguous.status.success());
    let ambiguous_value: Value =
        serde_json::from_str(String::from_utf8(ambiguous.stdout).unwrap().trim()).unwrap();
    assert!(error_validator().is_valid(&ambiguous_value));
    assert_eq!(ambiguous_value["code"], "ambiguous_entry");
}

#[test]
fn search_to_detail_preserves_snapshot_after_refresh() {
    let _guard = env_lock();
    let root = TestDir::new("search-to-detail-refresh");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let search = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "button",
            "--design-system",
            "acme",
            "--kind",
            "component",
            "--format",
            "json",
        ],
    );
    assert!(search.status.success());
    let search_value: Value =
        serde_json::from_str(String::from_utf8(search.stdout).unwrap().trim()).unwrap();
    let digest = search_value["results"][0]["snapshot"]["digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let source = search_value["results"][0]["snapshot"]["source"]
        .as_str()
        .expect("source")
        .to_owned();
    let component_id = search_value["results"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let token_search = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "ff00aa",
            "--design-system",
            "acme",
            "--kind",
            "token",
            "--format",
            "json",
        ],
    );
    assert!(token_search.status.success());
    let token_search_value: Value =
        serde_json::from_str(String::from_utf8(token_search.stdout).unwrap().trim()).unwrap();
    let token_digest = token_search_value["results"][0]["snapshot"]["digest"]
        .as_str()
        .expect("token digest")
        .to_owned();
    let token_id = token_search_value["results"][0]["id"]
        .as_str()
        .expect("token id")
        .to_owned();

    write_registry(
        &registry,
        r##"{
  "schema_version": 1,
  "language": "react",
  "components": [{"id": "ds.button", "symbol": "NewButton"}],
  "tokens": [{"id": "color.primary", "key": "color.primary", "category": "color", "value": "#00ff00"}]
}"##,
    );
    let refresh = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["registry", "refresh", "acme"])
        .env("WAX_HOME", &wax_home)
        .output()
        .unwrap();
    assert!(
        refresh.status.success(),
        "refresh failed: {}",
        String::from_utf8_lossy(&refresh.stderr)
    );

    let component = run_wax(
        &wax_home,
        &repo,
        &[
            "component",
            &component_id,
            "--design-system",
            "acme",
            "--snapshot",
            &digest,
            "--format",
            "json",
        ],
    );
    assert!(
        component.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&component.stderr)
    );
    let component_value: Value =
        serde_json::from_str(String::from_utf8(component.stdout).unwrap().trim()).unwrap();
    assert!(detail_validator().is_valid(&component_value));
    assert_eq!(component_value["snapshot"]["digest"], digest);
    assert_eq!(component_value["snapshot"]["source"], source);
    assert_eq!(component_value["entry"]["symbol"], "Button");
    assert!(
        component_value["entry"].get("package_version").is_none(),
        "registry pin must not claim installed package compatibility"
    );

    let token = run_wax(
        &wax_home,
        &repo,
        &[
            "token",
            &token_id,
            "--design-system",
            "acme",
            "--snapshot",
            &token_digest,
            "--format",
            "json",
        ],
    );
    assert!(token.status.success());
    let token_value: Value =
        serde_json::from_str(String::from_utf8(token.stdout).unwrap().trim()).unwrap();
    assert_eq!(token_value["snapshot"]["digest"], token_digest);
    assert_eq!(token_value["entry"]["value"], "#ff00aa");
}

#[test]
fn search_to_detail_preserves_app_lock_snapshot() {
    let _guard = env_lock();
    let root = TestDir::new("search-to-detail-app-lock");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::create_dir_all(repo.join("src")).unwrap();
    let registry_bytes = fixture_registry().as_bytes();
    fs::write(repo.join(".wax/react.registry.json"), registry_bytes).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {"source": ".wax/react.registry.json"}
    }
  }
}"#,
    )
    .unwrap();
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {{
    "react": {{
      "source": ".wax/react.registry.json",
      "sha256": "{}"
    }}
  }},
  "languages": {{}}
}}"#,
            sha256_hex(registry_bytes)
        ),
    )
    .unwrap();
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    fs::create_dir_all(&wax_home).unwrap();

    let search = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "button",
            "--kind",
            "component",
            "--format",
            "json",
        ],
    );
    assert!(
        search.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_value: Value =
        serde_json::from_str(String::from_utf8(search.stdout).unwrap().trim()).unwrap();
    let digest = search_value["results"][0]["snapshot"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();

    // A newer on-disk registry must not override the app lock when detailing by digest.
    fs::write(
        repo.join(".wax/react.registry.json"),
        r#"{"schema_version":1,"language":"react","components":[{"id":"ds.button","symbol":"NewButton"}],"tokens":[]}"#,
    )
    .unwrap();

    let detail = run_wax(
        &wax_home,
        &repo,
        &[
            "component",
            "ds.button",
            "--snapshot",
            &digest,
            "--format",
            "json",
        ],
    );
    assert!(
        detail.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&detail.stderr)
    );
    let detail_value: Value =
        serde_json::from_str(String::from_utf8(detail.stdout).unwrap().trim()).unwrap();
    assert_eq!(detail_value["snapshot"]["digest"], digest);
    assert_eq!(detail_value["entry"]["symbol"], "Button");
}

#[test]
fn selected_source_failure_fails_the_whole_query() {
    let _guard = env_lock();
    let root = TestDir::new("source-failure");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_wax(
        &wax_home,
        &repo,
        &[
            "search",
            "button",
            "--design-system",
            "/missing/registry.json",
            "--format",
            "json",
        ],
    );
    assert!(!output.status.success());
    let value: Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert!(error_validator().is_valid(&value));
    assert!(
        value["code"] == "source_not_found"
            || value["code"] == "invalid_argument"
            || value["code"] == "invalid_registry",
        "unexpected code: {}",
        value["code"]
    );
}

#[test]
fn text_search_and_detail_render_readable_sections() {
    let _guard = env_lock();
    let root = TestDir::new("text-output");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let registry = root.path.join("registry.json");
    write_registry(&registry, fixture_registry());
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);
    remember_local_registry(&wax_home, "acme", &registry);

    let search = run_wax(
        &wax_home,
        &repo,
        &["search", "button", "--design-system", "acme"],
    );
    assert!(search.status.success());
    let stdout = String::from_utf8(search.stdout).unwrap();
    assert!(stdout.contains("query: button"));
    assert!(stdout.contains("ds.button"));
    assert!(stdout.contains("summary: Primary action control"));

    let detail = run_wax(
        &wax_home,
        &repo,
        &["component", "ds.button", "--design-system", "acme"],
    );
    assert!(detail.status.success());
    let detail_stdout = String::from_utf8(detail.stdout).unwrap();
    assert!(detail_stdout.contains("kind: component"));
    assert!(detail_stdout.contains("id: ds.button"));
    assert!(detail_stdout.contains("metadata:"));
    assert!(detail_stdout.contains("extracted:"));
    assert!(detail_stdout.contains("Primary action control"));
}

#[test]
fn clap_argument_failure_emits_json_when_format_json_selected() {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["search", "--format", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(stdout.trim()).expect("json error on stdout");
    assert!(error_validator().is_valid(&value));
    assert_eq!(value["code"], "invalid_argument");
}
