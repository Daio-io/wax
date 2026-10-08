mod common;

use common::{
    EnvVarGuard, TestDir, env_lock, run_scan, setup_scan_repo, write_grouped_repo_files,
    write_installed_packs, write_pack_index, write_repo_files,
};
use std::fs;
use std::path::Path;
use wax_contract::{LanguageId, SCHEMA_VERSION};

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
    write_pack_without_registry_symbol_usages(&wax_home, "compose");
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
    let _guard = env_lock();
    let (_root, repo, _wax_home) = setup_scan_repo(
        "scan-graph-html-deferred",
        &[("compose", "complete", "0.5", "", "")],
    );

    let output = run_scan(&repo, &["--output", "html=.wax/out/report/index.html"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("output format `html` is not implemented yet"),
        "unexpected stderr: {stderr}"
    );
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

fn write_pack_without_registry_symbol_usages(wax_home: &Path, language: &str) {
    let install_dir = wax_home.join(format!("langs/{language}/0.1.0"));
    fs::create_dir_all(&install_dir).unwrap();

    let facts = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "language": {
            "id": LanguageId::try_from(language).unwrap(),
            "version": "0.1.0",
            "ecosystem": "test",
            "parser_name": "fixture",
            "parser_version": "1.0.0"
        },
        "snapshot_id": format!("snap-{language}"),
        "scanned_at": "1970-01-01T00:00:00Z",
        "status": "complete",
        "design_system_components": [],
        "local_components": [{
            "id": "local-1",
            "symbol": "LocalButton",
            "location": { "file": "src/local.kt", "line": 1 }
        }],
        "usage_sites": [
            {
                "id": "site-local",
                "location": { "file": "src/local.kt", "line": 2 },
                "symbol": "LocalButton",
                "match_status": "local",
                "callee_origin": "local",
                "resolution_evidence": { "kind": "local_same_file" },
                "local_definition_id": "local-1"
            },
            {
                "id": "site-unresolved",
                "location": { "file": "src/unknown.kt", "line": 1 },
                "symbol": "Mystery",
                "match_status": "unresolved",
                "callee_origin": "unknown",
                "resolution_evidence": { "kind": "no_matching_definition" }
            }
        ],
        "diagnostics": [],
        "metrics": {
            "invocation_adoption_ratio": 0.0,
            "registry_resolution_ratio": 0.0,
            "parse_extract_ms": 5,
            "files_scanned": 1
        },
        "counts": {
            "registry": {
                "component_count": 0,
                "used_component_count": 0,
                "resolved_raw_invocation_count": 0,
                "candidate_raw_invocation_count": 0
            },
            "definitions": {
                "local_definition_count": 1,
                "invoked_local_definition_count": 1,
                "unused_local_definition_count": 0
            },
            "raw_invocations": {
                "total": 2,
                "resolved": 0,
                "local": 1,
                "candidate": 0,
                "unresolved": 1
            },
            "adoption": {
                "eligible_invocation_count": 2,
                "adopted_invocation_count": 0,
                "non_adopted_invocation_count": 2,
                "adoption_excluded_invocation_count": 0
            },
            "parent_scopes": {
                "total": 0,
                "with_resolved_invocations": 0,
                "with_local_invocations": 0,
                "with_unresolved_invocations": 0
            },
            "invocation_origins": {
                "registry": 0,
                "local": 1,
                "framework": 0,
                "external": 0,
                "application": 0,
                "unknown": 1
            }
        }
    });
    let wire = serde_json::json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": language,
        "facts": facts
    });
    let script = install_dir.join("pack.sh");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
set -eu
cat >/dev/null
cat <<JSON
{}
JSON
"#,
            wire
        ),
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }

    fs::write(
        install_dir.join("manifest.json"),
        format!(
            r#"{{
  "id": "{language}",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#
        ),
    )
    .unwrap();

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "{language}": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}
  }}
}}"#,
            install_dir.display()
        ),
    )
    .unwrap();
}
