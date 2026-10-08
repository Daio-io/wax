mod common;

use common::{assert_schema_valid_summary, env_lock, run_scan, setup_scan_repo};
use std::fs;

#[test]
fn format_markdown_stdout_has_headline_and_table() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-stdout",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "markdown"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("# Wax scan\n\nAdoption: **100.0%**"));
    assert!(stdout.contains("| Language | Status | Coverage | Resolved | Candidate |"));
    assert!(stdout.contains("| compose | complete | 100.0% | 1 | 1 |"));
    assert!(!stdout.contains("scan output:"));
}

#[test]
fn markdown_artifact_written() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-artifact",
        &[(
            "compose",
            "partial",
            "0.5",
            "parse_failed",
            "fixture failed",
        )],
    );

    let output = run_scan(
        &repo,
        &[
            "--output",
            "markdown=.wax/out/scan-summary.md",
            "--output",
            "json-summary=.wax/out/scan-summary.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let markdown = fs::read_to_string(repo.join(".wax/out/scan-summary.md")).unwrap();
    assert!(markdown.contains("## Diagnostics\n\n- `parse_failed` (compose): fixture failed"));
    assert!(markdown.contains("## Artifacts"));
    assert!(markdown.contains("[json-summary](.wax/out/scan-summary.json)"));
    assert!(markdown.contains("## Limits"));
    assert!(markdown.contains("module rollups are not available"));
}

#[test]
fn markdown_diagnostics_are_capped_at_ten() {
    let _guard = env_lock();
    let specs = [
        ("lang00", "partial", "0.5", "error_00", "failure 00"),
        ("lang01", "partial", "0.5", "error_01", "failure 01"),
        ("lang02", "partial", "0.5", "error_02", "failure 02"),
        ("lang03", "partial", "0.5", "error_03", "failure 03"),
        ("lang04", "partial", "0.5", "error_04", "failure 04"),
        ("lang05", "partial", "0.5", "error_05", "failure 05"),
        ("lang06", "partial", "0.5", "error_06", "failure 06"),
        ("lang07", "partial", "0.5", "error_07", "failure 07"),
        ("lang08", "partial", "0.5", "error_08", "failure 08"),
        ("lang09", "partial", "0.5", "error_09", "failure 09"),
        ("lang10", "partial", "0.5", "error_10", "failure 10"),
    ];
    let (_root, repo, _wax_home) = setup_scan_repo("scan-markdown-diagnostic-cap", &specs);

    let output = run_scan(&repo, &["--format", "markdown"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let diagnostic_rows = stdout
        .lines()
        .filter(|line| line.starts_with("- `error_"))
        .count();

    assert_eq!(diagnostic_rows, 10);
    assert!(!stdout.contains("error_10"));
}

#[test]
fn baseline_json_summary_emits_deltas() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-json-baseline",
        &[(
            "compose",
            "partial",
            "0.5",
            "current_error",
            "current failure",
        )],
    );

    let initial = run_scan(&repo, &["--format", "json-summary"]);
    assert!(initial.status.success());
    let mut baseline: serde_json::Value =
        serde_json::from_slice(&initial.stdout).expect("json-summary stdout");
    baseline["adoption"]["coverage_ratio"] = serde_json::json!(0.5);
    baseline["adoption"]["raw_invocations"]["resolved"] = serde_json::json!(0);
    baseline["adoption"]["raw_invocations"]["candidate"] = serde_json::json!(2);
    baseline["diagnostics"] = serde_json::json!([{
        "severity": "error",
        "code": "old_error",
        "message": "old failure",
        "language": "compose"
    }]);
    let baseline_path = repo.join("prior-summary.json");
    fs::write(
        &baseline_path,
        serde_json::to_vec_pretty(&baseline).unwrap(),
    )
    .unwrap();

    let output = run_scan(
        &repo,
        &[
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json-summary",
            "--output",
            "markdown=.wax/out/baseline-summary.md",
            "--output",
            "json-summary=.wax/out/baseline-summary.json",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_schema_valid_summary(&value);
    assert_eq!(value["deltas"]["adoption_coverage_delta"], 0.5);
    assert_eq!(value["deltas"]["resolved_delta"], 1);
    assert_eq!(value["deltas"]["candidate_delta"], -1);
    assert_eq!(
        value["deltas"]["new_error_diagnostics"][0]["code"],
        "current_error"
    );
    assert_eq!(
        value["deltas"]["resolved_diagnostics"][0]["code"],
        "old_error"
    );
    let markdown = fs::read_to_string(repo.join(".wax/out/baseline-summary.md")).unwrap();
    assert!(markdown.contains("## Changes"));
    assert!(markdown.contains("Adoption coverage: +50.0 percentage points"));
    let artifact: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".wax/out/baseline-summary.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(artifact["deltas"]["resolved_delta"], 1);
}

#[test]
fn baseline_merged_scan_supported() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-merged-baseline",
        &[("compose", "complete", "0.5", "", "")],
    );

    let initial = run_scan(&repo, &["--format", "quiet"]);
    assert!(initial.status.success());
    let baseline_path = repo.join("prior-merged.json");
    fs::copy(repo.join(".wax/out/scan-merged.json"), &baseline_path).unwrap();

    let output = run_scan(
        &repo,
        &[
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "markdown",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("## Changes"));
    assert!(stdout.contains("Resolved invocations: +0"));
    assert!(stdout.contains("Candidate invocations: +0"));
}

#[test]
fn missing_baseline_fails() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-missing-baseline",
        &[("compose", "complete", "0.5", "", "")],
    );
    let missing = repo.join("missing-baseline.json");

    let output = run_scan(
        &repo,
        &[
            "--baseline",
            missing.to_str().unwrap(),
            "--format",
            "markdown",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to read scan baseline"));
    assert!(stderr.contains("missing-baseline.json"));
}

#[test]
fn unrecognized_baseline_fails() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-unrecognized-baseline",
        &[("compose", "complete", "0.5", "", "")],
    );
    let baseline = repo.join("unknown.json");
    fs::write(&baseline, r#"{"not":"a scan"}"#).unwrap();

    let output = run_scan(
        &repo,
        &[
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json-summary",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unrecognized scan baseline format"));
    assert!(stderr.contains("unknown.json"));
}

#[test]
fn identical_baseline_zero_deltas() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-identical-baseline",
        &[("compose", "partial", "0.5", "parse_failed", "same failure")],
    );

    let initial = run_scan(&repo, &["--format", "json-summary"]);
    assert!(initial.status.success());
    let baseline = repo.join("identical-summary.json");
    fs::write(&baseline, &initial.stdout).unwrap();

    let output = run_scan(
        &repo,
        &[
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json-summary",
        ],
    );
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(value["deltas"]["adoption_coverage_delta"], 0.0);
    assert_eq!(value["deltas"]["resolved_delta"], 0);
    assert_eq!(value["deltas"]["candidate_delta"], 0);
    assert_eq!(
        value["deltas"]["new_error_diagnostics"],
        serde_json::json!([])
    );
    assert_eq!(
        value["deltas"]["resolved_diagnostics"],
        serde_json::json!([])
    );
}

#[test]
fn no_baseline_omits_changes_and_deltas_field() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-no-baseline",
        &[("compose", "complete", "0.5", "", "")],
    );

    let markdown = run_scan(&repo, &["--format", "markdown"]);
    assert!(markdown.status.success());
    assert!(
        !String::from_utf8(markdown.stdout)
            .unwrap()
            .contains("## Changes")
    );

    let json = run_scan(&repo, &["--format", "json-summary"]);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert!(value.get("deltas").is_none());
}

#[test]
fn baseline_with_quiet_only_skips_deltas() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-markdown-quiet-baseline",
        &[("compose", "complete", "0.5", "", "")],
    );
    let missing = repo.join("missing-baseline.json");

    let output = run_scan(
        &repo,
        &["--baseline", missing.to_str().unwrap(), "--format", "quiet"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        repo.join(".wax/out/scan-merged.json").display().to_string()
    );

    let summary = run_scan(
        &repo,
        &[
            "--baseline",
            missing.to_str().unwrap(),
            "--format",
            "summary",
        ],
    );
    assert!(summary.status.success());
}
