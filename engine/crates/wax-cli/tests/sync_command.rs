use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_core::registry_memory::remember_design_system;

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
        let path = std::env::temp_dir().join(format!("wax-cli-sync-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_app_repo(app_repo: &Path, upstream: &str, source: &str) {
    fs::create_dir_all(app_repo.join(".wax/registries/acme")).expect("create registries dir");
    fs::write(
        app_repo.join(".wax/registries/acme/react.json"),
        r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
    )
    .expect("write app registry");
    fs::write(
        app_repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{
    "react": {{
      "roots": ["src"],
      "registry": {{
        "source": "{source}",
        "upstream": "{upstream}"
      }}
    }}
  }}
}}
"#
        ),
    )
    .expect("write app config");
    fs::write(
        app_repo.join(".wax/wax.lock.json"),
        r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {
    "react": {
      "source": ".wax/registries/acme/react.json",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  },
  "languages": {}
}
"#,
    )
    .expect("write app lockfile");
}

fn setup_remembered_local_ds(root: &Path) -> (PathBuf, PathBuf) {
    let ds_repo = root.join("acme-ds");
    fs::create_dir_all(ds_repo.join(".wax/registries")).expect("create ds registries dir");
    fs::write(
        ds_repo.join(".wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
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
    (ds_repo, wax_home)
}

#[test]
fn sync_command_copies_local_design_system_registry_changes() {
    let _guard = env_lock();
    let root = TestDir::new("sync-copy-local");
    let app_repo = root.path.join("app");
    write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
    let (ds_repo, wax_home) = setup_remembered_local_ds(&root.path);
    fs::write(
        ds_repo.join(".wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"name":"Button"},{"name":"Card"}]}"#,
    )
    .expect("update ds registry");

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["sync", "--repo-root"])
        .arg(&app_repo)
        .output()
        .expect("spawn wax sync");

    assert!(
        output.status.success(),
        "wax sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.contains("updated react registry from acme/react"));
    let copied = fs::read_to_string(app_repo.join(".wax/registries/acme/react.json"))
        .expect("read copied registry");
    assert!(copied.contains("Card"));
}

#[test]
fn sync_command_switches_app_registry_source_to_published_source() {
    let _guard = env_lock();
    let root = TestDir::new("sync-published-source");
    let app_repo = root.path.join("app");
    write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
    let (ds_repo, wax_home) = setup_remembered_local_ds(&root.path);
    let published_registry = ds_repo.join("published-react.registry.json");
    fs::write(
        &published_registry,
        r#"{"schema_version":1,"components":[{"name":"PublishedButton"}]}"#,
    )
    .expect("write published registry");
    let published_source = format!("file://{}", published_registry.display());
    fs::write(
        ds_repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "design_systems": {{
    "acme": {{
      "name": "Acme Design System",
      "registries": {{
        "react": {{
          "source": ".wax/registries/react.json",
          "published_source": "{published_source}"
        }}
      }}
    }}
  }}
}}
"#
        ),
    )
    .expect("write ds config with published source");

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["sync", "--repo-root"])
        .arg(&app_repo)
        .output()
        .expect("spawn wax sync");

    assert!(
        output.status.success(),
        "wax sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let app_config =
        fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("read config");
    assert!(app_config.contains(&published_source));
}

#[test]
fn sync_command_fails_when_upstream_design_system_is_not_remembered() {
    let _guard = env_lock();
    let root = TestDir::new("sync-missing-memory");
    let app_repo = root.path.join("app");
    write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    fs::write(
        wax_home.join("state.json"),
        r#"{"installed_languages":{},"design_systems":{}}"#,
    )
    .expect("write empty state");

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["sync", "--repo-root"])
        .arg(&app_repo)
        .output()
        .expect("spawn wax sync");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(stderr.contains("acme"));
}
