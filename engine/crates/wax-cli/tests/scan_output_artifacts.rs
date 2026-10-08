mod common;

use common::{
    assert_deferred_format, assert_schema_valid_summary, env_lock, run_scan, setup_scan_repo,
    write_repo_files,
};
use std::fs;
use std::path::PathBuf;

#[test]
fn cli_output_writes_json_summary() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-cli",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &["--output", "json-summary=.wax/out/scan-summary.json"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let path = repo.join(".wax/out/scan-summary.json");
    assert!(path.exists());
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_schema_valid_summary(&value);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("artifacts:"));
    assert!(stdout.contains("json-summary: .wax/out/scan-summary.json"));
}

#[test]
fn each_written_json_summary_lists_all_requested_artifacts() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-manifest-complete",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &[
            "--output",
            "json-summary=.wax/out/summary-a.json",
            "--output",
            "json-summary=.wax/out/summary-b.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for name in ["summary-a.json", "summary-b.json"] {
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(repo.join(".wax/out").join(name)).unwrap())
                .unwrap();
        assert_schema_valid_summary(&value);
        let paths: Vec<&str> = value["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect();
        assert!(
            paths.contains(&".wax/out/summary-a.json"),
            "{name} missing summary-a in artifacts: {paths:?}"
        );
        assert!(
            paths.contains(&".wax/out/summary-b.json"),
            "{name} missing summary-b in artifacts: {paths:?}"
        );
    }
}

#[test]
fn config_outputs_union_with_cli_idempotent() {
    let _guard = env_lock();
    let root = common::TestDir::new("scan-artifact-union");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    common::write_pack_index(&registry_file);
    write_repo_files(
        &repo,
        &registry_file,
        &["compose"],
        Some(
            r#"[
    {"format":"json-summary","path":".wax/out/from-config.json"},
    {"format":"json-summary","path":".wax/out/from-cli.json"}
  ]"#,
        ),
    );
    common::write_installed_packs(&wax_home, &[("compose", "complete", "0.5", "", "")]);
    let _wax_home = common::EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(
        &repo,
        &[
            "--output",
            "json-summary=.wax/out/from-cli.json",
            "--output",
            "json-summary=.wax/out/from-extra.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(repo.join(".wax/out/from-config.json").exists());
    assert!(repo.join(".wax/out/from-cli.json").exists());
    assert!(repo.join(".wax/out/from-extra.json").exists());
}

#[test]
fn duplicate_output_pair_idempotent() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-dedupe",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &[
            "--output",
            "json-summary=.wax/out/scan-summary.json",
            "--output",
            "json-summary=.wax/out/scan-summary.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .matches("json-summary: .wax/out/scan-summary.json")
            .count(),
        1
    );
}

#[test]
fn shared_destination_across_formats_rejected() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-shared-dest",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &[
            "--output",
            "graph-data=.wax/out/shared.json",
            "--output",
            "json-summary=.wax/out/shared.json",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("already requested for format `graph-data`")
            || stderr.contains("already requested for format `json-summary`"),
        "expected destination conflict error, got: {stderr}"
    );
    assert!(
        !repo.join(".wax/out/shared.json").exists(),
        "conflicting outputs must not write a shared destination"
    );
}

#[test]
fn unknown_artifact_format_errors_distinctly_from_parse_shape() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-unknown-format",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "csv=.wax/out/summary.csv"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unknown output format `csv`")
            || stderr.contains("unsupported output format `csv`"),
        "expected dedicated unknown-format error, got: {stderr}"
    );
    assert!(
        !stderr.contains("expected FORMAT=PATH"),
        "unknown format must not reuse parse-shape InvalidOutputFlag: {stderr}"
    );
}

#[test]
fn deferred_html_errors() {
    assert_deferred_format("html");
}

#[test]
fn deferred_markdown_errors() {
    assert_deferred_format("markdown");
}

#[test]
fn absolute_path_requires_allow_flag() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-absolute-denied",
        &[("compose", "complete", "0.5", "", "")],
    );
    let abs = repo
        .join(".wax/out/absolute-summary.json")
        .display()
        .to_string();

    let output = run_scan(&repo, &["--output", &format!("json-summary={abs}")]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("requires --allow-absolute-output"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn parent_traversal_output_path_rejected() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-parent-escape",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "json-summary=../outside-summary.json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("escapes the repository")
            || stderr.contains("must stay within the repository"),
        "unexpected stderr: {stderr}"
    );
    assert!(!repo.join("../outside-summary.json").exists());
}

#[test]
fn nested_parent_escape_output_path_rejected() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-nested-escape",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &["--output", "json-summary=.wax/out/../../../outside.json"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("escapes the repository")
            || stderr.contains("must stay within the repository"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn output_colliding_with_scan_merged_rejected() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-merged-collision",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &["--output", "json-summary=.wax/out/scan-merged.json"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("collides with the canonical scan output"),
        "unexpected stderr: {stderr}"
    );

    assert!(
        repo.join(".wax/out/scan-merged.json").exists(),
        "path-policy errors run after merge so the canonical scan artifact is still written"
    );
}

#[test]
fn allow_absolute_output_writes() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-absolute-allowed",
        &[("compose", "complete", "0.5", "", "")],
    );
    let abs = repo.join(".wax/out/absolute-summary.json");
    let abs_str = abs.display().to_string();

    let output = run_scan(
        &repo,
        &[
            "--allow-absolute-output",
            "--output",
            &format!("json-summary={abs_str}"),
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(abs.exists());
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(abs).unwrap()).unwrap();
    assert_schema_valid_summary(&value);
}

#[test]
fn output_flag_missing_equals_errors() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-missing-eq",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "json-summary"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("invalid --output value"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn output_flag_empty_path_errors() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-empty-path",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "json-summary="]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("invalid --output value"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn output_flag_empty_format_errors() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-artifact-empty-format",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "=.wax/out/x.json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("invalid --output value"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn config_absolute_path_requires_allow_flag() {
    let _guard = env_lock();
    let root = common::TestDir::new("scan-artifact-config-abs");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    common::write_pack_index(&registry_file);
    let abs = PathBuf::from("/tmp/wax-scan-summary-absolute.json");
    write_repo_files(
        &repo,
        &registry_file,
        &["compose"],
        Some(&format!(
            r#"[{{"format":"json-summary","path":"{}"}}]"#,
            abs.display()
        )),
    );
    common::write_installed_packs(&wax_home, &[("compose", "complete", "0.5", "", "")]);
    let _wax_home = common::EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(&repo, &[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("requires --allow-absolute-output"),
        "unexpected stderr: {stderr}"
    );
}
