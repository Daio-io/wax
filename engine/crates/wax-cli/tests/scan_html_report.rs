mod common;

use common::{env_lock, run_scan, setup_scan_repo};
use std::fs;

#[test]
fn html_contains_adoption_headline_and_diagnostics() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-headline",
        &[(
            "compose",
            "complete",
            "0.5",
            "parse_failed",
            "fixture failed",
        )],
    );

    let output = run_scan(&repo, &["--output", "html=.wax/out/report/index.html"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = fs::read_to_string(repo.join(".wax/out/report/index.html")).unwrap();
    assert!(html.contains("Adoption"));
    assert!(html.contains("compose"));
    assert!(html.contains("Diagnostics"));
    assert!(html.contains("fixture failed"));
    assert!(html.contains("Limits"));
}

#[test]
fn html_is_offline_no_external_script_src() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-offline",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "html=.wax/out/report/index.html"]);
    assert!(output.status.success());
    let html = fs::read_to_string(repo.join(".wax/out/report/index.html")).unwrap();

    assert!(!html.contains("src=\"http"));
    assert!(!html.contains("<script src="));
}

#[test]
fn html_placeholder_when_metrics_empty() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-empty",
        &[("compose", "complete", "null", "", "")],
    );

    let output = run_scan(&repo, &["--output", "html=.wax/out/report/index.html"]);
    assert!(output.status.success());
    let html = fs::read_to_string(repo.join(".wax/out/report/index.html")).unwrap();

    assert!(html.contains("No graph metrics"));
}

#[test]
fn html_parent_dir_created() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-parent",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "html=.wax/out/deep/report/index.html"]);
    assert!(output.status.success());
    assert!(repo.join(".wax/out/deep/report/index.html").is_file());
}

#[test]
fn config_output_html_idempotent() {
    let _guard = env_lock();
    let root = common::TestDir::new("scan-html-config");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    common::write_pack_index(&registry_file);
    common::write_repo_files(
        &repo,
        &registry_file,
        &["compose"],
        Some(
            r#"[{"format":"html","path":".wax/out/report/index.html"},{"format":"html","path":".wax/out/report/index.html"}]"#,
        ),
    );
    common::write_installed_packs(&wax_home, &[("compose", "complete", "0.5", "", "")]);
    let _wax_home = common::EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(&repo, &[]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .matches("html: .wax/out/report/index.html")
            .count(),
        1
    );
}

#[test]
fn write_order_graph_before_html_when_both_written() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) =
        setup_scan_repo("scan-html-order", &[("compose", "complete", "0.5", "", "")]);

    let output = run_scan(
        &repo,
        &[
            "--output",
            "html=.wax/out/report/index.html",
            "--output",
            "graph-data=.wax/out/scan-graph.json",
        ],
    );
    assert!(output.status.success());
    let html = fs::read_to_string(repo.join(".wax/out/report/index.html")).unwrap();
    assert!(html.contains("graph-data ("));
    assert!(!html.contains("graph-data (size unknown)"));
}

#[test]
fn html_links_json_summary_when_both_written() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) =
        setup_scan_repo("scan-html-links", &[("compose", "complete", "0.5", "", "")]);

    let output = run_scan(
        &repo,
        &[
            "--output",
            "html=.wax/out/report/index.html",
            "--output",
            "json-summary=.wax/out/scan-summary.json",
        ],
    );
    assert!(output.status.success());
    let html = fs::read_to_string(repo.join(".wax/out/report/index.html")).unwrap();
    assert!(html.contains("href=\"../scan-summary.json\""));
}

#[test]
fn html_links_merged_scan_when_html_is_only_output() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-merged-link",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "html=report/index.html"]);
    assert!(output.status.success());
    let html = fs::read_to_string(repo.join("report/index.html")).unwrap();
    assert!(html.contains("href=\"../.wax/out/scan-merged.json\""));
}

#[test]
fn markdown_still_deferred() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-html-markdown-deferred",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "markdown=.wax/out/report.md"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("markdown"));
}
