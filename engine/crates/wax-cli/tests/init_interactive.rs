use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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
fn init_without_non_interactive_requires_tty() {
    let root = TestDir::new("init-non-tty");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("init")
        .arg("--no-install")
        .arg("--repo-root")
        .arg(&repo)
        .stdin(Stdio::null())
        .output()
        .expect("run wax init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wax init needs an interactive terminal"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("wax init --non-interactive --language <language-id>"),
        "stderr was: {stderr}"
    );
}

#[test]
fn init_rejects_existing_config_before_tty_check() {
    let root = TestDir::new("init-existing-config");
    let repo = root.path.join("repo");
    let config_path = repo.join(".wax/wax.config.json");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("create .wax");
    fs::write(&config_path, "{}\n").expect("write existing config");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("init")
        .arg("--no-install")
        .arg("--repo-root")
        .arg(&repo)
        .stdin(Stdio::null())
        .output()
        .expect("run wax init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wax config already exists"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("wax init needs an interactive terminal"),
        "existing config should be rejected before TTY check; stderr was: {stderr}"
    );
}
