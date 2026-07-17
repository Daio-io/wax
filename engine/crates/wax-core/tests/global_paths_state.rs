use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use wax_contract::LanguageId;
use wax_core::global_state::{
    GlobalState, GlobalStateError, InstalledLanguagePack, load_global_state, save_global_state,
};
use wax_core::paths::{PathsError, lang_install_dir, state_file, wax_home};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "wax-core-global-state-{name}-{}",
        std::process::id()
    ))
}

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct TestDir {
    path: PathBuf,
}

struct CurrentDirGuard {
    previous: PathBuf,
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
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

    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while mutating process environment variables, which keeps env access serialized inside this test binary"
    )]
    fn remove(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::remove_var(name);
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

impl CurrentDirGuard {
    fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).unwrap();
    }
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path = temp_path(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn wax_home_uses_wax_home_override() {
    let _guard = env_lock();
    let dir = TestDir::new("wax-home-override");
    let _wax_home = EnvVarGuard::set("WAX_HOME", dir.path());
    let _home = EnvVarGuard::remove("HOME");

    let resolved = wax_home().unwrap();
    assert_eq!(resolved, dir.path());
}

#[cfg(not(windows))]
#[test]
fn wax_home_uses_home_when_wax_home_is_empty() {
    let _guard = env_lock();
    let dir = TestDir::new("empty-wax-home");
    let _wax_home = EnvVarGuard::set("WAX_HOME", "");
    let _home = EnvVarGuard::set("HOME", dir.path());

    let resolved = wax_home().unwrap();

    assert_eq!(resolved, dir.path().join(".wax"));
}

#[test]
fn state_file_lives_under_wax_home() {
    let _guard = env_lock();
    let dir = TestDir::new("state-file");
    let _wax_home = EnvVarGuard::set("WAX_HOME", dir.path());

    let resolved = state_file().unwrap();
    assert_eq!(resolved, dir.path().join("state.json"));
}

#[test]
fn lang_install_dir_uses_validated_language_id_and_version() {
    let _guard = env_lock();
    let dir = TestDir::new("lang-install-dir");
    let _wax_home = EnvVarGuard::set("WAX_HOME", dir.path());
    let language_id = LanguageId::try_from("react").unwrap();

    let resolved = lang_install_dir(&language_id, "1.2.3").unwrap();
    assert_eq!(resolved, dir.path().join("langs/react/1.2.3"));
}

#[test]
fn lang_install_dir_rejects_versions_that_escape_version_segment() {
    let _guard = env_lock();
    let dir = TestDir::new("lang-install-invalid");
    let _wax_home = EnvVarGuard::set("WAX_HOME", dir.path());
    let language_id = LanguageId::try_from("react").unwrap();

    for version in [
        "../other",
        "a/b",
        "/tmp/react",
        ".",
        "",
        "1.2.3/",
        "1.2.3/.",
        "1.2.3//",
    ] {
        let err = lang_install_dir(&language_id, version).unwrap_err();

        assert!(matches!(err, PathsError::InvalidVersion { .. }));
        assert!(err.to_string().contains(version));
    }
}

#[cfg(not(windows))]
#[test]
fn wax_home_errors_when_home_is_unavailable() {
    let _guard = env_lock();
    let _wax_home = EnvVarGuard::remove("WAX_HOME");
    let _home = EnvVarGuard::remove("HOME");

    let err = wax_home().unwrap_err();

    assert!(matches!(err, PathsError::HomeUnavailable));
}

#[cfg(not(windows))]
#[test]
fn wax_home_errors_when_home_is_empty() {
    let _guard = env_lock();
    let _wax_home = EnvVarGuard::remove("WAX_HOME");
    let _home = EnvVarGuard::set("HOME", "");

    let err = wax_home().unwrap_err();

    assert!(matches!(err, PathsError::HomeUnavailable));
}

#[test]
fn load_global_state_returns_default_when_missing() {
    let dir = TestDir::new("missing-default");
    let state = load_global_state(dir.path().join("state.json")).unwrap();

    assert_eq!(state, GlobalState::default());
}

#[test]
fn save_global_state_creates_parent_dirs_and_loads_roundtrip() {
    let dir = TestDir::new("roundtrip");
    let path = dir.path().join("nested/state.json");
    let language_id = LanguageId::try_from("compose").unwrap();
    let mut installed_languages = BTreeMap::new();
    installed_languages.insert(
        language_id.clone(),
        BTreeMap::from([(
            "0.4.2".to_string(),
            InstalledLanguagePack {
                install_dir: dir.path().join("langs/compose/0.4.2"),
            },
        )]),
    );
    let state = GlobalState {
        installed_languages,
        ..GlobalState::default()
    };

    save_global_state(&path, &state).unwrap();

    let loaded = load_global_state(&path).unwrap();
    assert_eq!(loaded, state);
    assert_eq!(
        loaded.installed_languages[&language_id]["0.4.2"].install_dir,
        dir.path().join("langs/compose/0.4.2")
    );
}

#[test]
fn save_global_state_replaces_existing_file() {
    let dir = TestDir::new("replace-existing");
    let path = dir.path().join("state.json");
    let compose_id = LanguageId::try_from("compose").unwrap();
    let react_id = LanguageId::try_from("react").unwrap();
    let first = GlobalState {
        installed_languages: BTreeMap::from([(
            compose_id,
            BTreeMap::from([(
                "0.4.2".to_string(),
                InstalledLanguagePack {
                    install_dir: dir.path().join("langs/compose/0.4.2"),
                },
            )]),
        )]),
        ..GlobalState::default()
    };
    let second = GlobalState {
        installed_languages: BTreeMap::from([(
            react_id.clone(),
            BTreeMap::from([(
                "1.0.0".to_string(),
                InstalledLanguagePack {
                    install_dir: dir.path().join("langs/react/1.0.0"),
                },
            )]),
        )]),
        ..GlobalState::default()
    };

    save_global_state(&path, &first).unwrap();
    save_global_state(&path, &second).unwrap();

    let loaded = load_global_state(&path).unwrap();
    assert_eq!(loaded, second);
    assert!(loaded.installed_languages.contains_key(&react_id));
    assert_eq!(loaded.installed_languages.len(), 1);
}

#[test]
fn save_global_state_rejects_invalid_version_keys() {
    let dir = TestDir::new("save-invalid-version");
    let path = dir.path().join("state.json");
    let language_id = LanguageId::try_from("react").unwrap();
    let state = GlobalState {
        installed_languages: BTreeMap::from([(
            language_id,
            BTreeMap::from([(
                "1.2.3/.".to_string(),
                InstalledLanguagePack {
                    install_dir: dir.path().join("langs/react/1.2.3"),
                },
            )]),
        )]),
        ..GlobalState::default()
    };

    let err = save_global_state(&path, &state).unwrap_err();

    assert!(matches!(err, GlobalStateError::InvalidVersion { .. }));
    assert!(err.to_string().contains("react"));
    assert!(err.to_string().contains("1.2.3/."));
    assert!(!path.exists());
}

#[test]
fn save_global_state_accepts_current_dir_filename() {
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = TestDir::new("current-dir-filename");
    let _cwd = CurrentDirGuard::enter(dir.path());

    save_global_state("state.json", &GlobalState::default()).unwrap();
    let loaded = load_global_state("state.json").unwrap();

    assert_eq!(loaded, GlobalState::default());
}

#[test]
fn save_global_state_reports_rename_error_when_destination_is_directory() {
    let dir = TestDir::new("rename-directory");
    let path = dir.path().join("state.json");
    std::fs::create_dir(&path).unwrap();

    let err = save_global_state(&path, &GlobalState::default()).unwrap_err();

    assert!(matches!(err, GlobalStateError::Rename { .. }));
    assert!(err.to_string().contains("state.json"));
}

#[test]
fn load_global_state_reports_malformed_json() {
    let dir = TestDir::new("malformed");
    let path = dir.path().join("state.json");
    std::fs::write(&path, "{").unwrap();

    let err = load_global_state(&path).unwrap_err();

    assert!(matches!(err, GlobalStateError::MalformedJson { .. }));
    assert!(err.to_string().contains("malformed wax global state"));
    assert!(err.to_string().contains("state.json"));
}

#[test]
fn load_global_state_rejects_invalid_language_ids() {
    let dir = TestDir::new("invalid-language-id");
    let path = dir.path().join("state.json");
    std::fs::write(
        &path,
        r#"{"installed_languages":{"React":{"1.0.0":{"install_dir":"/tmp/react"}}}}"#,
    )
    .unwrap();

    let err = load_global_state(&path).unwrap_err();

    assert!(matches!(err, GlobalStateError::InvalidState { .. }));
    assert!(err.to_string().contains("invalid language id"));
}

#[test]
fn load_global_state_rejects_invalid_version_keys() {
    let dir = TestDir::new("load-invalid-version");
    let path = dir.path().join("state.json");
    std::fs::write(
        &path,
        r#"{"installed_languages":{"react":{"1.2.3/.":{"install_dir":"/tmp/react"}}}}"#,
    )
    .unwrap();

    let err = load_global_state(&path).unwrap_err();

    assert!(matches!(err, GlobalStateError::InvalidVersion { .. }));
    assert!(err.to_string().contains("react"));
    assert!(err.to_string().contains("1.2.3/."));
}
