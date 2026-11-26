use crate::EngineError;
use crate::adoption_merge::MergeOptions;
use crate::config::lockfile::WaxLock;
use crate::config::waxrc::{EngineConfig, LanguageRegistrySource, WaxRc};
use crate::root_group::RootGroups;
use crate::{registry_lock, registry_source};
use std::collections::BTreeMap;
use std::path::Path;
use wax_contract::LanguageId;
use wax_lang_api::ScanConfig;

#[derive(Default)]
pub(crate) struct ScanSelection {
    pub root_group: Option<String>,
    pub scan_concurrency: Option<u32>,
}

pub(crate) struct PlannedScan {
    pub language_id: LanguageId,
    pub config: ScanConfig,
}

pub(crate) struct ScanPlan {
    /// Scans ordered by language id.
    pub scans: Vec<PlannedScan>,
    pub merge: MergeOptions,
    pub concurrency: usize,
    pub lockfile: WaxLock,
    pub lockfile_changed: bool,
}

/// Plans which packs to run without launching them or persisting config or lockfile changes.
/// Registry resolution may populate the registry cache.
pub(crate) fn prepare_scan(
    repo_root: &Path,
    waxrc: WaxRc,
    mut lockfile: WaxLock,
    selection: &ScanSelection,
) -> Result<ScanPlan, EngineError> {
    let root_groups = configured_root_groups(&waxrc, selection)?;
    let mut language_configs = BTreeMap::new();
    let mut lockfile_changed = false;
    for entry in waxrc.languages {
        let roots = if let Some(root_group) = &selection.root_group {
            let Some(roots) = entry.root_groups.get(root_group) else {
                continue;
            };
            roots.clone()
        } else if entry.root_groups.is_empty() {
            entry.roots
        } else {
            entry.root_groups.into_values().flatten().collect()
        };
        let resolved_registry = registry_source::resolve_language_registry_source(
            repo_root,
            entry.id.as_str(),
            entry.registry_source.as_ref(),
            lockfile.registries.get(&entry.id),
            false,
        )?;
        if entry
            .registry_source
            .as_ref()
            .and_then(LanguageRegistrySource::git_parts)
            .is_some()
            && !lockfile.registries.contains_key(&entry.id)
        {
            lockfile.registries.insert(
                entry.id.clone(),
                resolved_registry.to_locked(entry.registry_source.as_ref()),
            );
            lockfile_changed = true;
        } else {
            registry_lock::verify_registry_lock(&entry.id, &resolved_registry, &lockfile)
                .map_err(registry_lock_mismatch_to_engine_error)?;
        }

        let mut config = entry.extra;
        if !roots.is_empty() {
            config.insert(
                "roots".to_owned(),
                serde_json::Value::Array(
                    roots.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }
        config.insert(
            "registry".to_owned(),
            serde_json::Value::String(resolved_registry.repo_relative_path),
        );
        language_configs.insert(entry.id, config);
    }

    Ok(ScanPlan {
        scans: language_configs
            .into_iter()
            .map(|(language_id, config)| PlannedScan {
                language_id,
                config,
            })
            .collect(),
        merge: MergeOptions {
            parent_scope_limit: waxrc.adoption.symbol_usage_summary.parent_scope_limit,
            token_inference: waxrc.token_inference,
            root_groups: root_groups_for_scan(root_groups, selection.root_group.as_deref()),
            scan_scope: selection.root_group.clone(),
        },
        concurrency: effective_scan_concurrency(&waxrc.engine, selection),
        lockfile,
        lockfile_changed,
    })
}

pub(crate) fn configured_root_groups(
    waxrc: &WaxRc,
    selection: &ScanSelection,
) -> Result<RootGroups, EngineError> {
    let mut root_groups = RootGroups::new();
    for entry in &waxrc.languages {
        for (group, roots) in &entry.root_groups {
            root_groups
                .entry(group.clone())
                .or_default()
                .insert(entry.id.clone(), roots.clone());
        }
    }
    if let Some(root_group) = &selection.root_group
        && !root_groups.contains_key(root_group)
    {
        return Err(EngineError::UnknownRootGroup {
            root_group: root_group.clone(),
            configured: root_groups.keys().cloned().collect(),
        });
    }
    Ok(root_groups)
}

fn root_groups_for_scan(root_groups: RootGroups, selected: Option<&str>) -> RootGroups {
    match selected {
        Some(selected) => root_groups
            .into_iter()
            .filter(|(group, _)| group == selected)
            .collect(),
        None => root_groups,
    }
}

fn effective_scan_concurrency(engine_config: &EngineConfig, selection: &ScanSelection) -> usize {
    let configured = selection
        .scan_concurrency
        .unwrap_or(engine_config.scan_concurrency);
    configured.max(1) as usize
}

fn registry_lock_mismatch_to_engine_error(
    mismatch: registry_lock::RegistryLockMismatch,
) -> EngineError {
    match mismatch {
        registry_lock::RegistryLockMismatch::Missing { language_id } => EngineError::RegistryLock {
            language_id,
            reason: "missing registry lock entry".to_owned(),
        },
        registry_lock::RegistryLockMismatch::SourceDrift {
            language_id,
            lockfile_source,
            resolved_source,
        } => EngineError::RegistryLock {
            language_id,
            reason: format!("source changed from {lockfile_source} to {resolved_source}"),
        },
        registry_lock::RegistryLockMismatch::DigestDrift {
            language_id,
            lockfile_sha256,
            resolved_sha256,
        } => EngineError::RegistryLock {
            language_id,
            reason: format!("digest changed from {lockfile_sha256} to {resolved_sha256}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ScanSelection, prepare_scan};
    use crate::config::lockfile::WaxLock;
    use crate::config::waxrc::{WaxRc, parse_waxrc_bytes};
    use crate::paths::WaxHome;
    use crate::{Engine, EngineError, EphemeralScanConfig, ScanOptions};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wax_contract::LanguageId;

    const REGISTRY: &[u8] = br#"{"schema_version":1,"components":[]}"#;
    const REGISTRY_SHA256: &str =
        "357519d8798a9635e3f6316fcdeacdaba5f2b389d3f465c1b2026ab6b96593dd";

    fn compose_lock(sha256: &str) -> WaxLock {
        let mut lockfile = empty_lock();
        lockfile.registries.insert(
            LanguageId::try_from("compose").unwrap(),
            serde_json::from_value(json!({
                "source": ".wax/compose.registry.json",
                "sha256": sha256
            }))
            .unwrap(),
        );
        lockfile
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "wax-scan-plan-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn with_registry() -> Self {
            let dir = Self::new();
            fs::create_dir(dir.0.join(".wax")).unwrap();
            fs::write(dir.0.join(".wax/compose.registry.json"), REGISTRY).unwrap();
            dir
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn config(languages: Value) -> WaxRc {
        let json = json!({ "schema_version": 2, "languages": languages });
        parse_waxrc_bytes(json.to_string().as_bytes(), Path::new("wax.config.json")).unwrap()
    }

    fn empty_lock() -> WaxLock {
        serde_json::from_value(json!({
            "schema_version": 3,
            "engine_api_version": 1,
            "wax_version": "0.0.0",
            "registries": {},
            "languages": {}
        }))
        .unwrap()
    }

    #[test]
    fn unknown_group_reports_sorted_configured_groups() {
        let waxrc = config(json!({
            "compose": { "roots": { "zeta": ["app"] } },
            "react": { "roots": { "alpha": ["web"] } }
        }));
        let selection = ScanSelection {
            root_group: Some("missing".to_owned()),
            scan_concurrency: None,
        };

        let Err(EngineError::UnknownRootGroup {
            root_group,
            configured,
        }) = prepare_scan(Path::new("."), waxrc, empty_lock(), &selection)
        else {
            panic!("expected an unknown root group error");
        };
        assert_eq!(root_group, "missing");
        assert_eq!(configured, ["alpha", "zeta"]);
    }

    #[test]
    fn selected_group_only_plans_languages_in_that_group() {
        let repo = TestDir::with_registry();
        let waxrc = config(json!({
            "compose": { "roots": { "app": ["android"], "ui": ["components"] } },
            "react": { "roots": { "ui": ["web"] } },
            "swift": { "roots": ["ios"] }
        }));
        let selection = ScanSelection {
            root_group: Some("app".to_owned()),
            scan_concurrency: None,
        };

        let plan = prepare_scan(&repo.0, waxrc, compose_lock(REGISTRY_SHA256), &selection).unwrap();

        let [scan] = plan.scans.as_slice() else {
            panic!("expected only the language configured for app");
        };
        assert_eq!(scan.language_id.as_str(), "compose");
        assert_eq!(scan.config["roots"], json!(["android"]));
        assert_eq!(plan.merge.scan_scope.as_deref(), Some("app"));
        assert_eq!(plan.merge.root_groups.keys().collect::<Vec<_>>(), ["app"]);
    }

    #[test]
    fn unscoped_scan_combines_all_group_roots() {
        let repo = TestDir::with_registry();
        let waxrc = config(json!({
            "compose": { "roots": { "ui": ["components", "shared"], "app": ["android"] } }
        }));

        let plan = prepare_scan(
            &repo.0,
            waxrc,
            compose_lock(REGISTRY_SHA256),
            &ScanSelection::default(),
        )
        .unwrap();

        assert_eq!(
            plan.scans[0].config["roots"],
            json!(["android", "components", "shared"])
        );
        assert_eq!(plan.merge.scan_scope, None);
        assert_eq!(
            plan.merge.root_groups.keys().collect::<Vec<_>>(),
            ["app", "ui"]
        );
    }

    fn git_registry() -> (TestDir, String) {
        let publisher = TestDir::new();
        fs::create_dir_all(publisher.0.join(".wax/registries")).unwrap();
        fs::write(publisher.0.join(".wax/registries/compose.json"), REGISTRY).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args([
                    "-c",
                    "user.name=Wax Test",
                    "-c",
                    "user.email=wax@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&publisher.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-m", "Registry"]);
        git(&["tag", "v1"]);
        let commit = git(&["rev-parse", "HEAD"]);
        (publisher, commit)
    }

    #[test]
    fn new_git_source_adds_a_registry_pin() {
        let repo = TestDir::new();
        let (publisher, commit) = git_registry();
        let remote = publisher.0.to_str().unwrap();
        let waxrc = config(json!({
            "compose": { "registry": { "git": remote, "tag": "v1" } }
        }));

        let plan = prepare_scan(&repo.0, waxrc, empty_lock(), &ScanSelection::default()).unwrap();

        assert!(plan.lockfile_changed);
        let language_id = LanguageId::try_from("compose").unwrap();
        let pin = &plan.lockfile.registries[&language_id];
        assert_eq!(pin.git.as_deref(), Some(remote));
        assert_eq!(pin.tag.as_deref(), Some("v1"));
        assert_eq!(pin.commit.as_deref(), Some(commit.as_str()));
        assert!(!repo.0.join(".wax/wax.lock.json").exists());
    }

    #[test]
    fn registry_digest_drift_reports_the_change() {
        let repo = TestDir::with_registry();
        let waxrc = config(json!({ "compose": {} }));
        let old_digest = "0".repeat(64);

        let Err(EngineError::RegistryLock {
            language_id,
            reason,
        }) = prepare_scan(
            &repo.0,
            waxrc,
            compose_lock(&old_digest),
            &ScanSelection::default(),
        )
        else {
            panic!("expected a registry lock error");
        };
        assert_eq!(language_id.as_str(), "compose");
        assert_eq!(
            reason,
            format!("digest changed from {old_digest} to {REGISTRY_SHA256}")
        );
    }

    #[test]
    fn zero_concurrency_override_uses_one_worker() {
        let repo = TestDir::with_registry();
        let mut waxrc = config(json!({ "compose": {} }));
        waxrc.engine.scan_concurrency = 8;
        let selection = ScanSelection {
            scan_concurrency: Some(0),
            ..ScanSelection::default()
        };

        let plan = prepare_scan(&repo.0, waxrc, compose_lock(REGISTRY_SHA256), &selection).unwrap();

        assert_eq!(plan.concurrency, 1);
    }

    #[test]
    fn ephemeral_scan_does_not_persist_new_git_pin() {
        let repo = TestDir::new();
        let home = TestDir::new();
        let (publisher, _) = git_registry();
        let waxrc = config(json!({
            "compose": { "registry": { "git": publisher.0, "tag": "v1" } }
        }));
        let lockfile = empty_lock();
        let on_disk_lockfile = serde_json::to_vec(&lockfile).unwrap();
        fs::create_dir(repo.0.join(".wax")).unwrap();
        let lockfile_path = repo.0.join(".wax/wax.lock.json");
        fs::write(&lockfile_path, &on_disk_lockfile).unwrap();

        let result = Engine::scan_repo_with_options(
            &repo.0,
            ScanOptions {
                wax_home: Some(WaxHome::new(home.0.clone())),
                ephemeral: Some(EphemeralScanConfig { waxrc, lockfile }),
                ..ScanOptions::default()
            },
        );

        // A missing pack lock stops execution after the new registry pin has been prepared.
        assert!(
            matches!(result, Err(EngineError::AutoInstallPolicyBlocked { .. })),
            "{result:?}"
        );
        assert_eq!(fs::read(lockfile_path).unwrap(), on_disk_lockfile);
    }
}
