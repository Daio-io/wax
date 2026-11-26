use std::{
    fs,
    process::{Command, Output},
};
use wax_cli::{
    cli::{LookupStdoutFormat, RegistryFindArgs},
    commands::registry::run_registry_find_with_presentation,
    presentation::Presentation,
};

fn validated_find_json(stdout: &[u8]) -> serde_json::Value {
    assert_eq!(stdout.iter().filter(|&&byte| byte == b'\n').count(), 1);
    assert!(stdout.ends_with(b"\n"));
    let json: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../wax-contract/schemas/registry-catalog-find.schema.json"
    ))
    .unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json));
    json
}

fn configure_acme(root: &std::path::Path) {
    let path = root.join(".wax/wax.config.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        r#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme Local","registries":{"react":{"source":"react.json"}}}}}"#,
    )
    .unwrap();
}

fn run(name: &str, args: &[&str]) -> Output {
    let root = std::env::temp_dir().join(format!("wax-cli-catalog-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(args)
        .current_dir(&root)
        .env("WAX_HOME", root.join("home"))
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(root);
    result
}
#[test]
fn find_json_reports_cold_offline_partial_success_and_matches_schema() {
    let result = run(
        "find",
        &["registry", "find", "--offline", "--format", "json"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let json = validated_find_json(&result.stdout);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["results"], serde_json::json!([]));
    assert_eq!(json["public_status"]["complete"], false);
    assert_eq!(
        json["public_status"]["diagnostic"]["code"],
        "offline_cache_miss"
    );
}

#[test]
fn find_runner_uses_json_even_when_passed_human_presentation() {
    let root = common::TestDir::new("find-json-human");
    let args = RegistryFindArgs {
        query: String::new(),
        language: None,
        offline: true,
        format: LookupStdoutFormat::Json,
        repo_root: root.path.clone(),
    };
    let mut stdout = Vec::new();
    run_registry_find_with_presentation(
        args,
        root.path.clone(),
        Some(&wax_core::paths::WaxHome::new(root.path.join("home"))),
        Presentation::Human,
        &mut stdout,
    )
    .unwrap();
    assert_eq!(
        validated_find_json(&stdout)["results"],
        serde_json::json!([])
    );
}
#[test]
fn install_requires_positive_exact_version_and_never_prompts() {
    let missing = run("version", &["registry", "install", "acme"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--version"));
    let zero = run("zero", &["registry", "install", "acme", "--version", "0"]);
    assert_eq!(zero.status.code(), Some(2));
    let cold = run(
        "install-cold",
        &["registry", "install", "acme", "--version", "1", "--offline"],
    );
    assert!(!cold.status.success());
    assert!(cold.stdout.is_empty());
    assert!(String::from_utf8_lossy(&cold.stderr).contains("offline cache"));
}

mod common;
#[test]
fn piped_find_preserves_text_bytes_and_json_order_for_colliding_rows() {
    let root = common::TestDir::new("find-streams");
    common::catalog::seed_catalogue(&root.path);
    common::catalog::add_revision(&root.path, "acme", &["compose", "react"]);
    configure_acme(&root.path);
    let commit = common::catalog::git(&root.path.join("remote"), &["rev-parse", "HEAD"]);

    let text = common::catalog::run(&root.path, &["registry", "find", "acme"]);
    assert!(text.status.success());
    assert!(text.stderr.is_empty());
    assert_eq!(
        text.stdout,
        format!(
            concat!(
                "origin\tid\trevision\tname\tlanguages\tinstalled\n",
                "configured\tacme\t\tAcme Local\treact\ttrue\n",
                "public\tacme\t1\tAcme\tcompose,react\tfalse\n",
                "public\tacme\t2\tAcme\tcompose,react\tfalse\n",
                "public: complete=true cached=false commit={}\n",
            ),
            commit
        )
        .into_bytes()
    );

    let json = common::catalog::run(&root.path, &["registry", "find", "--format", "json"]);
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    let json = validated_find_json(&json.stdout);
    let results = json["results"].as_array().unwrap();
    let rows: Vec<_> = results
        .iter()
        .map(|row| {
            (
                row["origin"].as_str().unwrap(),
                row["id"].as_str().unwrap(),
                row["revision"].as_u64(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("configured", "acme", None),
            ("public", "acme", Some(1)),
            ("public", "acme", Some(2))
        ]
    );
    assert_eq!(json["public_status"]["commit"], commit);
    assert!(!root.path.join("home/state.json").exists());
}

#[test]
fn malformed_local_state_is_fatal_before_find_writes_stdout() {
    let root = common::TestDir::new("find-malformed-state");
    fs::create_dir_all(root.path.join("home")).unwrap();
    fs::write(root.path.join("home/state.json"), "{").unwrap();
    let output = common::catalog::run(&root.path, &["registry", "find", "--offline"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("state.json"));
}

#[test]
fn public_find_install_show_and_refresh_preserve_exact_bundle_pin() {
    let root = common::TestDir::new("public-install");
    common::catalog::seed_catalogue(&root.path);
    let find = common::catalog::run(
        &root.path,
        &[
            "registry",
            "find",
            "acm",
            "--language",
            "react",
            "--format",
            "json",
        ],
    );
    assert!(
        find.status.success(),
        "{}",
        String::from_utf8_lossy(&find.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&find.stdout).unwrap();
    assert_eq!(json["results"][0]["origin"], "public");
    let install = common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    );
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let state_path = root.path.join("home/state.json");
    let first =
        wax_core::registry_memory::show_remembered_design_system(&state_path, "public-acme-r1")
            .unwrap();
    let show = common::catalog::run(&root.path, &["registry", "show", "public-acme-r1"]);
    let show = String::from_utf8(show.stdout).unwrap();
    assert!(show.contains(&format!(
        "commit: {}",
        first.bundle_pin.as_ref().unwrap().commit
    )));
    assert!(show.contains("revision: 1"));
    assert!(show.contains("languages: compose,react"));
    common::catalog::change_registry(&root.path);
    let refresh = common::catalog::run(&root.path, &["registry", "refresh", "public-acme-r1"]);
    assert!(
        refresh.status.success(),
        "{}",
        String::from_utf8_lossy(&refresh.stderr)
    );
    let after =
        wax_core::registry_memory::show_remembered_design_system(&state_path, "public-acme-r1")
            .unwrap();
    assert_eq!(after.bundle_pin, first.bundle_pin);
    assert!(!root.path.join(".wax").exists());
}

#[test]
fn registry_list_displays_public_bundle_revision_commit_and_languages() {
    let root = common::TestDir::new("public-list");
    common::catalog::seed_catalogue(&root.path);
    assert!(
        common::catalog::run(
            &root.path,
            &["registry", "install", "acme", "--version", "1"]
        )
        .status
        .success()
    );
    let result = common::catalog::run(&root.path, &["registry", "list"]);
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with("commit\trevision\tlanguages")
    );
    assert!(text.contains("\t1\tcompose,react"));
}

#[test]
fn missing_git_is_partial_for_find_and_fatal_for_install() {
    let root = common::TestDir::new("no-git");
    configure_acme(&root.path);
    let output = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wax"))
            .args(args)
            .current_dir(&root.path)
            .env("WAX_HOME", root.path.join("home"))
            .env("PATH", root.path.join("no-executables"))
            .output()
            .unwrap()
    };
    let found = output(&["registry", "find", "--format", "json"]);
    assert!(found.status.success());
    assert!(found.stderr.is_empty());
    let found = validated_find_json(&found.stdout);
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    assert_eq!(found["results"][0]["origin"], "configured");
    assert_eq!(found["results"][0]["id"], "acme");
    assert_eq!(found["public_status"]["complete"], false);
    assert_eq!(
        found["public_status"]["diagnostic"]["code"],
        "public_unavailable"
    );
    assert!(
        found["public_status"]["diagnostic"]["message"]
            .as_str()
            .unwrap()
            .contains("git executable")
    );
    let install = output(&["registry", "install", "acme", "--version", "1"]);
    assert!(!install.status.success());
    assert!(install.stdout.is_empty());
    assert!(!root.path.join("home/state.json").exists());
}
