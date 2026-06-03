use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use wax_core::config::repo_files::{RepoFileSet, RepoFileWarning, discover_repo_files};

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
            warnings: vec![
                RepoFileWarning::IgnoredLegacyConfig {
                    preferred: root.path().join(".wax/wax.config.json"),
                    legacy: root.path().join(".waxrc"),
                },
                RepoFileWarning::IgnoredLegacyLockfile {
                    preferred: root.path().join(".wax/wax.lock.json"),
                    legacy: root.path().join("wax.lock.json"),
                },
            ],
        }
    );
}

#[test]
fn falls_back_to_legacy_config_and_lock_paths() {
    let root = TestRepo::new("legacy-fallback");
    fs::write(root.path().join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path().join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".waxrc"));
    assert_eq!(files.lockfile_path, root.path().join("wax.lock.json"));
    assert!(files.warnings.is_empty());
}

#[test]
fn returns_preferred_paths_when_files_do_not_exist() {
    let root = TestRepo::new("preferred-defaults");

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".wax/wax.config.json"));
    assert_eq!(files.lockfile_path, root.path().join(".wax/wax.lock.json"));
    assert!(files.warnings.is_empty());
}

#[test]
fn falls_back_when_preferred_paths_are_directories() {
    let root = TestRepo::new("directory-fallback");
    fs::create_dir_all(root.path().join(".wax/wax.config.json")).unwrap();
    fs::create_dir_all(root.path().join(".wax/wax.lock.json")).unwrap();
    fs::write(root.path().join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path().join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".waxrc"));
    assert_eq!(files.lockfile_path, root.path().join("wax.lock.json"));
    assert!(files.warnings.is_empty());
}

#[test]
fn mixes_preferred_config_with_legacy_lockfile() {
    let root = TestRepo::new("partial-layout");
    fs::create_dir_all(root.path().join(".wax")).unwrap();
    fs::write(root.path().join(".wax/wax.config.json"), "{}\n").unwrap();
    fs::write(root.path().join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(root.path());

    assert_eq!(files.config_path, root.path().join(".wax/wax.config.json"));
    assert_eq!(files.lockfile_path, root.path().join("wax.lock.json"));
    assert_eq!(
        files.warnings,
        vec![RepoFileWarning::PreferredConfigWithLegacyLockfile {
            preferred_config: root.path().join(".wax/wax.config.json"),
            legacy_lockfile: root.path().join("wax.lock.json"),
        }]
    );
}
