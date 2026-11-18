use std::{fs, path::PathBuf};
use wax_core::global_state::{GlobalState, RememberedDesignSystem, save_global_state};
use wax_core::registry_catalog::{CatalogOptions, find_design_systems};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wax-catalog-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn options(&self) -> CatalogOptions {
        CatalogOptions {
            repo_root: self.0.clone(),
            state_path: self.0.join("home/state.json"),
            cache_root: self.0.join("home/cache/registries"),
            offline: true,
            ..Default::default()
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cold_offline_find_retains_origin_collisions_and_filters_deterministically() {
    let f = Fixture::new("cold");
    fs::create_dir_all(f.0.join(".wax")).unwrap();
    fs::write(f.0.join(".wax/wax.config.json"), r#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme","registries":{"react":{"source":"react.json"}}}}}"#).unwrap();
    let options = f.options();
    let mut state = GlobalState::default();
    state.design_systems.insert(
        "acme".into(),
        RememberedDesignSystem {
            name: "Acme remembered".into(),
            repo_root: f.0.clone(),
            last_seen_config: ".wax/wax.config.json".into(),
            source: None,
            snapshots: vec![],
            bundle_pin: None,
        },
    );
    save_global_state(&options.state_path, &state).unwrap();
    let result = find_design_systems(&options).unwrap();
    assert_eq!(
        result
            .results
            .iter()
            .map(|r| (r.origin.as_str(), r.id.as_str()))
            .collect::<Vec<_>>(),
        vec![("configured", "acme"), ("remembered", "acme")]
    );
    assert!(!result.public_status.complete);
    assert_eq!(
        result.public_status.diagnostic.unwrap().code,
        "offline_cache_miss"
    );
    assert_eq!(
        find_design_systems(&CatalogOptions {
            query: "ACM".into(),
            language: Some("react".try_into().unwrap()),
            ..options.clone()
        })
        .unwrap()
        .results
        .len(),
        2
    );
    assert!(
        find_design_systems(&CatalogOptions {
            language: Some("swift".try_into().unwrap()),
            ..options
        })
        .unwrap()
        .results
        .is_empty()
    );
}

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Wax Test",
            "-c",
            "user.email=test@example.com",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn catalogue(f: &Fixture) -> CatalogOptions {
    let repo = f.0.join("remote");
    fs::create_dir_all(repo.join("catalog")).unwrap();
    git(&repo, &["init", "-qb", "main"]);
    let bundle = repo.join("systems/acme/1");
    fs::create_dir_all(bundle.join(".wax/registries")).unwrap();
    fs::write(bundle.join(".wax/wax.config.json"), r#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme","registries":{"react":{"source":".wax/registries/react.json"},"compose":{"source":".wax/registries/compose.json"}}}}}"#).unwrap();
    fs::write(bundle.join("provenance.json"), "{\"upstream\":[]}").unwrap();
    for language in ["compose", "react"] {
        fs::write(bundle.join(format!(".wax/registries/{language}.json")), format!(r##"{{"schema_version":1,"language":"{language}","components":[{{"id":"button","symbol":"Button"}}],"tokens":[{{"id":"primary","key":"color.primary","value":"#fff","category":"color"}}]}}"##)).unwrap();
    }
    let artifact = |path: &str| serde_json::json!({"path":path,"sha256":wax_core::digest::sha256_hex(&fs::read(bundle.join(path)).unwrap())});
    let index = serde_json::json!({"schema_version":1,"entries":[{"id":"acme","name":"Acme","revision":1,"subdir":"systems/acme/1","minimum_wax_version":"0.0.0","definition":artifact(".wax/wax.config.json"),"provenance":artifact("provenance.json"),"registries":{"compose":artifact(".wax/registries/compose.json"),"react":artifact(".wax/registries/react.json")},"upstream":[],"coverage":["buttons"],"gaps":["icons"]}]});
    fs::write(repo.join("catalog/index.json"), index.to_string()).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "catalogue"]);
    CatalogOptions {
        offline: false,
        source: wax_core::registry_catalog::CatalogSource {
            repository: repo.display().to_string(),
            ..Default::default()
        },
        ..f.options()
    }
}
#[test]
fn online_find_refreshes_public_rows_and_offline_reuses_selected_commit() {
    let f = Fixture::new("online");
    let options = catalogue(&f);
    let first = find_design_systems(&options).unwrap();
    assert!(first.public_status.complete);
    assert!(!first.public_status.cached);
    assert_eq!(first.results[0].id, "acme");
    assert_eq!(first.results[0].revision, Some(1));
    assert_eq!(first.results[0].origin, "public");
    assert!(!first.results[0].installed);
    let repo = std::path::Path::new(&options.source.repository);
    fs::write(repo.join("unrelated"), "new commit").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "advance"]);
    let second = find_design_systems(&options).unwrap();
    assert_ne!(first.public_status.commit, second.public_status.commit);
    fs::rename(repo, f.0.join("unavailable")).unwrap();
    let warm = find_design_systems(&CatalogOptions {
        offline: true,
        ..options.clone()
    })
    .unwrap();
    assert!(warm.public_status.complete && warm.public_status.cached);
    assert_eq!(warm.results, second.results);
    let partial = find_design_systems(&options).unwrap();
    assert!(!partial.public_status.complete);
    assert_eq!(
        partial.public_status.diagnostic.unwrap().code,
        "public_unavailable"
    );
}

#[test]
fn installs_entire_bundle_at_selected_commit_and_repeats_without_reselecting() {
    use wax_core::registry_catalog::install_design_system;
    use wax_core::registry_memory::show_remembered_design_system;
    let f = Fixture::new("install");
    let options = catalogue(&f);
    let discovery = find_design_systems(&options).unwrap();
    let selected = discovery.public_status.commit.unwrap();
    let repo = std::path::Path::new(&options.source.repository);
    // Branch can move (and bundle bytes can become invalid) after selection.
    fs::write(repo.join("systems/acme/1/.wax/registries/react.json"), "{}").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "move branch"]);
    let first = install_design_system(&options, "acme", 1).unwrap();
    assert_eq!(first.commit, selected);
    assert_eq!(first.registries.len(), 2);
    let remembered = show_remembered_design_system(&options.state_path, "public-acme-r1").unwrap();
    assert_eq!(remembered.bundle_pin, Some(first.clone()));
    assert_eq!(install_design_system(&options, "acme", 1).unwrap(), first);
    assert!(
        find_design_systems(&CatalogOptions {
            offline: true,
            ..options.clone()
        })
        .unwrap()
        .results
        .iter()
        .find(|r| r.origin == "public")
        .unwrap()
        .installed
    );
    fs::rename(repo, f.0.join("unavailable")).unwrap();
    assert_eq!(
        install_design_system(
            &CatalogOptions {
                offline: true,
                ..options
            },
            "acme",
            1
        )
        .unwrap(),
        first
    );
}

#[test]
fn offline_install_uses_git_objects_cached_by_discovery_without_prior_install() {
    let f = Fixture::new("offline-install");
    let options = catalogue(&f);
    let discovery = find_design_systems(&options).unwrap();
    fs::rename(&options.source.repository, f.0.join("unavailable")).unwrap();
    let pin = wax_core::registry_catalog::install_design_system(
        &CatalogOptions {
            offline: true,
            ..options
        },
        "acme",
        1,
    )
    .unwrap();
    assert_eq!(Some(pin.commit), discovery.public_status.commit);
    assert_eq!(pin.registries.len(), 2);
}

#[test]
fn public_lookup_keeps_history_after_refresh_without_remembering_a_system() {
    use wax_core::registry_catalog::resolve_catalog_snapshots;
    let f = Fixture::new("lookup");
    let options = catalogue(&f);
    let language = "react".try_into().unwrap();
    let first = resolve_catalog_snapshots(&options, "acme", 1, Some(&language), None).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].document.components[0].symbol, "Button");
    assert!(
        wax_core::global_state::load_global_state(&options.state_path)
            .unwrap()
            .design_systems
            .is_empty()
    );
    let digest = first[0].reference.digest.clone();
    let repo = std::path::Path::new(&options.source.repository);
    let registry = repo.join("systems/acme/1/.wax/registries/react.json");
    fs::write(&registry, r#"{"schema_version":1,"language":"react","components":[{"id":"button","symbol":"ChangedButton"}]}"#).unwrap();
    let index_path = repo.join("catalog/index.json");
    let mut index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    index["entries"][0]["registries"]["react"]["sha256"] =
        wax_core::digest::sha256_hex(&fs::read(&registry).unwrap()).into();
    fs::write(index_path, index.to_string()).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "refresh"]);
    find_design_systems(&options).unwrap();
    assert_ne!(
        resolve_catalog_snapshots(&options, "acme", 1, Some(&language), None).unwrap(),
        first
    );
    let offline = CatalogOptions {
        offline: true,
        ..options.clone()
    };
    assert_eq!(
        resolve_catalog_snapshots(&offline, "acme", 1, Some(&language), Some(&digest)).unwrap(),
        first
    );
    assert!(
        resolve_catalog_snapshots(&offline, "other", 1, Some(&language), Some(&digest)).is_err()
    );
    assert!(
        resolve_catalog_snapshots(
            &offline,
            "acme",
            1,
            Some(&"compose".try_into().unwrap()),
            Some(&digest)
        )
        .is_err()
    );
    assert!(
        resolve_catalog_snapshots(&offline, "acme", 1, Some(&language), Some(&"e".repeat(64)))
            .is_err()
    );
}

#[test]
fn find_includes_configured_app_selections_and_bundle_revision() {
    let f = Fixture::new("app-find");
    fs::create_dir_all(f.0.join(".wax")).unwrap();
    fs::write(f.0.join(".wax/wax.config.json"), r#"{"schema_version":2,"languages":{"react":{"registry":{"source":"react.json","upstream":"acme/react"}},"compose":{"registry":{"git":"https://example.test/ds.git","tag":"v1","subdir":"systems/other/2","system":"other"}}}}"#).unwrap();
    let result = find_design_systems(&f.options()).unwrap();
    assert_eq!(
        result
            .results
            .iter()
            .map(|r| (r.id.as_str(), r.languages[0].as_str()))
            .collect::<Vec<_>>(),
        vec![("acme", "react"), ("other", "compose")]
    );
}

fn edit_index(options: &CatalogOptions, edit: impl FnOnce(&mut serde_json::Value)) {
    let repo = std::path::Path::new(&options.source.repository);
    let path = repo.join("catalog/index.json");
    let mut json = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut json);
    fs::write(path, json.to_string()).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "update index"]);
}
#[test]
fn invalid_public_indexes_are_partial_but_invalid_local_state_is_fatal() {
    let f = Fixture::new("invalid");
    let options = catalogue(&f);
    for (key, value) in [("minimum_wax_version", "999.0.0"), ("subdir", "../escape")] {
        edit_index(&options, |index| {
            index["entries"][0]["minimum_wax_version"] = "0.0.0".into();
            index["entries"][0][key] = value.into();
        });
        let result = find_design_systems(&options).unwrap();
        assert!(!result.public_status.complete);
        assert_eq!(
            result.public_status.diagnostic.unwrap().code,
            "public_unavailable"
        );
        assert!(wax_core::registry_catalog::install_design_system(&options, "acme", 1).is_err());
    }
    fs::write(
        std::path::Path::new(&options.source.repository).join("catalog/index.json"),
        "{broken",
    )
    .unwrap();
    git(
        std::path::Path::new(&options.source.repository),
        &["commit", "-qam", "malformed"],
    );
    assert!(
        !find_design_systems(&options)
            .unwrap()
            .public_status
            .complete
    );
    fs::create_dir_all(options.state_path.parent().unwrap()).unwrap();
    fs::write(&options.state_path, "{broken").unwrap();
    assert!(find_design_systems(&options).is_err());
    fs::remove_file(&options.state_path).unwrap();
    fs::create_dir_all(f.0.join(".wax")).unwrap();
    fs::write(f.0.join(".wax/wax.config.json"), "{broken").unwrap();
    assert!(find_design_systems(&options).is_err());
}
#[test]
fn digest_failure_and_local_slug_collision_preserve_previous_state() {
    use wax_core::registry_catalog::{CatalogError, install_design_system};
    let f = Fixture::new("conflict");
    let options = catalogue(&f);
    let mut state = GlobalState::default();
    state.design_systems.insert(
        "public-acme-r1".into(),
        RememberedDesignSystem {
            name: "Local".into(),
            repo_root: f.0.clone(),
            last_seen_config: ".wax/wax.config.json".into(),
            source: None,
            snapshots: vec![],
            bundle_pin: None,
        },
    );
    save_global_state(&options.state_path, &state).unwrap();
    assert!(matches!(
        install_design_system(&options, "acme", 1),
        Err(CatalogError::Conflict { .. })
    ));
    assert_eq!(
        wax_core::global_state::load_global_state(&options.state_path).unwrap(),
        state
    );
    state.design_systems.clear();
    save_global_state(&options.state_path, &state).unwrap();
    edit_index(&options, |i| {
        i["entries"][0]["registries"]["react"]["sha256"] = "e".repeat(64).into()
    });
    find_design_systems(&options).unwrap();
    assert!(install_design_system(&options, "acme", 1).is_err());
    assert_eq!(
        wax_core::global_state::load_global_state(&options.state_path).unwrap(),
        state
    );
}
#[test]
fn concurrent_revisions_are_independent_and_historical_repeat_keeps_original_commit() {
    use wax_core::registry_catalog::install_design_system;
    let f = Fixture::new("concurrent");
    let options = catalogue(&f);
    let first = install_design_system(&options, "acme", 1).unwrap();
    edit_index(&options, |i| {
        let mut second = i["entries"][0].clone();
        second["revision"] = 2.into();
        i["entries"].as_array_mut().unwrap().push(second);
    });
    find_design_systems(&options).unwrap();
    assert_eq!(install_design_system(&options, "acme", 1).unwrap(), first);
    // Independent installs share a state lock; neither may lose the other's entry.
    fs::remove_file(&options.state_path).unwrap();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| install_design_system(&options, "acme", 1).unwrap());
        let b = scope.spawn(|| install_design_system(&options, "acme", 2).unwrap());
        assert_eq!(a.join().unwrap().system_id, "acme");
        assert_eq!(b.join().unwrap().system_id, "acme");
    });
    let state = wax_core::global_state::load_global_state(&options.state_path).unwrap();
    assert_eq!(
        state
            .design_systems
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["public-acme-r1", "public-acme-r2"]
    );
}

#[test]
fn unchanged_historical_digest_resolves_once_across_catalogue_commits() {
    use wax_core::registry_catalog::resolve_catalog_snapshots;
    let f = Fixture::new("unchanged");
    let options = catalogue(&f);
    let language = "react".try_into().unwrap();
    let first = resolve_catalog_snapshots(&options, "acme", 1, Some(&language), None).unwrap();
    let repo = std::path::Path::new(&options.source.repository);
    fs::write(repo.join("unrelated"), "advance").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "advance"]);
    find_design_systems(&options).unwrap();
    resolve_catalog_snapshots(&options, "acme", 1, Some(&language), None).unwrap();
    assert_eq!(
        resolve_catalog_snapshots(
            &options,
            "acme",
            1,
            Some(&language),
            Some(&first[0].reference.digest)
        )
        .unwrap(),
        first
    );
}

#[test]
fn online_install_refreshes_when_selected_index_cannot_supply_requested_revision() {
    let f = Fixture::new("install-new-revision");
    let options = catalogue(&f);
    find_design_systems(&options).unwrap();
    edit_index(&options, |i| {
        let mut second = i["entries"][0].clone();
        second["revision"] = 2.into();
        i["entries"].as_array_mut().unwrap().push(second);
    });
    let pin = wax_core::registry_catalog::install_design_system(&options, "acme", 2).unwrap();
    assert_eq!(
        pin.commit,
        git(
            std::path::Path::new(&options.source.repository),
            &["rev-parse", "HEAD"]
        )
    );
}

#[test]
fn changed_artifact_path_cannot_be_treated_as_an_idempotent_install() {
    let f = Fixture::new("path-conflict");
    let options = catalogue(&f);
    let first = wax_core::registry_catalog::install_design_system(&options, "acme", 1).unwrap();
    edit_index(&options, |i| {
        i["entries"][0]["registries"]["react"]["path"] = "different.json".into()
    });
    find_design_systems(&options).unwrap();
    assert!(matches!(
        wax_core::registry_catalog::install_design_system(&options, "acme", 1),
        Err(wax_core::registry_catalog::CatalogError::Conflict { .. })
    ));
    assert_eq!(
        wax_core::registry_memory::show_remembered_design_system(
            &options.state_path,
            "public-acme-r1"
        )
        .unwrap()
        .bundle_pin,
        Some(first)
    );
}

#[test]
fn missing_ref_blob_and_repository_are_explicit_public_failures() {
    let f = Fixture::new("missing");
    let options = catalogue(&f);
    let mut missing_ref = options.clone();
    missing_ref.source.branch = "absent".into();
    assert!(
        !find_design_systems(&missing_ref)
            .unwrap()
            .public_status
            .complete
    );
    assert!(wax_core::registry_catalog::install_design_system(&missing_ref, "acme", 1).is_err());
    let repo = std::path::Path::new(&options.source.repository);
    fs::remove_file(repo.join("systems/acme/1/provenance.json")).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "remove provenance"]);
    assert!(wax_core::registry_catalog::install_design_system(&options, "acme", 1).is_err());
    assert!(
        wax_core::global_state::load_global_state(&options.state_path)
            .unwrap()
            .design_systems
            .is_empty()
    );
    fs::rename(repo, f.0.join("unavailable")).unwrap();
    assert!(
        !find_design_systems(&options)
            .unwrap()
            .public_status
            .complete
    );
}
#[cfg(unix)]
#[test]
fn failed_state_write_does_not_remove_existing_installs() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("state-write");
    let options = catalogue(&f);
    wax_core::registry_catalog::install_design_system(&options, "acme", 1).unwrap();
    edit_index(&options, |i| {
        let mut second = i["entries"][0].clone();
        second["revision"] = 2.into();
        i["entries"].as_array_mut().unwrap().push(second);
    });
    find_design_systems(&options).unwrap();
    let previous = wax_core::global_state::load_global_state(&options.state_path).unwrap();
    let parent = options.state_path.parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
    let result = wax_core::registry_catalog::install_design_system(&options, "acme", 2);
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(
        wax_core::global_state::load_global_state(&options.state_path).unwrap(),
        previous
    );
}

#[test]
fn minimum_version_accepts_the_engine_version_advertised_by_the_binary() {
    let f = Fixture::new("build-version");
    let options = catalogue(&f);
    edit_index(&options, |i| {
        i["entries"][0]["minimum_wax_version"] = wax_lang_api::build_version().into()
    });
    let response = find_design_systems(&options).unwrap();
    assert!(
        response.public_status.complete,
        "{:?}",
        response.public_status.diagnostic
    );
}

#[test]
fn remembered_public_rows_expose_the_installed_bundle_revision_and_commit() {
    let f = Fixture::new("remembered-row");
    let options = catalogue(&f);
    let installed = wax_core::registry_catalog::install_design_system(&options, "acme", 1).unwrap();
    let found = find_design_systems(&CatalogOptions {
        offline: true,
        ..options
    })
    .unwrap();
    let row = found
        .results
        .iter()
        .find(|r| r.origin == "remembered")
        .unwrap();
    assert_eq!(row.revision, Some(1));
    assert_eq!(
        row.catalogue_commit.as_deref(),
        Some(installed.commit.as_str())
    );
}

#[test]
fn public_union_retains_same_id_from_all_origins_with_stable_json_rows() {
    let f = Fixture::new("union");
    let options = catalogue(&f);
    fs::create_dir_all(f.0.join(".wax")).unwrap();
    fs::write(f.0.join(".wax/wax.config.json"), r#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme local","registries":{"react":{"source":"react.json"}}}}}"#).unwrap();
    let mut state = GlobalState::default();
    state.design_systems.insert(
        "acme".into(),
        RememberedDesignSystem {
            name: "Acme remembered".into(),
            repo_root: f.0.clone(),
            last_seen_config: ".wax/wax.config.json".into(),
            source: None,
            snapshots: vec![],
            bundle_pin: None,
        },
    );
    save_global_state(&options.state_path, &state).unwrap();
    let first = find_design_systems(&options).unwrap();
    assert_eq!(
        first
            .results
            .iter()
            .map(|r| (r.origin.as_str(), r.id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("configured", "acme"),
            ("public", "acme"),
            ("remembered", "acme")
        ]
    );
    let again = find_design_systems(&options).unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
}
#[test]
fn concurrent_source_collision_installs_only_one_source_without_overwrite() {
    let a = Fixture::new("source-a");
    let b = Fixture::new("source-b");
    let options_a = catalogue(&a);
    let mut options_b = catalogue(&b);
    options_b.state_path = options_a.state_path.clone();
    options_b.cache_root = options_a.cache_root.clone();
    let (first, second) = std::thread::scope(|scope| {
        let first = scope
            .spawn(|| wax_core::registry_catalog::install_design_system(&options_a, "acme", 1));
        let second = scope
            .spawn(|| wax_core::registry_catalog::install_design_system(&options_b, "acme", 1));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_ne!(first.is_ok(), second.is_ok());
    let pin = first.or(second).unwrap();
    assert_eq!(
        wax_core::registry_memory::show_remembered_design_system(
            &options_a.state_path,
            "public-acme-r1"
        )
        .unwrap()
        .bundle_pin,
        Some(pin)
    );
}
#[test]
fn installation_preserves_app_files_and_language_pack_state() {
    use wax_core::global_state::InstalledLanguagePack;
    let f = Fixture::new("app-files");
    let options = catalogue(&f);
    fs::create_dir_all(f.0.join(".wax")).unwrap();
    fs::write(f.0.join(".wax/wax.config.json"), "app config sentinel").unwrap();
    fs::write(f.0.join(".wax/wax.lock.json"), "app lock sentinel").unwrap();
    let mut state = GlobalState::default();
    state.installed_languages.insert(
        "react".try_into().unwrap(),
        [(
            "1.0.0".into(),
            InstalledLanguagePack {
                install_dir: f.0.join("pack"),
            },
        )]
        .into(),
    );
    save_global_state(&options.state_path, &state).unwrap();
    wax_core::registry_catalog::install_design_system(&options, "acme", 1).unwrap();
    assert_eq!(
        fs::read_to_string(f.0.join(".wax/wax.config.json")).unwrap(),
        "app config sentinel"
    );
    assert_eq!(
        fs::read_to_string(f.0.join(".wax/wax.lock.json")).unwrap(),
        "app lock sentinel"
    );
    assert_eq!(
        wax_core::global_state::load_global_state(&options.state_path)
            .unwrap()
            .installed_languages,
        state.installed_languages
    );
}
