mod common;

use common::{
    EnvVarGuard, TestDir, assert_deferred_format, env_lock, no_registry_symbol_usage_override,
    run_scan, setup_scan_repo, write_grouped_repo_files, write_installed_packs,
    write_installed_packs_with_usage_sites, write_pack_index, write_repo_files,
};
use std::fs;
use std::path::Path;

fn graph_validator() -> jsonschema::Validator {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../wax-contract/schemas/scan-graph.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn assert_schema_valid_graph(value: &serde_json::Value) {
    let validator = graph_validator();
    assert!(
        validator.is_valid(value),
        "scan graph failed schema validation: {value}"
    );
}

fn read_graph(repo: &Path, relative: &str) -> serde_json::Value {
    let path = repo.join(relative);
    assert!(
        path.exists(),
        "missing graph artifact at {}",
        path.display()
    );
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn graph_data_validates_against_schema() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-graph-validates",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "graph-data=.wax/out/scan-graph.json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value = read_graph(&repo, ".wax/out/scan-graph.json");
    assert_schema_valid_graph(&value);
    assert!(
        value["metadata"]["limits"]
            .as_array()
            .is_some_and(|limits| !limits.is_empty()),
        "limits must be non-empty: {value}"
    );
    assert!(value["nodes"].as_array().is_some());
    assert!(value["edges"].as_array().is_some());
    assert!(value["metrics"].as_array().is_some());
}

#[test]
fn graph_includes_candidate_usage_edges() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-graph-candidate-edges",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "graph-data=.wax/out/scan-graph.json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value = read_graph(&repo, ".wax/out/scan-graph.json");
    let edges = value["edges"].as_array().unwrap();
    assert!(
        edges.iter().any(|edge| {
            edge["from"] == "file:src/b.tsx"
                && edge["to"] == "ds:compose:button"
                && edge["kind"] == "usage"
                && edge["match_status"] == "candidate"
        }),
        "expected candidate usage edge, got: {edges:?}"
    );
    assert!(
        edges.iter().any(|edge| {
            edge["from"] == "file:src/a.tsx"
                && edge["to"] == "ds:compose:button"
                && edge["kind"] == "usage"
                && edge["match_status"] == "resolved"
        }),
        "expected resolved usage edge, got: {edges:?}"
    );
}

#[test]
fn graph_skips_usage_without_registry_symbol() {
    let _guard = env_lock();
    let root = TestDir::new("scan-graph-skip-no-registry");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(&repo, &registry_file, &["compose"], None);
    write_installed_packs_with_usage_sites(
        &wax_home,
        &[("compose", "complete", "0.5", "", "")],
        Some(no_registry_symbol_usage_override()),
    );
    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(&repo, &["--output", "graph-data=.wax/out/scan-graph.json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value = read_graph(&repo, ".wax/out/scan-graph.json");
    let edges = value["edges"].as_array().unwrap();
    assert!(
        edges.is_empty(),
        "Local/Unresolved without registry_symbol must not emit usage edges: {edges:?}"
    );
}

#[test]
fn config_output_graph_idempotent() {
    let _guard = env_lock();
    let root = TestDir::new("scan-graph-config-idempotent");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();
    let registry_file = root.path.join("registry.json");
    write_pack_index(&registry_file);
    write_repo_files(
        &repo,
        &registry_file,
        &["compose"],
        Some(
            r#"[
    {"format":"graph-data","path":".wax/out/scan-graph.json"},
    {"format":"graph-data","path":".wax/out/scan-graph.json"}
  ]"#,
        ),
    );
    write_installed_packs(&wax_home, &[("compose", "complete", "0.5", "", "")]);
    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_scan(&repo, &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .matches("graph-data: .wax/out/scan-graph.json")
            .count(),
        1
    );
    assert_schema_valid_graph(&read_graph(&repo, ".wax/out/scan-graph.json"));
}

#[test]
fn html_still_deferred() {
    assert_deferred_format("html");
}

#[test]
fn graph_and_json_summary_both_succeed() {
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-graph-and-summary",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(
        &repo,
        &[
            "--output",
            "graph-data=.wax/out/scan-graph.json",
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
    assert!(stdout.contains("graph-data: .wax/out/scan-graph.json"));
    assert!(stdout.contains("json-summary: .wax/out/scan-summary.json"));
    assert_schema_valid_graph(&read_graph(&repo, ".wax/out/scan-graph.json"));
    assert!(repo.join(".wax/out/scan-summary.json").exists());
}

#[test]
fn root_group_graph_scoped_only() {
    let _guard = env_lock();
    let root = TestDir::new("scan-graph-root-group");
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
        &[
            "--output",
            "graph-data=.wax/out/scan-graph.json",
            "--root-group",
            "mobile",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value = read_graph(&repo, ".wax/out/scan-graph.json");
    assert_schema_valid_graph(&value);
    let language_nodes: Vec<&str> = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["kind"] == "language")
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(language_nodes, vec!["lang:compose"]);
}
