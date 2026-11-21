mod common;

use common::catalog::{add_revision, git};
use serde_json::Value;
use std::{fs, path::Path, process::Output};

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn init(root: &Path, languages: &[&str]) -> Output {
    let index =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/registry/alpha-index.json");
    let url = format!("file://{}", index.display());
    let mut args = vec![
        "init",
        "--non-interactive",
        "--design-system",
        "public-acme-r1",
        "--no-install",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--pack-index",
        &url,
    ];
    for language in languages {
        args.extend(["--language", language]);
    }
    common::catalog::run(root, &args)
}

#[test]
fn scriptable_init_pins_all_selected_bundle_languages_and_validates_without_global_state() {
    let root = common::TestDir::new("public-init");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    assert!(!root.path.join(".wax").exists());
    success(init(&root.path, &["compose", "react"]));
    let config = fs::read_to_string(root.path.join(".wax/wax.config.json")).unwrap();
    let lock = fs::read_to_string(root.path.join(".wax/wax.lock.json")).unwrap();
    assert!(!config.contains(root.path.to_str().unwrap()));
    assert!(!lock.contains(root.path.join("home").to_str().unwrap()));
    let lock: Value = serde_json::from_str(&lock).unwrap();
    assert_eq!(
        lock["registries"]["react"]["definition"],
        lock["registries"]["compose"]["definition"]
    );
    assert_eq!(
        lock["registries"]["react"]["commit"],
        lock["registries"]["compose"]["commit"]
    );
    fs::remove_dir_all(root.path.join("home")).unwrap();
    success(common::catalog::run(&root.path, &["validate"]));
}

#[test]
fn initialized_app_search_works_offline_after_global_state_and_upstream_are_removed() {
    let root = common::TestDir::new("public-init-offline");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    fs::remove_dir_all(root.path.join("home")).unwrap();
    fs::remove_dir_all(root.path.join("remote")).unwrap();
    let output = success(common::catalog::run(
        &root.path,
        &["search", "Button", "--offline", "--format", "json"],
    ));
    let result: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(result["results"].as_array().unwrap().len(), 2);
}

fn app_files(root: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(root.join(".wax/wax.config.json")).unwrap(),
        fs::read(root.join(".wax/wax.lock.json")).unwrap(),
    )
}

#[test]
fn explicit_sync_adopts_whole_revision_while_ordinary_operations_keep_existing_pins() {
    let root = common::TestDir::new("public-sync");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let original = app_files(&root.path);
    add_revision(&root.path, "acme", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    for args in [
        vec!["sync"],
        vec!["sync", "--upgrade"],
        vec!["registry", "refresh", "public-acme-r1"],
    ] {
        success(common::catalog::run(&root.path, &args));
        assert_eq!(app_files(&root.path), original);
    }
    let output = success(common::catalog::run(
        &root.path,
        &["sync", "--design-system", "public-acme-r2"],
    ));
    assert!(
        output.contains("systems/acme/1 -> systems/acme/2"),
        "{output}"
    );
    let updated = app_files(&root.path);
    assert_ne!(updated, original);
    let lock: Value = serde_json::from_slice(&updated.1).unwrap();
    assert_eq!(lock["registries"]["react"]["subdir"], "systems/acme/2");
    assert_eq!(
        lock["registries"]["react"]["definition"],
        lock["registries"]["compose"]["definition"]
    );
    success(common::catalog::run(&root.path, &["validate"]));
    fs::remove_dir_all(root.path.join("home")).unwrap();
    fs::remove_dir_all(root.path.join("remote")).unwrap();
    let results = success(common::catalog::run(
        &root.path,
        &["search", "NewButton", "--offline", "--format", "json"],
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&results).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn explicit_sync_rejects_incomplete_bundle_without_changing_either_app_file() {
    let root = common::TestDir::new("public-sync-incomplete");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let original = app_files(&root.path);
    add_revision(&root.path, "acme", &["react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("compose"));
    assert_eq!(app_files(&root.path), original);
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_sync_preserves_exact_app_bytes_when_config_or_lockfile_replacement_fails() {
    let root = common::TestDir::new("public-sync-write-failure");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let config_path = root.path.join(".wax/wax.config.json");
    let config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    fs::write(&config_path, format!("{}\r\n", config)).unwrap();
    let original = app_files(&root.path);
    add_revision(&root.path, "acme", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    for filename in ["wax.config.json", "wax.lock.json"] {
        let contract = root.path.join(".wax").join(filename);
        assert!(
            std::process::Command::new("chflags")
                .arg("uchg")
                .arg(&contract)
                .status()
                .unwrap()
                .success()
        );
        let output =
            common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
        assert!(
            std::process::Command::new("chflags")
                .arg("nouchg")
                .arg(&contract)
                .status()
                .unwrap()
                .success()
        );
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(filename));
        assert_eq!(app_files(&root.path), original);
    }
}

#[test]
fn init_rejects_unsupported_bundle_language_before_writing_app_files() {
    let root = common::TestDir::new("public-init-language");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    let output = init(&root.path, &["swift"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("swift"));
    assert!(!root.path.join(".wax").exists());
}

#[test]
fn explicit_sync_rejects_wrong_system_and_unrelated_source_without_app_writes() {
    let root = common::TestDir::new("public-sync-wrong-system");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let original = app_files(&root.path);
    add_revision(&root.path, "other", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "other", "--version", "2"],
    ));
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-other-r2"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no bindings"));
    assert_eq!(app_files(&root.path), original);

    add_revision(&root.path, "acme", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    // Simulate a same-name bundle from a different publisher, through the state-file contract.
    let state_path = root.path.join("home/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["design_systems"]["public-acme-r2"]["bundle_pin"]["repo"] =
        "https://example.invalid/unrelated.git".into();
    fs::write(state_path, state.to_string()).unwrap();
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
    assert!(!output.status.success());
    assert_eq!(app_files(&root.path), original);
}

#[test]
fn explicit_sync_preserves_app_on_corrupt_cached_definition_and_unavailable_exact_commit() {
    let root = common::TestDir::new("public-sync-corrupt");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let original = app_files(&root.path);
    add_revision(&root.path, "acme", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    let remembered = wax_core::registry_memory::show_remembered_design_system(
        root.path.join("home/state.json"),
        "public-acme-r2",
    )
    .unwrap();
    let bundle = remembered.bundle_pin.unwrap();
    let blobs = root.path.join(".wax/cache/registries/blobs");
    fs::write(
        blobs.join(format!("{}.json", bundle.definition.sha256)),
        "corrupt",
    )
    .unwrap();
    // A definition is checked alongside a registry; seed a cached registry so it cannot be refetched around the corrupt definition.
    for snapshot in &remembered.snapshots {
        fs::copy(
            root.path.join(format!(
                "home/cache/registries/blobs/{}.json",
                snapshot.digest
            )),
            blobs.join(format!("{}.json", snapshot.digest)),
        )
        .unwrap();
    }
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest"));
    assert_eq!(app_files(&root.path), original);
    fs::copy(
        root.path.join(format!(
            "home/cache/registries/blobs/{}.json",
            bundle.definition.sha256
        )),
        blobs.join(format!("{}.json", bundle.definition.sha256)),
    )
    .unwrap();
    let react = remembered
        .snapshots
        .iter()
        .find(|snapshot| snapshot.language.as_str() == "react")
        .unwrap();
    fs::write(
        blobs.join(format!("{}.json", react.digest)),
        "corrupt registry",
    )
    .unwrap();
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest"));
    assert_eq!(app_files(&root.path), original);
    fs::remove_dir_all(root.path.join(".wax/cache")).unwrap();
    fs::remove_dir_all(root.path.join("remote")).unwrap();
    let output = common::catalog::run(&root.path, &["sync", "--design-system", "public-acme-r2"]);
    assert!(!output.status.success());
    assert_eq!(app_files(&root.path), original);
}

fn install_test_packs(root: &Path) -> String {
    let state_path = root.join("home/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    common::write_installed_packs(
        &root.join("home"),
        &[
            ("react", "complete", "null", "", ""),
            ("compose", "complete", "null", "", ""),
        ],
    );
    let installed: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["installed_languages"] = installed["installed_languages"].clone();
    fs::write(state_path, state.to_string()).unwrap();
    let index = root.join("packs.json");
    common::write_pack_index(&index);
    let target = host_target();
    let contents = fs::read_to_string(&index)
        .unwrap()
        .replace("x86_64-unknown-linux-gnu", &target);
    fs::write(&index, contents).unwrap();
    for language in ["compose", "react"] {
        let path = root.join(format!("home/langs/{language}/0.1.0/manifest.json"));
        let contents = fs::read_to_string(&path)
            .unwrap()
            .replace("x86_64-unknown-linux-gnu", &target);
        fs::write(path, contents).unwrap();
    }
    format!("file://{}", index.display())
}

#[test]
fn configless_scans_use_warmed_exact_public_sources_and_require_preinstalled_packs() {
    let root = common::TestDir::new("public-configless");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    let index = install_test_packs(&root.path);
    fs::create_dir(root.path.join("src")).unwrap();
    fs::remove_dir_all(root.path.join("remote")).unwrap();
    for selector in ["public-acme-r1", "public:acme@1"] {
        success(
            common::catalog::command(&root.path)
                .env("WAX_PACK_INDEX", &index)
                .args([
                    "scan",
                    "--design-system",
                    selector,
                    "--language",
                    "react",
                    "--root",
                    "src",
                    "--no-auto-install",
                ])
                .output()
                .unwrap(),
        );
        assert!(!root.path.join(".wax/wax.config.json").exists());
        assert!(!root.path.join(".wax/wax.lock.json").exists());
    }
    fs::remove_dir_all(root.path.join("home/langs")).unwrap();
    let missing = common::catalog::command(&root.path)
        .env("WAX_PACK_INDEX", &index)
        .args([
            "scan",
            "--design-system",
            "public-acme-r1",
            "--language",
            "react",
            "--root",
            "src",
            "--no-auto-install",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("react"));
    assert!(!root.path.join(".wax/wax.config.json").exists());
    assert!(!root.path.join(".wax/wax.lock.json").exists());
}

fn host_target() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-gnu",
        "windows" => "pc-windows-msvc",
        other => panic!("unsupported test platform: {other}"),
    };
    format!("{}-{platform}", std::env::consts::ARCH)
}

#[test]
fn initialized_app_scans_keep_lock_after_global_revision_changes_and_work_without_registry_memory()
{
    let root = common::TestDir::new("public-app-scan");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    let index = install_test_packs(&root.path);
    success(common::catalog::run(
        &root.path,
        &[
            "init",
            "--non-interactive",
            "--design-system",
            "public-acme-r1",
            "--language",
            "react",
            "--language",
            "compose",
            "--no-install",
            "--pack-index",
            &index,
        ],
    ));
    let original = app_files(&root.path);
    let search: Value = serde_json::from_str(&success(common::catalog::run(
        &root.path,
        &[
            "search",
            "Button",
            "--language",
            "react",
            "--format",
            "json",
        ],
    )))
    .unwrap();
    let digest = search["results"][0]["snapshot"]["digest"].as_str().unwrap();
    add_revision(&root.path, "acme", &["compose", "react"]);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "2"],
    ));
    success(common::catalog::run(
        &root.path,
        &["registry", "find", "--format", "json"],
    ));
    let scan = || {
        common::catalog::command(&root.path)
            .env("WAX_PACK_INDEX", &index)
            .args(["scan", "--no-auto-install"])
            .output()
            .unwrap()
    };
    success(scan());
    assert_eq!(app_files(&root.path), original);
    success(common::catalog::run(
        &root.path,
        &["sync", "--design-system", "public-acme-r2"],
    ));
    let detail: Value = serde_json::from_str(&success(common::catalog::run(
        &root.path,
        &[
            "component",
            "button",
            "--design-system",
            "public-acme-r1",
            "--language",
            "react",
            "--snapshot",
            digest,
            "--offline",
            "--format",
            "json",
        ],
    )))
    .unwrap();
    assert_eq!(detail["entry"]["symbol"], "Button");
    let upgraded = app_files(&root.path);
    let state_path = root.path.join("home/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["design_systems"] = serde_json::json!({});
    fs::write(state_path, state.to_string()).unwrap();
    fs::remove_dir_all(root.path.join("home/cache")).unwrap();
    fs::remove_dir_all(root.path.join("remote")).unwrap();
    success(scan());
    success(common::catalog::run(&root.path, &["validate"]));
    assert_eq!(app_files(&root.path), upgraded);
}

#[test]
fn clean_checkout_sync_reproduces_exact_pins_without_memory_or_catalogue_main() {
    let root = common::TestDir::new("public-clean-checkout");
    common::catalog::seed_catalogue(&root.path);
    success(common::catalog::run(
        &root.path,
        &["registry", "install", "acme", "--version", "1"],
    ));
    success(init(&root.path, &["compose", "react"]));
    let original = app_files(&root.path);
    common::catalog::change_registry(&root.path);
    fs::remove_dir_all(root.path.join("home")).unwrap();
    fs::remove_dir_all(root.path.join(".wax/cache")).unwrap();
    // main no longer contains a catalogue; only the committed bundle pins can work.
    fs::remove_file(root.path.join("remote/catalog/index.json")).unwrap();
    git(&root.path.join("remote"), &["add", "."]);
    git(
        &root.path.join("remote"),
        &["commit", "-qm", "remove current catalogue"],
    );
    success(common::catalog::run(&root.path, &["validate"]));
    success(common::catalog::run(&root.path, &["sync"]));
    assert_eq!(app_files(&root.path), original);
    let results: Value = serde_json::from_str(&success(common::catalog::run(
        &root.path,
        &["search", "Button", "--offline", "--format", "json"],
    )))
    .unwrap();
    assert!(
        results["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["label"] == "Button")
    );
    assert_eq!(results["results"].as_array().unwrap().len(), 2);
}

#[test]
fn interactive_selections_use_the_same_pinned_bundle_handoff() {
    use wax_cli::commands::init::{InitOptions, InitSelections, RegistrySetup, run_init};
    use wax_core::registry_cache::RegistrySourceSpec;
    let root = common::TestDir::new("public-interactive");
    common::catalog::seed_catalogue(&root.path);
    let remote = root.path.join("remote");
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&remote)
        .output()
        .unwrap();
    assert!(commit.status.success());
    let commit = String::from_utf8(commit.stdout).unwrap().trim().to_owned();
    let state_path = root.path.join("home/state.json");
    wax_core::registry_memory::add_remembered_registry(
        &state_path,
        "bundle",
        "Acme",
        &RegistrySourceSpec::GitSystem {
            url: remote.display().to_string(),
            tag: commit.clone(),
            subdir: "systems/acme/1".into(),
            system_id: "acme".into(),
        },
    )
    .unwrap();
    let index =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/registry/alpha-index.json");
    let languages: Vec<wax_contract::LanguageId> = ["compose", "react"]
        .into_iter()
        .map(|id| id.try_into().unwrap())
        .collect();
    run_init(
        InitOptions {
            non_interactive: false,
            design_system: None,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(format!("file://{}", index.display())),
            repo_root: root.path.clone(),
            target_triple: Some("x86_64-unknown-linux-gnu".into()),
            state_path: Some(state_path),
            scaffold_registries: false,
            interactive: Some(InitSelections {
                scan_roots: languages
                    .iter()
                    .map(|id| (id.clone(), vec!["src".into()]))
                    .collect(),
                languages,
                registry_setup: RegistrySetup::RememberedDesignSystem {
                    design_system_id: "bundle".into(),
                },
            }),
        },
        &mut Vec::new(),
    )
    .unwrap();
    let (config, lock) = app_files(&root.path);
    let config: Value = serde_json::from_slice(&config).unwrap();
    let lock: Value = serde_json::from_slice(&lock).unwrap();
    for language in ["compose", "react"] {
        assert_eq!(config["languages"][language]["registry"]["tag"], commit);
        assert_eq!(
            config["languages"][language]["roots"],
            serde_json::json!(["src"])
        );
        assert_eq!(lock["registries"][language]["commit"], commit);
    }
    success(common::catalog::run(&root.path, &["validate"]));
}
