mod common;

use common::{
    EnvVarGuard, TestDir, assert_schema_valid_summary, env_lock, run_scan, setup_scan_repo,
    write_committed_scan_repo_with_upstream, write_grouped_repo_files, write_installed_packs,
    write_pack_index,
};
use std::fs;
use std::process::Command;

#[test]
fn format_summary_default_matches_legacy_prefix() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-summary",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let merged = repo.join(".wax/out/scan-merged.json");
    assert!(stdout.starts_with(&format!("scan output: {}", merged.display())));
    assert!(stdout.contains("language status:"));
    assert!(stdout.contains("compose: complete"));
}

#[test]
fn format_quiet_path_only() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-quiet",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "quiet"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let merged = repo.join(".wax/out/scan-merged.json");
    assert_eq!(stdout.trim(), merged.display().to_string());
    assert!(!stdout.contains("language status:"));
    assert!(merged.exists());
}

#[test]
fn format_json_summary_is_sole_stdout_and_schema_valid() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-json",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "json-summary"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("scan output:"));
    assert!(!stdout.contains("language status:"));
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_schema_valid_summary(&value);
}

#[test]
fn format_json_summary_has_schema_version_2() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-json-version",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "json-summary"]);
    assert!(output.status.success());
    let value: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert_eq!(value["schema_version"], 2);
}

#[test]
fn format_json_summary_uses_repo_relative_paths() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-json-relative-paths",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "json-summary"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert_eq!(value["repo_root"], ".");
    assert_eq!(value["scan_path"], ".wax/out/scan-merged.json");
    let repo_display = repo.display().to_string();
    assert!(
        !value["repo_root"].as_str().unwrap().starts_with('/')
            && !value["scan_path"].as_str().unwrap().contains(&repo_display),
        "json-summary paths must be portable, got repo_root={} scan_path={}",
        value["repo_root"],
        value["scan_path"]
    );
}

#[test]
fn format_and_output_json_summary_both_schema_valid() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-and-output",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &[
            "--format",
            "json-summary",
            "--output",
            "json-summary=.wax/out/scan-summary.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_value: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert_schema_valid_summary(&stdout_value);

    let file_path = repo.join(".wax/out/scan-summary.json");
    assert!(file_path.exists());
    let file_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(file_path).unwrap()).unwrap();
    assert_schema_valid_summary(&file_value);
    assert!(
        !file_value["artifacts"].as_array().unwrap().is_empty()
            || !stdout_value["artifacts"].as_array().unwrap().is_empty()
    );
    assert!(!stdout_value["artifacts"].as_array().unwrap().is_empty());
}

#[test]
fn root_group_json_summary_scoped() {
    let _guard = env_lock();
    let root = TestDir::new("scan-format-root-group");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    write_grouped_repo_files(&repo, &registry_file);
    write_installed_packs(
        &wax_home,
        &[
            ("compose", "complete", "0.5", "", ""),
            ("react", "complete", "0.5", "", ""),
        ],
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(
        &repo,
        &["--format", "json-summary", "--root-group", "mobile"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert_schema_valid_summary(&value);
    let languages = value["languages"].as_array().unwrap();
    assert_eq!(languages.len(), 1);
    assert_eq!(languages[0]["id"], "compose");
}

#[test]
fn json_summary_sync_warning_goes_to_stderr() {
    let _guard = env_lock();
    let root = TestDir::new("scan-format-sync-stderr");
    let app_repo = root.path.join("app");
    write_committed_scan_repo_with_upstream(&app_repo);
    fs::create_dir_all(app_repo.join("src")).unwrap();

    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).unwrap();
    fs::write(
        wax_home.join("state.json"),
        r#"{"installed_languages":{},"design_systems":{}}"#,
    )
    .unwrap();

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--repo-root"])
        .arg(&app_repo)
        .args(["--format", "json-summary", "--concurrency", "1"])
        .output()
        .expect("spawn wax scan");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stdout.contains("warning: registry sync failed"),
        "warning leaked to stdout: {stdout}"
    );
    assert!(
        stderr.contains("warning: registry sync failed"),
        "expected sync warning on stderr, got: {stderr}"
    );
}
