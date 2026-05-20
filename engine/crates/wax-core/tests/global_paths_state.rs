use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use wax_contract::LanguageId;
use wax_core::global_state::{
    GlobalState, GlobalStateError, InstalledLanguagePack, load_global_state, save_global_state,
};
use wax_core::paths::{lang_install_dir, state_file, wax_home};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "wax-core-global-state-{name}-{}",
        std::process::id()
    ))
}

struct TestDir {
    path: PathBuf,
}

struct CurrentDirGuard {
    previous: PathBuf,
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
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = TestDir::new("wax-home-override");
    let previous = std::env::var_os("WAX_HOME");

    unsafe {
        std::env::set_var("WAX_HOME", dir.path());
    }
    let resolved = wax_home();
    restore_wax_home(previous);

    assert_eq!(resolved, dir.path());
}

#[test]
fn state_file_lives_under_wax_home() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = TestDir::new("state-file");
    let previous = std::env::var_os("WAX_HOME");

    unsafe {
        std::env::set_var("WAX_HOME", dir.path());
    }
    let resolved = state_file();
    restore_wax_home(previous);

    assert_eq!(resolved, dir.path().join("state.json"));
}

#[test]
fn lang_install_dir_uses_validated_language_id_and_version() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = TestDir::new("lang-install-dir");
    let previous = std::env::var_os("WAX_HOME");
    let language_id = LanguageId::try_from("react").unwrap();

    unsafe {
        std::env::set_var("WAX_HOME", dir.path());
    }
    let resolved = lang_install_dir(&language_id, "1.2.3");
    restore_wax_home(previous);

    assert_eq!(resolved, dir.path().join("langs/react/1.2.3"));
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
fn save_global_state_accepts_current_dir_filename() {
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = TestDir::new("current-dir-filename");
    let _cwd = CurrentDirGuard::enter(dir.path());

    save_global_state("state.json", &GlobalState::default()).unwrap();
    let loaded = load_global_state("state.json").unwrap();

    assert_eq!(loaded, GlobalState::default());
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

fn restore_wax_home(previous: Option<std::ffi::OsString>) {
    unsafe {
        match previous {
            Some(value) => std::env::set_var("WAX_HOME", value),
            None => std::env::remove_var("WAX_HOME"),
        }
    }
}
