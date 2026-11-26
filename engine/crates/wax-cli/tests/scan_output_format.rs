use wax_core::paths::WaxHome;
mod common;

use common::{
    TestDir, assert_schema_valid_summary, run_scan, setup_scan_repo,
    write_committed_scan_repo_with_upstream, write_grouped_repo_files, write_installed_packs,
    write_pack_index,
};
use std::fs;
use std::process::Command;

#[test]
fn format_summary_default_matches_legacy_prefix() {
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
fn compact_summary_is_one_line_with_status_and_headline_counts() {
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-compact",
        &[
            ("react", "partial", "null", "", ""),
            ("compose", "complete", "0.5", "", ""),
        ],
    );

    let output = run_scan(
        &repo,
        &["--compact", "--output", "markdown=.wax/out/scan-summary.md"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let merged = repo.join(".wax/out/scan-merged.json");
    assert_eq!(
        stdout,
        format!(
            "scan={} languages=compose:complete,react:partial coverage=100.0% resolved=1 candidate=1 local=0 unresolved=0 token_refs=0 assessed=0/0 exact=0 near=0 unmatched=0 unassessed=0 failures=0 artifacts=1 ephemeral=false\n",
            merged.display()
        )
    );
}

#[test]
fn compact_summary_uses_na_when_coverage_is_unavailable() {
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-compact-no-coverage",
        &[("react", "partial", "null", "", "")],
    );

    let output = run_scan(&repo, &["--compact"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(" coverage=n/a "), "stdout: {stdout}");
}

#[test]
fn compact_sync_warning_goes_to_stderr() {
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-compact-sync-stderr",
        &[("react", "complete", "0.5", "", "")],
    );
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "registry": {
        "source": ".wax/react.registry.json",
        "upstream": "acme/react"
      }
    }
  }
}"#,
    )
    .unwrap();

    let output = run_scan(&repo, &["--compact"]);

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(!stdout.contains("warning: registry sync failed"));
    assert!(stderr.contains("warning: registry sync failed"));
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn compact_rejects_explicit_format() {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["scan", "--compact", "--format", "quiet"])
        .output()
        .expect("spawn wax scan");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with '--format"));
}

#[test]
fn format_quiet_path_only() {
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
fn format_json_summary_has_schema_version_1() {
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-format-json-version",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--format", "json-summary"]);
    assert!(output.status.success());
    let value: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    assert_eq!(value["schema_version"], 1);
}

#[test]
fn format_json_summary_uses_repo_relative_paths() {
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

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .env("WAX_HOME", &wax_home)
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

#[test]
fn human_summary_groups_existing_facts_without_losing_status_or_metrics() {
    use wax_cli::commands::scan::{ScanCommandOptions, run_scan as render_scan};
    use wax_cli::presentation::Presentation;
    let (_root, repo, wax_home) = setup_scan_repo(
        "human-summary",
        &[
            ("compose", "complete", "0.5", "", ""),
            ("react", "partial", "null", "", ""),
        ],
    );
    let mut bytes = Vec::new();
    render_scan(
        ScanCommandOptions {
            presentation: Presentation::Human,
            repo_root: repo.clone(),
            strict: false,
            allow_auto_install: false,
            scan_concurrency: Some(1),
            root_group: None,
            format: wax_cli::cli::ScanStdoutFormat::Summary,
            compact: false,
            cli_outputs: Vec::new(),
            allow_absolute_output: false,
            baseline: None,
            wax_home: Some(WaxHome::new(wax_home)),
            pack_index_url: None,
            target_triple: None,
            ephemeral: None,
        },
        &mut bytes,
        false,
    )
    .unwrap();
    let human = String::from_utf8(bytes).unwrap();
    assert!(human.starts_with("Scan results\n============\nscan output: "));
    let mut previous = 0;
    for section in [
        "\n\nLanguage status\n---------------\n",
        "\n\nAdoption metrics\n----------------\n",
        "\n\nToken metrics\n-------------\n",
        "\n\nFailure diagnostics\n-------------------\n",
    ] {
        let index = human
            .find(section)
            .unwrap_or_else(|| panic!("missing {section:?}: {human}"));
        assert!(index > previous);
        previous = index;
    }
    assert!(human.contains("compose: complete"));
    assert!(human.contains("react: partial"));
    // Every plain fact row remains present, in its original order.
    let plain = String::from_utf8(run_scan(&repo, &[]).stdout).unwrap();
    let mut remaining = human.as_str();
    for row in plain.lines().filter(|line| line.starts_with("  ")) {
        let index = remaining
            .find(row)
            .unwrap_or_else(|| panic!("lost {row:?}: {human}"));
        remaining = &remaining[index + row.len()..];
    }
    assert!(human.contains("  none\n"));
    assert!(!human.contains("Artifacts"));
}

fn human_options(
    repo: &std::path::Path,
    wax_home: &std::path::Path,
) -> wax_cli::commands::scan::ScanCommandOptions {
    wax_cli::commands::scan::ScanCommandOptions {
        presentation: wax_cli::presentation::Presentation::Human,
        repo_root: repo.to_path_buf(),
        strict: false,
        allow_auto_install: false,
        scan_concurrency: Some(1),
        root_group: None,
        format: wax_cli::cli::ScanStdoutFormat::Summary,
        compact: false,
        cli_outputs: Vec::new(),
        allow_absolute_output: false,
        baseline: None,
        wax_home: Some(WaxHome::new(wax_home)),
        pack_index_url: None,
        target_triple: None,
        ephemeral: None,
    }
}

#[test]
fn human_strict_summary_keeps_unavailable_metrics_bounded_diagnostics_and_artifacts() {
    use wax_cli::commands::scan::{ScanCommandError, ScanOutputRequest, run_scan as render_scan};
    let (root, repo, wax_home) = setup_scan_repo(
        "human-strict",
        &[
            ("compose", "partial", "null", "", ""),
            ("swift", "failed", "null", "", ""),
        ],
    );
    let long_path =
        "src/a/very/long/source/path/that/must/remain/fully/readable/at/forty/columns.kt";
    let script = root.path.join("wax-home/langs/swift/0.1.0/pack.sh");
    let text = fs::read_to_string(&script).unwrap();
    let mut wire: serde_json::Value =
        serde_json::from_str(text.lines().find(|line| line.starts_with('{')).unwrap()).unwrap();
    wire["facts"]["diagnostics"] = serde_json::json!((1..=7).map(|line| serde_json::json!({
        "severity": "error", "code": "PARSE_ERROR", "message": format!("broken syntax {line}"),
        "location": {"file": long_path, "line": line}
    })).collect::<Vec<_>>());
    fs::write(
        script,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{wire}\nJSON\n"),
    )
    .unwrap();
    let mut opts = human_options(&repo, &wax_home);
    opts.strict = true;
    opts.cli_outputs.push(ScanOutputRequest {
        format: "markdown".into(),
        path: ".wax/out/a-long-artifact-name-for-human-output.md".into(),
    });
    let mut bytes = Vec::new();
    let result = render_scan(opts, &mut bytes, true);
    assert!(matches!(
        result,
        Err(ScanCommandError::IncompleteScan { .. })
    ));
    let human = String::from_utf8(bytes).unwrap();
    for fact in [
        "compose: partial",
        "swift: failed",
        "UI invocation adoption: n/a",
        "Registry resolution: n/a",
        "Raw DS invocations: 0 resolved, 0 candidate",
        "Assessed observations: 0 of 0",
        "7 total; showing 5",
        "broken syntax 5",
        "2 more diagnostics omitted",
        long_path,
        "\n\nArtifacts\n---------\n",
        ".wax/out/a-long-artifact-name-for-human-output.md",
        "To save this setup for CI or teammates, run `wax init`.",
    ] {
        assert!(human.contains(fact), "missing {fact:?}: {human}");
    }
    assert!(!human.contains("broken syntax 6"));
    assert!(human.find("Failure diagnostics").unwrap() < human.find("Artifacts").unwrap());
    assert!(repo.join(".wax/out/scan-merged.json").exists());
}

#[test]
fn human_root_groups_keep_full_labels_and_section_order() {
    use wax_cli::commands::scan::run_scan as render_scan;
    let (root, repo, wax_home) = setup_scan_repo(
        "human-groups",
        &[
            ("compose", "complete", "0.5", "", ""),
            ("react", "complete", "0.5", "", ""),
        ],
    );
    write_grouped_repo_files(&repo, &root.path.join("registry.json"));
    let config = repo.join(".wax/wax.config.json");
    let label = "mobile-with-a-label-that-is-longer-than-forty-terminal-columns";
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("\"mobile\"", &format!("\"{label}\"")),
    )
    .unwrap();
    for (language, prefix) in [("compose", "mobile"), ("react", "web")] {
        let script = root
            .path
            .join(format!("wax-home/langs/{language}/0.1.0/pack.sh"));
        fs::write(
            &script,
            fs::read_to_string(&script)
                .unwrap()
                .replace("src/", &format!("{prefix}/src/")),
        )
        .unwrap();
    }
    let mut bytes = Vec::new();
    render_scan(human_options(&repo, &wax_home), &mut bytes, false).unwrap();
    let human = String::from_utf8(bytes).unwrap();
    assert!(human.contains("\n\nRoot groups\n-----------\n"));
    assert!(
        human.contains(&format!("{label}/compose: UI invocation adoption:")),
        "{human}"
    );
    assert!(human.find("Adoption metrics").unwrap() < human.find("Root groups").unwrap());
    assert!(human.find("Root groups").unwrap() < human.find("Token metrics").unwrap());
}

#[test]
fn human_token_inference_retains_confidence_evidence_and_comparison_caveats() {
    use wax_cli::commands::scan::run_scan as render_scan;
    let (root, repo, wax_home) =
        setup_scan_repo("human-tokens", &[("compose", "complete", "0.5", "", "")]);
    let registry = serde_json::json!({
        "schema_version": 1, "components": [{"id":"ds.button", "symbol":"Button"}],
        "tokens": [
            {"id":"spacing.small", "key":"Space.small", "category":"spacing", "value":"4.dp"},
            {"id":"color.primary", "key":"Color.primary", "category":"color"}
        ]
    });
    let registry_path = repo.join(".wax/compose.registry.json");
    fs::write(&registry_path, registry.to_string()).unwrap();
    let lock_path = repo.join(".wax/wax.lock.json");
    let mut lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    lock["registries"]["compose"]["sha256"] = wax_core::registry_source::resolve_registry_source(
        wax_core::registry_source::RegistrySourceInput {
            repo_root: &repo,
            language_id: "compose",
            source: Some(".wax/compose.registry.json"),
        },
    )
    .unwrap()
    .sha256
    .into();
    fs::write(lock_path, lock.to_string()).unwrap();

    let script = root.path.join("wax-home/langs/compose/0.1.0/pack.sh");
    let text = fs::read_to_string(&script).unwrap();
    let mut wire: serde_json::Value =
        serde_json::from_str(text.lines().find(|line| line.starts_with('{')).unwrap()).unwrap();
    let mut facts: wax_contract::ScanFacts = serde_json::from_value(wire["facts"].clone()).unwrap();
    facts.design_system_tokens = serde_json::from_value(registry["tokens"].clone()).unwrap();
    facts.hardcoded_style_sites = serde_json::from_value(serde_json::json!([
        {"id":"exact", "location":{"file":"src/Card.kt", "line":12}, "value":"4.dp", "category":"spacing", "context":"padding"},
        {"id":"unassessed", "location":{"file":"src/Card.kt", "line":13}, "value":"#123456", "category":"color", "context":"color"}
    ])).unwrap();
    facts.recompute_counts().unwrap();
    wire["facts"] = serde_json::to_value(facts).unwrap();
    fs::write(
        script,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{wire}\nJSON\n"),
    )
    .unwrap();
    let mut bytes = Vec::new();
    render_scan(human_options(&repo, &wax_home), &mut bytes, false).unwrap();
    let human = String::from_utf8(bytes).unwrap();
    for fact in [
        "Assessed observations: 1 of 2",
        "Confirmed migration candidates: 1",
        "Unassessed observations: 1 (comparison unavailable)",
        "src/Card.kt:12 padding 4.dp -> Space.small=4.dp",
        "exact, very high",
        "evidence: exact value",
        "Run wax-registry-discover to review missing canonical token values.",
    ] {
        assert!(human.contains(fact), "missing {fact:?}: {human}");
    }
    let plain = String::from_utf8(run_scan(&repo, &[]).stdout).unwrap();
    for row in plain.lines().filter(|line| line.starts_with("  ")) {
        assert!(human.contains(row), "lost {row:?}");
    }
}
