use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use wax_core::registry_git::fetch_git_files;
struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wax-bundle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Wax Test",
            "-c",
            "user.email=test@example.com",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
#[test]
fn git_files_reproduce_one_commit_offline_after_a_tag_moves() {
    let root = Temp::new("files");
    let repo = root.0.join("repo");
    fs::create_dir_all(repo.join("systems/one")).unwrap();
    git(&repo, &["init", "-q"]);
    fs::write(repo.join("systems/one/definition.json"), b"definition v1").unwrap();
    fs::write(repo.join("systems/one/react.json"), b"react v1").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "first"]);
    git(&repo, &["tag", "v1"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    let cache = root.0.join("cache");
    let paths = ["systems/one/definition.json", "systems/one/react.json"];
    let first = fetch_git_files(repo.to_str().unwrap(), "v1", &paths, &cache, false).unwrap();
    fs::write(repo.join(paths[1]), b"react v2").unwrap();
    git(&repo, &["commit", "-am", "second", "-q"]);
    git(&repo, &["tag", "-f", "v1"]);
    fs::rename(&repo, root.0.join("unavailable")).unwrap();
    let warm = fetch_git_files(repo.to_str().unwrap(), &commit, &paths, &cache, true).unwrap();
    assert_eq!(first, warm);
    assert_eq!(warm.files[paths[1]], b"react v1");
    assert!(
        fetch_git_files(
            repo.to_str().unwrap(),
            &commit,
            &paths,
            &root.0.join("cold"),
            true
        )
        .is_err()
    );
}

#[test]
fn git_files_reject_pathspecs_and_non_regular_files() {
    let root = Temp::new("paths");
    let repo = root.0.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    fs::write(repo.join("regular"), b"safe").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("regular", repo.join("link")).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "files"]);
    let cache = root.0.join("cache");
    for path in [
        "",
        "/regular",
        "a//b",
        "./regular",
        "a/../regular",
        "a\\b",
        "a/-b",
        "a\0b",
        ":(glob)*",
        "link",
        "missing",
    ] {
        assert!(
            fetch_git_files(repo.to_str().unwrap(), "HEAD", &[path], &cache, false).is_err(),
            "accepted {path:?}"
        );
    }
}

use wax_core::registry_cache::{RegistryCache, RegistryReadPolicy, RegistrySourceSpec};
fn bundle_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    for (id, symbol) in [("one", "Button"), ("two", "OtherButton")] {
        let bundle = repo.join(format!("systems/{id}/1"));
        fs::create_dir_all(bundle.join(".wax/registries")).unwrap();
        fs::write(bundle.join(".wax/wax.config.json"), format!(r#"{{"schema_version":2,"design_systems":{{"{id}":{{"name":"{id}","registries":{{"compose":{{"source":".wax/registries/compose.json"}},"react":{{"source":".wax/registries/react.json","published_source":"https://invalid.example/ignore.json"}}}}}}}}}}"#)).unwrap();
        for language in ["compose", "react"] {
            fs::write(
                bundle.join(format!(".wax/registries/{language}.json")),
                format!(
                    r#"{{"schema_version":1,"components":[{{"id":"button","symbol":"{symbol}"}}]}}"#
                ),
            )
            .unwrap();
        }
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "bundles"]);
    git(&repo, &["tag", "v1"]);
    repo
}
fn bundle_source(repo: &Path, id: &str) -> RegistrySourceSpec {
    RegistrySourceSpec::GitSystem {
        url: repo.display().to_string(),
        tag: "v1".into(),
        subdir: format!("systems/{id}/1"),
        system_id: id.into(),
    }
}
#[test]
fn cache_selects_two_systems_independently_and_pins_every_language() {
    let root = Temp::new("systems");
    let repo = bundle_repo(&root.0);
    let cache = RegistryCache::open(root.0.join("cache"));
    let source = bundle_source(&repo, "one");
    let one = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .unwrap();
    let two = cache
        .resolve(&bundle_source(&repo, "two"), &RegistryReadPolicy::default())
        .unwrap();
    assert_eq!(one.len(), 2);
    assert_eq!(one[0].reference.git_commit, one[1].reference.git_commit);
    assert_eq!(
        one[0].reference.extra["definition"],
        one[1].reference.extra["definition"]
    );
    assert_eq!(
        one[0].reference.extra["definition"]["path"],
        "systems/one/1/.wax/wax.config.json"
    );
    assert_ne!(one[0].reference.source, two[0].reference.source);
    assert_eq!(two[0].document.components[0].symbol, "OtherButton");
    fs::rename(repo, root.0.join("unavailable")).unwrap();
    let offline = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .unwrap();
    assert_eq!(one, offline);
}

#[test]
fn invalid_bundle_refresh_keeps_the_previous_selection_from_a_bare_remote() {
    let root = Temp::new("invalid-refresh");
    let work = bundle_repo(&root.0);
    let remote = root.0.join("remote.git");
    git(
        &root.0,
        &[
            "clone",
            "--bare",
            work.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    let source = bundle_source(&remote, "one");
    let cache = RegistryCache::open(root.0.join("cache"));
    let selected = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .unwrap();
    let definition_path = work.join("systems/one/1/.wax/wax.config.json");
    let registry_path = work.join("systems/one/1/.wax/registries/react.json");
    let definition = fs::read(&definition_path).unwrap();
    let registry = fs::read(&registry_path).unwrap();
    for invalid in [
        "definition",
        "missing-registry",
        "invalid-registry",
        "external-registry",
    ] {
        fs::write(&definition_path, &definition).unwrap();
        fs::write(&registry_path, &registry).unwrap();
        match invalid {
            "definition" => fs::write(&definition_path, b"{invalid").unwrap(),
            "missing-registry" => fs::remove_file(&registry_path).unwrap(),
            "invalid-registry" => fs::write(&registry_path, b"{}").unwrap(),
            _ => {
                let mut config: serde_json::Value = serde_json::from_slice(&definition).unwrap();
                config["design_systems"]["one"]["registries"]["react"]["source"] =
                    "https://invalid.example/react.json".into();
                fs::write(&definition_path, config.to_string()).unwrap();
            }
        }
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-qm", invalid]);
        git(&work, &["tag", "-f", "v1"]);
        git(
            &work,
            &["push", "--force", remote.to_str().unwrap(), "refs/tags/v1"],
        );
        assert!(cache.refresh(&source).is_err(), "accepted {invalid}");
        assert_eq!(
            cache
                .resolve(
                    &source,
                    &RegistryReadPolicy {
                        offline: true,
                        expected_snapshot: None,
                    }
                )
                .unwrap(),
            selected
        );
    }
}

#[test]
fn expected_bundle_snapshot_reads_history_and_verifies_definition_hash() {
    let root = Temp::new("expected");
    let repo = bundle_repo(&root.0);
    let source = bundle_source(&repo, "one");
    let cache = RegistryCache::open(root.0.join("cache"));
    let first = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .unwrap();
    fs::write(
        repo.join("systems/one/1/.wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"id":"other","symbol":"Other"}]}"#,
    )
    .unwrap();
    git(&repo, &["commit", "-am", "change", "-q"]);
    git(&repo, &["tag", "-f", "v1"]);
    let fresh = RegistryCache::open(root.0.join("fresh"));
    let expected = first[1].reference.clone();
    let reproduced = fresh
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: false,
                expected_snapshot: Some(expected.clone()),
            },
        )
        .unwrap();
    assert_eq!(reproduced, vec![first[1].clone()]);
    let mut wrong = expected;
    wrong.extra["definition"]["sha256"] = "e".repeat(64).into();
    assert!(
        fresh
            .resolve(
                &source,
                &RegistryReadPolicy {
                    offline: false,
                    expected_snapshot: Some(wrong)
                }
            )
            .is_err()
    );
}

#[test]
fn ordinary_source_resolution_honors_bundle_locks_even_when_upgrading() {
    use wax_core::{
        config::{lockfile::LockedRegistry, waxrc::load_waxrc},
        registry_source::resolve_language_registry_source,
    };
    let root = Temp::new("source");
    let repo = bundle_repo(&root.0);
    let app = root.0.join("app");
    fs::create_dir_all(&app).unwrap();
    let config = app.join("config.json");
    fs::write(&config, serde_json::json!({"schema_version":2,"languages":{"react":{"registry":{"git":repo,"tag":"v1","subdir":"systems/one/1","system":"one"}}}}).to_string()).unwrap();
    let waxrc = load_waxrc(&config).unwrap();
    let registry = waxrc.languages[0].registry_source.as_ref();
    let first = resolve_language_registry_source(&app, "react", registry, None, false).unwrap();
    assert_eq!(first.subdir.as_deref(), Some("systems/one/1"));
    assert_eq!(first.definition.as_ref().unwrap().system_id, "one");
    let lock = LockedRegistry {
        source: first.source.clone(),
        sha256: first.sha256.clone(),
        git: Some(repo.display().to_string()),
        tag: Some("v1".into()),
        commit: first.git_commit.clone(),
        subdir: first.subdir.clone(),
        definition: first.definition.clone(),
    };
    fs::write(
        repo.join("systems/one/1/.wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"id":"other","symbol":"Other"}]}"#,
    )
    .unwrap();
    git(&repo, &["commit", "-am", "change", "-q"]);
    git(&repo, &["tag", "-f", "v1"]);
    assert_eq!(
        resolve_language_registry_source(&app, "react", registry, Some(&lock), true).unwrap(),
        first
    );
    let mut wrong = lock;
    wrong.definition.as_mut().unwrap().sha256 = "e".repeat(64);
    assert!(
        resolve_language_registry_source(&app, "react", registry, Some(&wrong), false).is_err()
    );
}

#[test]
fn remembered_bundle_keeps_definition_and_emits_nested_app_config() {
    use wax_core::registry_memory::{
        add_remembered_registry, resolve_remembered_registry, show_remembered_design_system,
    };
    let root = Temp::new("remember");
    let repo = bundle_repo(&root.0);
    let state = root.0.join("home/state.json");
    let snapshots =
        add_remembered_registry(&state, "one", "One", &bundle_source(&repo, "one")).unwrap();
    let remembered = show_remembered_design_system(&state, "one").unwrap();
    let pin = remembered.bundle_pin.as_ref().unwrap();
    assert_eq!(pin.registries.len(), 2);
    assert_eq!(
        pin.commit,
        snapshots[0].reference.git_commit.as_deref().unwrap()
    );
    let resolved = resolve_remembered_registry(&remembered, &"react".try_into().unwrap()).unwrap();
    assert_eq!(
        resolved.definition.as_ref().unwrap().sha256,
        pin.definition.sha256
    );
    assert_eq!(
        resolved.to_config_registry_value()["subdir"],
        "systems/one/1"
    );
    assert_eq!(resolved.to_config_registry_value()["system"], "one");
}

#[cfg(unix)]
#[test]
fn failed_remembered_state_save_preserves_previous_cache_selection() {
    use wax_core::registry_memory::{
        add_remembered_registry, refresh_remembered_registry, show_remembered_design_system,
    };
    let root = Temp::new("failed-save");
    let repo = bundle_repo(&root.0);
    let state = root.0.join("home/state.json");
    let source = bundle_source(&repo, "one");
    let first = add_remembered_registry(&state, "one", "One", &source).unwrap();
    fs::write(
        repo.join("systems/one/1/.wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"id":"other","symbol":"Other"}]}"#,
    )
    .unwrap();
    git(&repo, &["commit", "-am", "change", "-q"]);
    git(&repo, &["tag", "-f", "v1"]);
    use std::os::unix::fs::PermissionsExt;
    let home = state.parent().unwrap();
    fs::set_permissions(home, fs::Permissions::from_mode(0o500)).unwrap();
    let result = refresh_remembered_registry(&state, "one");
    fs::set_permissions(home, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    let remembered = show_remembered_design_system(&state, "one").unwrap();
    assert_eq!(
        remembered.snapshots,
        first
            .iter()
            .map(|s| s.reference.clone())
            .collect::<Vec<_>>()
    );
    let cache = RegistryCache::open(root.0.join("home/cache/registries"));
    assert_eq!(
        cache
            .resolve(&source, &RegistryReadPolicy::default())
            .unwrap(),
        first
    );
}

#[test]
fn catalogue_hashes_gate_selection_and_provenance_is_cached_offline() {
    use wax_contract::{ArtifactDigest, CatalogEntry};
    use wax_core::digest::sha256_hex;
    let root = Temp::new("catalogue");
    let repo = bundle_repo(&root.0);
    fs::write(
        repo.join("systems/one/1/provenance.json"),
        b"{\"upstream\":[]}",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "provenance"]);
    git(&repo, &["tag", "-f", "v1"]);
    let artifact = |path: &str| ArtifactDigest {
        path: path.into(),
        sha256: sha256_hex(&fs::read(repo.join("systems/one/1").join(path)).unwrap()),
    };
    let entry = CatalogEntry {
        id: "one".into(),
        name: "One".into(),
        revision: 1,
        subdir: "systems/one/1".into(),
        minimum_wax_version: "0.0.0".into(),
        definition: artifact(".wax/wax.config.json"),
        registries: [
            (
                "compose".try_into().unwrap(),
                artifact(".wax/registries/compose.json"),
            ),
            (
                "react".try_into().unwrap(),
                artifact(".wax/registries/react.json"),
            ),
        ]
        .into(),
        provenance: artifact("provenance.json"),
        upstream: vec![],
        coverage: vec![],
        gaps: vec![],
    };
    let cache = RegistryCache::open(root.0.join("cache"));
    let mut wrong = entry.clone();
    wrong.definition.sha256 = "e".repeat(64);
    assert!(
        cache
            .resolve_catalog_entry(
                repo.to_str().unwrap(),
                "v1",
                &wrong,
                &RegistryReadPolicy::default()
            )
            .is_err()
    );
    let snapshots = cache
        .resolve_catalog_entry(
            repo.to_str().unwrap(),
            "v1",
            &entry,
            &RegistryReadPolicy::default(),
        )
        .unwrap();
    assert_eq!(
        snapshots[0].reference.extra["provenance"]["sha256"],
        entry.provenance.sha256
    );
    assert_eq!(
        cache
            .resolve_catalog_entry(
                repo.to_str().unwrap(),
                "v1",
                &entry,
                &RegistryReadPolicy {
                    offline: true,
                    expected_snapshot: Some(snapshots[1].reference.clone())
                },
            )
            .unwrap(),
        snapshots,
    );
    let fresh = RegistryCache::open(root.0.join("fresh"));
    let reproduced = fresh
        .resolve(
            &bundle_source(&repo, "one"),
            &RegistryReadPolicy {
                offline: false,
                expected_snapshot: Some(snapshots[1].reference.clone()),
            },
        )
        .unwrap();
    assert_eq!(reproduced, vec![snapshots[1].clone()]);
    fs::rename(&repo, root.0.join("unavailable")).unwrap();
    assert_eq!(
        cache
            .resolve_catalog_entry(
                repo.to_str().unwrap(),
                "v1",
                &entry,
                &RegistryReadPolicy {
                    offline: true,
                    expected_snapshot: None
                }
            )
            .unwrap(),
        snapshots
    );
    wrong = entry;
    wrong.registries.get_mut("react").unwrap().sha256 = "f".repeat(64);
    assert!(
        cache
            .resolve_catalog_entry(
                repo.to_str().unwrap(),
                "v1",
                &wrong,
                &RegistryReadPolicy {
                    offline: true,
                    expected_snapshot: None
                }
            )
            .is_err()
    );
}

#[test]
fn sync_writes_bundle_pins_that_lookup_reproduces_after_tag_moves() {
    use wax_core::{
        config::lockfile::load_lockfile,
        registry_lookup::{RegistryLookupOptions, resolve_lookup_snapshots},
        sync::{SyncOptions, sync_app_registries},
        validate::validate_repo,
    };
    let root = Temp::new("sync");
    let repo = bundle_repo(&root.0);
    let app = root.0.join("app");
    fs::create_dir_all(app.join(".wax")).unwrap();
    let registry =
        serde_json::json!({"git":repo,"tag":"v1","subdir":"systems/one/1","system":"one"});
    fs::write(app.join(".wax/wax.config.json"), serde_json::json!({"schema_version":2,"languages":{"compose":{"registry":registry},"react":{"registry":registry}}}).to_string()).unwrap();
    let lock_path = app.join(".wax/wax.lock.json");
    fs::write(&lock_path, r#"{"schema_version":2,"engine_api_version":1,"wax_version":"0.1.0","registries":{},"languages":{}}"#).unwrap();
    sync_app_registries(&SyncOptions {
        design_system: None,
        repo_root: app.clone(),
        state_path: None,
        upgrade: false,
    })
    .unwrap();
    let first = load_lockfile(&lock_path).unwrap();
    assert_eq!(first.schema_version, 3);
    assert_eq!(
        first.registries["react"]
            .definition
            .as_ref()
            .unwrap()
            .system_id,
        "one"
    );
    validate_repo(&app).unwrap();
    fs::write(
        repo.join("systems/one/1/.wax/registries/react.json"),
        r#"{"schema_version":1,"components":[{"id":"other","symbol":"Other"}]}"#,
    )
    .unwrap();
    git(&repo, &["commit", "-am", "change", "-q"]);
    git(&repo, &["tag", "-f", "v1"]);
    sync_app_registries(&SyncOptions {
        design_system: None,
        repo_root: app.clone(),
        state_path: None,
        upgrade: true,
    })
    .unwrap();
    assert_eq!(
        load_lockfile(&lock_path).unwrap().registries,
        first.registries
    );
    let options = RegistryLookupOptions {
        repo_root: app,
        selector: None,
        language: None,
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("lookup/state.json")),
    };
    let snapshots = resolve_lookup_snapshots(&options).unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(
        snapshots[1].reference.digest,
        first.registries["react"].sha256
    );
    assert_eq!(
        snapshots[1].reference.extra["definition"]["system_id"],
        "one"
    );
    fs::rename(repo, root.0.join("unavailable")).unwrap();
    assert_eq!(
        resolve_lookup_snapshots(&RegistryLookupOptions {
            offline: true,
            ..options
        })
        .unwrap(),
        snapshots
    );
}

#[test]
fn cached_bundle_snapshot_must_match_the_requested_subdirectory() {
    let root = Temp::new("snapshot-subdir");
    let repo = bundle_repo(&root.0);
    let source = bundle_source(&repo, "one");
    let cache = RegistryCache::open(root.0.join("cache"));
    let snapshots = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .unwrap();
    let mut expected = snapshots[0].reference.clone();
    expected.extra["subdir"] = "systems/two/1".into();
    expected.extra["definition"]["path"] = "systems/two/1/.wax/wax.config.json".into();
    assert!(
        cache
            .resolve(
                &source,
                &RegistryReadPolicy {
                    offline: true,
                    expected_snapshot: Some(expected)
                }
            )
            .is_err()
    );
}

#[test]
fn bundle_snapshot_rejects_partial_metadata_even_with_a_warm_registry_blob() {
    let root = Temp::new("partial-snapshot");
    let repo = bundle_repo(&root.0);
    let source = bundle_source(&repo, "one");
    let cache = RegistryCache::open(root.0.join("cache"));
    let snapshots = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .unwrap();
    for field in ["definition", "subdir"] {
        let mut expected = snapshots[0].reference.clone();
        expected.extra.remove(field);
        assert!(
            cache
                .resolve(
                    &source,
                    &RegistryReadPolicy {
                        offline: true,
                        expected_snapshot: Some(expected)
                    }
                )
                .is_err(),
            "accepted missing {field}"
        );
    }
}

#[test]
fn remembered_bundle_rejects_mixed_or_partial_pins_on_load() {
    use wax_core::registry_memory::{add_remembered_registry, show_remembered_design_system};
    let root = Temp::new("invalid-state");
    let repo = bundle_repo(&root.0);
    let state = root.0.join("home/state.json");
    add_remembered_registry(&state, "one", "One", &bundle_source(&repo, "one")).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    let mut mixed = original.clone();
    mixed["design_systems"]["one"]["snapshots"][1]["git_commit"] = "f".repeat(40).into();
    fs::write(&state, mixed.to_string()).unwrap();
    assert!(show_remembered_design_system(&state, "one").is_err());
    let mut partial = original;
    partial["design_systems"]["one"]
        .as_object_mut()
        .unwrap()
        .remove("bundle_pin");
    fs::write(&state, partial.to_string()).unwrap();
    assert!(show_remembered_design_system(&state, "one").is_err());
}
