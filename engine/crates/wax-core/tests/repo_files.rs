use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use wax_core::config::repo_files::{RepoFileSet, discover_repo_files};

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "wax-core-repo-files-{label}-{}-{nonce}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn prefers_centralized_config_and_lock_paths() {
    let root = TestRepo::new("prefers-centralized");
    fs::create_dir_all(root.path().join(".wax")).unwrap();
    fs::write(root.path().join(".wax/wax.config.json"), "{}\n").unwrap();
    fs::write(root.path().join(".wax/wax.lock.json"), "{}\n").unwrap();
    fs::write(root.path().join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path().join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(root.path());

    assert_eq!(
        files,
        RepoFileSet {
            config_path: root.path().join(".wax/wax.config.json"),
            lockfile_path: root.path().join(".wax/wax.lock.json"),
        }
    );
}

#[test]
fn ignores_legacy_config_and_lock_paths() {
    let root = TestRepo::new("legacy-ignored");
    fs::write(root.path().join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path().join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".wax/wax.config.json"));
    assert_eq!(files.lockfile_path, root.path().join(".wax/wax.lock.json"));
}

#[test]
fn returns_preferred_paths_when_files_do_not_exist() {
    let root = TestRepo::new("preferred-defaults");

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".wax/wax.config.json"));
    assert_eq!(files.lockfile_path, root.path().join(".wax/wax.lock.json"));
}
