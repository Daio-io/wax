//! Tests that CLI progress spinners stay off stderr when it is not a TTY.

use std::fs;
use std::path::{Path, PathBuf};
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

fn write_minimal_validate_repo(repo: &Path) {
    write_repo(repo, "design-system/registry.json", "[]");
}

fn write_repo(repo: &Path, registry_path: &str, components: &str) {
    let registry_abs = repo.join(registry_path);
    if let Some(parent) = registry_abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        &registry_abs,
        format!("{{\n  \"schema_version\": 1,\n  \"components\": {components}\n}}\n"),
    )
    .unwrap();

    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            "{{\n  \"schema_version\": 2,\n  \"languages\": {{\"compose\": {{\"registry\": \"{registry_path}\"}}}}\n}}\n"
        ),
    )
    .unwrap();

    let resolved = wax_core::registry_source::resolve_registry_source(
        wax_core::registry_source::RegistrySourceInput {
            repo_root: repo,
            language_id: "compose",
            source: Some(registry_path),
        },
    )
    .unwrap();
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "registries": {{
    "compose": {{
      "source": "{registry_path}",
      "sha256": "{}"
    }}
  }},
  "languages": {{
    "compose": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file:///tmp/registry.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}
  }}
}}
"#,
            resolved.sha256
        ),
    )
    .unwrap();
}

fn stderr_has_no_progress_spinner(stderr: &str) {
    assert!(!stderr.contains("__      __"), "{stderr}");
    assert!(!stderr.contains('\x1b'), "{stderr}");
    assert!(
        !stderr.contains("Loading wax configuration"),
        "piped stderr should not include validate progress text: {stderr}"
    );
    assert!(
        !stderr.contains("Preparing scan"),
        "piped stderr should not include scan progress text: {stderr}"
    );
    assert!(
        !stderr.contains("Scanning languages ("),
        "piped stderr should not include parallel scan progress text: {stderr}"
    );
    assert!(
        !stderr
            .chars()
            .any(|ch| ('\u{28b8}'..='\u{28ff}').contains(&ch)),
        "piped stderr should not include indicatif braille spinner glyphs: {stderr}"
    );
}

#[test]
fn validate_command_stderr_piped_suppresses_progress_spinner() {
    let root = TestDir::new("progress-validate-piped");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");
    write_minimal_validate_repo(repo.as_path());

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_HOME", root.path.join("wax-home"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run wax validate");

    assert!(
        output.status.success(),
        "validate should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    stderr_has_no_progress_spinner(&stderr);
}

#[test]
fn scan_command_stderr_piped_suppresses_progress_spinner() {
    let root = TestDir::new("progress-scan-piped");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo");
    write_minimal_validate_repo(repo.as_path());

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_HOME", root.path.join("wax-home"))
        .args([
            "scan",
            "--repo-root",
            repo.to_str().expect("utf-8 repo path"),
            "--no-auto-install",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run wax scan");

    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    stderr_has_no_progress_spinner(&stderr);
}

#[test]
fn explicit_plain_progress_is_disabled_and_cleanup_is_repeatable() {
    let progress = wax_cli::progress::CliProgress::for_presentation(
        wax_cli::presentation::Presentation::Plain,
    );
    assert!(!progress.is_enabled());
    progress.set_message("must stay invisible");
    progress.finish();
    progress.finish();
}

#[test]
fn all_piped_scan_formats_preserve_errors_without_decoration() {
    let root = TestDir::new("progress-formats");
    for args in [
        vec!["--compact"],
        vec!["--format", "json-summary"],
        vec!["--format", "quiet"],
        vec!["--format", "markdown"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wax"))
            .args(["scan", "--no-auto-install", "--repo-root"])
            .arg(&root.path)
            .args(args)
            .env("WAX_HOME", root.path.join("home"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("error:"), "{stderr}");
        stderr_has_no_progress_spinner(&stderr);
    }
}
