use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::LanguageId;
use wax_contract::RegistryLookupErrorCode;
use wax_contract::RegistrySnapshotRef;
use wax_core::digest::sha256_hex;
use wax_core::global_state::{GlobalState, RememberedDesignSystem, save_global_state};
use wax_core::paths::WaxHome;
use wax_core::registry_cache::{RegistryCache, RegistrySourceSpec};
use wax_core::registry_lookup::{RegistryLookupOptions, resolve_lookup_snapshots};
use wax_core::registry_memory::{add_remembered_registry, remember_design_system};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-registry-lookup-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn language(value: &str) -> LanguageId {
    LanguageId::try_from(value).expect("valid language")
}

fn write_app(repo: &Path, registry: &[u8]) {
    fs::create_dir_all(repo.join(".wax")).expect("create wax directory");
    fs::write(repo.join(".wax/compose.registry.json"), registry).expect("write registry");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "compose": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/compose.registry.json",
        "upstream": "acme/compose"
      }
    }
  }
}"#,
    )
    .expect("write config");
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {{
    "compose": {{
      "source": ".wax/compose.registry.json",
      "sha256": "{}"
    }}
  }},
  "languages": {{}}
}}"#,
            sha256_hex(registry)
        ),
    )
    .expect("write lockfile");
}

#[test]
fn explicit_app_id_keeps_app_lock() {
    let root = TestDir::new("app-lock");
    let repo = root.0.join("app");
    let state_path = root.0.join("wax-home/state.json");
    let locked = br#"{"schema_version":1,"language":"compose","components":[{"id":"locked","symbol":"Locked"}]}"#;
    let newer_path = root.0.join("newer-compose.json");
    fs::write(
        &newer_path,
        br#"{"schema_version":1,"language":"compose","components":[{"id":"newer","symbol":"Newer"}]}"#,
    )
    .expect("write newer registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: newer_path,
            language: None,
        },
    )
    .expect("remember newer registry");
    write_app(&repo, locked);

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(state_path),
    })
    .expect("resolve locked app registry");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(locked));
}

#[test]
fn remembered_source_works_without_publisher_checkout() {
    let root = TestDir::new("remembered-source");
    let source_path = root.0.join("remote-compose.json");
    let state_path = root.0.join("wax-home/state.json");
    let bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"remote","symbol":"Remote"}]}"#;
    fs::write(&source_path, bytes).expect("write source registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: source_path.clone(),
            language: None,
        },
    )
    .expect("remember registry source");
    fs::remove_file(source_path).expect("remove original source");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("standalone"),
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("resolve remembered cached registry");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(bytes));
}

#[test]
fn locked_snapshot_must_match_digest_and_source() {
    let root = TestDir::new("lock-mismatch");
    let repo = root.0.join("app");
    let state_path = root.0.join("wax-home/state.json");
    let locked = br#"{"schema_version":1,"language":"compose","components":[{"id":"locked","symbol":"Locked"}]}"#;
    write_app(&repo, locked);
    fs::write(
        repo.join(".wax/compose.registry.json"),
        br#"{"schema_version":1,"language":"compose","components":[{"id":"newer","symbol":"Newer"}]}"#,
    )
    .expect("replace registry bytes");

    let error = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(state_path),
    })
    .expect_err("changed locked bytes must fail");

    assert_eq!(error.code, RegistryLookupErrorCode::SnapshotMismatch);
}

#[test]
fn standalone_source_uses_language_identity_from_json() {
    let root = TestDir::new("standalone");
    let source = root.0.join("registry.json");
    fs::write(
        &source,
        br#"{"schema_version":1,"language":"compose","components":[{"id":"button","symbol":"Button"}]}"#,
    )
    .expect("write registry");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("working-directory"),
        selector: Some(source.display().to_string()),
        language: None,
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect("resolve standalone source");

    assert_eq!(snapshots[0].reference.language, language("compose"));
}

#[test]
fn standalone_source_without_language_identity_fails() {
    let root = TestDir::new("missing-language");
    let source = root.0.join("registry.json");
    fs::write(
        &source,
        br#"{"schema_version":1,"components":[{"id":"button","symbol":"Button"}]}"#,
    )
    .expect("write registry");

    let error = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("working-directory"),
        selector: Some(source.display().to_string()),
        language: None,
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect_err("missing language identity must fail");

    assert_eq!(error.code, RegistryLookupErrorCode::InvalidArgument);
}

#[test]
fn publisher_config_is_default_outside_app_context() {
    let root = TestDir::new("publisher-default");
    let repo = root.0.join("publisher");
    fs::create_dir_all(repo.join(".wax/registries")).expect("create registries");
    fs::write(
        repo.join(".wax/registries/compose.json"),
        br#"{"schema_version":1,"language":"compose","components":[{"id":"publisher","symbol":"Publisher"}]}"#,
    )
    .expect("write registry");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "compose": {"source": ".wax/registries/compose.json"}
      }
    }
  }
}"#,
    )
    .expect("write config");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: None,
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect("resolve publisher default");

    assert_eq!(snapshots[0].reference.system_id.as_deref(), Some("acme"));
}

#[test]
fn mixed_publisher_and_app_prefers_app_lock_without_selector() {
    let root = TestDir::new("mixed-app-publisher");
    let repo = root.0.join("mixed");
    let locked =
        br#"{"schema_version":1,"language":"compose","components":[{"id":"app","symbol":"App"}]}"#;
    write_app(&repo, locked);
    fs::create_dir_all(repo.join(".wax/registries")).expect("create publisher registries");
    fs::write(
        repo.join(".wax/registries/compose.json"),
        br#"{"schema_version":1,"language":"compose","components":[{"id":"publisher","symbol":"Publisher"}]}"#,
    )
    .expect("write publisher registry");
    let mut config = fs::read_to_string(repo.join(".wax/wax.config.json")).expect("read config");
    config = config.replacen(
        r#""languages""#,
        r#""design_systems": {
    "acme-pub": {
      "name": "Acme Pub",
      "registries": {
        "compose": {"source": ".wax/registries/compose.json"}
      }
    }
  },
  "languages""#,
        1,
    );
    fs::write(repo.join(".wax/wax.config.json"), config).expect("write mixed config");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: None,
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect("prefer app lock");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(locked));
}

#[test]
fn publisher_resolves_multiple_languages_without_filter() {
    let root = TestDir::new("multi-lang");
    let repo = root.0.join("publisher");
    fs::create_dir_all(repo.join(".wax/registries")).expect("create registries");
    fs::write(
        repo.join(".wax/registries/compose.json"),
        br#"{"schema_version":1,"language":"compose","components":[{"id":"c","symbol":"C"}]}"#,
    )
    .expect("write compose registry");
    fs::write(
        repo.join(".wax/registries/react.json"),
        br#"{"schema_version":1,"language":"react","components":[{"id":"r","symbol":"R"}]}"#,
    )
    .expect("write react registry");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "compose": {"source": ".wax/registries/compose.json"},
        "react": {"source": ".wax/registries/react.json"}
      }
    }
  }
}"#,
    )
    .expect("write config");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: None,
        language: None,
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect("resolve all publisher languages");

    let mut languages: Vec<_> = snapshots
        .iter()
        .map(|snapshot| snapshot.reference.language.as_str().to_owned())
        .collect();
    languages.sort();
    assert_eq!(languages, vec!["compose".to_owned(), "react".to_owned()]);
}

#[test]
fn outside_context_requires_selector() {
    let root = TestDir::new("no-selector");
    let error = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("standalone"),
        selector: None,
        language: None,
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect_err("standalone lookup needs a selector");

    assert_eq!(error.code, RegistryLookupErrorCode::InvalidArgument);
}

#[test]
fn remembered_selector_uses_overridden_state_file() {
    let root = TestDir::new("overridden-state");
    let home = WaxHome::new(root.0.join("wax-home"));
    let state_path = home.root().join("alternate-state.json");
    let source_path = root.0.join("remote-compose.json");
    let bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"remote","symbol":"Remote"}]}"#;
    fs::write(&source_path, bytes).expect("write source registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: source_path,
            language: None,
        },
    )
    .expect("remember registry source");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("standalone"),
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("resolve remembered id via overridden state file");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(bytes));
}

#[test]
fn remembered_id_wins_over_colliding_publisher_system_id() {
    let root = TestDir::new("remembered-over-publisher");
    let repo = root.0.join("publisher");
    fs::create_dir_all(repo.join(".wax/registries")).expect("create registries");
    let publisher_bytes =
        br#"{"schema_version":1,"language":"compose","components":[{"id":"publisher","symbol":"Publisher"}]}"#;
    fs::write(repo.join(".wax/registries/compose.json"), publisher_bytes)
        .expect("write publisher registry");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "compose": {"source": ".wax/registries/compose.json"}
      }
    }
  }
}"#,
    )
    .expect("write publisher config");
    let wax_home = root.0.join("wax-home");
    let state_path = wax_home.join("state.json");
    let remote_path = root.0.join("remote-compose.json");
    let remembered_bytes =
        br#"{"schema_version":1,"language":"compose","components":[{"id":"remembered","symbol":"Remembered"}]}"#;
    fs::write(&remote_path, remembered_bytes).expect("write remembered registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Remote",
        &RegistrySourceSpec::LocalFile {
            path: remote_path,
            language: None,
        },
    )
    .expect("remember remote");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("remembered id should win over publisher id");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(remembered_bytes));
}

#[test]
fn remembered_local_publisher_uses_published_source() {
    let root = TestDir::new("remembered-published-source");
    let repo = root.0.join("publisher");
    fs::create_dir_all(repo.join(".wax/registries")).expect("create registries");
    let authoring = br#"{"schema_version":1,"language":"compose","components":[{"id":"authoring","symbol":"Authoring"}]}"#;
    let published = br#"{"schema_version":1,"language":"compose","components":[{"id":"published","symbol":"Published"}]}"#;
    fs::write(repo.join(".wax/registries/compose.json"), authoring)
        .expect("write authoring registry");
    let published_path = root.0.join("published-compose.json");
    fs::write(&published_path, published).expect("write published registry");
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{}},
  "design_systems": {{
    "acme": {{
      "name": "Acme",
      "registries": {{
        "compose": {{"source": ".wax/registries/compose.json", "published_source": "file://{}"}}
      }}
    }}
  }}
}}"#,
            published_path.display()
        ),
    )
    .expect("write publisher config");
    let state_path = root.0.join("wax-home/state.json");
    remember_design_system(&state_path, "acme", "Acme", &repo).expect("remember publisher");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("standalone"),
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: false,
        state_path: Some(state_path),
    })
    .expect("resolve remembered publisher");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(published));
}

#[test]
fn conflicting_tag_with_app_lock_is_rejected() {
    let root = TestDir::new("tag-conflict");
    let repo = root.0.join("app");
    let locked = br#"{"schema_version":1,"language":"compose","components":[{"id":"locked","symbol":"Locked"}]}"#;
    write_app(&repo, locked);
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "compose": {
      "roots": ["src"],
      "registry": {
        "git": "https://example.com/acme.git",
        "tag": "v1.0.0"
      }
    }
  }
}"#,
    )
    .expect("write git config");
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {{
    "compose": {{
      "source": "git:https://example.com/acme.git#v1.0.0",
      "sha256": "{}",
      "git": "https://example.com/acme.git",
      "tag": "v1.0.0",
      "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }}
  }},
  "languages": {{}}
}}"#,
            sha256_hex(locked)
        ),
    )
    .expect("write lockfile");

    let error = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: None,
        language: Some(language("compose")),
        tag: Some("v2.0.0".to_owned()),
        snapshot: None,
        offline: false,
        state_path: Some(root.0.join("wax-home/state.json")),
    })
    .expect_err("conflicting tag must fail");

    assert_eq!(error.code, RegistryLookupErrorCode::SnapshotMismatch);
}

#[test]
fn git_app_lock_matches_remembered_id_selector() {
    let root = TestDir::new("git-app-lock-selector");
    let repo = root.0.join("app");
    let state_path = root.0.join("wax-home/state.json");
    let locked = br#"{"schema_version":1,"language":"compose","components":[{"id":"locked","symbol":"Locked"}]}"#;
    let newer = br#"{"schema_version":1,"language":"compose","components":[{"id":"newer","symbol":"Newer"}]}"#;
    let git_url = "https://example.com/acme.git";
    let locked_digest = sha256_hex(locked);
    let newer_digest = sha256_hex(newer);

    fs::create_dir_all(repo.join(".wax")).expect("create wax directory");
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{
    "compose": {{
      "roots": ["src"],
      "registry": {{
        "git": "{git_url}",
        "tag": "v1.0.0"
      }}
    }}
  }}
}}"#
        ),
    )
    .expect("write git config");
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {{
    "compose": {{
      "source": "git:{git_url}#v1.0.0",
      "sha256": "{locked_digest}",
      "git": "{git_url}",
      "tag": "v1.0.0",
      "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }}
  }},
  "languages": {{}}
}}"#
        ),
    )
    .expect("write lockfile");

    let cache_root = root.0.join("wax-home/cache/registries");
    fs::create_dir_all(cache_root.join("blobs")).expect("blob directory");
    fs::write(
        cache_root
            .join("blobs")
            .join(format!("{locked_digest}.json")),
        locked,
    )
    .expect("seed locked blob");
    fs::write(
        cache_root
            .join("blobs")
            .join(format!("{newer_digest}.json")),
        newer,
    )
    .expect("seed newer blob");

    let mut state = GlobalState::default();
    state.design_systems.insert(
        "acme".to_owned(),
        RememberedDesignSystem {
            bundle_pin: None,
            name: "Acme Design System".to_owned(),
            repo_root: PathBuf::new(),
            last_seen_config: PathBuf::new(),
            source: Some(RegistrySourceSpec::Git {
                subdir: None,
                url: git_url.to_owned(),
                tag: Some("v2.0.0".to_owned()),
                language: language("compose"),
            }),
            snapshots: vec![RegistrySnapshotRef {
                source: format!("git:{git_url}#v2.0.0"),
                language: language("compose"),
                digest: newer_digest,
                system_id: None,
                git_commit: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()),
                extra: serde_json::Map::new(),
            }],
        },
    );
    fs::create_dir_all(state_path.parent().expect("state parent")).expect("state parent");
    save_global_state(&state_path, &state).expect("save remembered git remote");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("git app lock must match remembered id selector");

    assert_eq!(snapshots[0].reference.digest, locked_digest);
}

#[test]
fn app_git_system_lookup_falls_back_to_existing_global_snapshot_offline() {
    let root = TestDir::new("app-git-system-global-cache");
    let repo = root.0.join("app");
    let state_path = root.0.join("wax-home/state.json");
    let remote_dir = root.0.join("remote.git");
    fs::create_dir_all(&remote_dir).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Wax Test",
                "-c",
                "user.email=test@example.com",
            ])
            .args(args)
            .current_dir(&remote_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-qb", "main"]);
    fs::create_dir_all(remote_dir.join("systems/acme/1/.wax/registries")).unwrap();
    fs::write(remote_dir.join("systems/acme/1/.wax/registries/compose.json"), br#"{"schema_version":1,"language":"compose","components":[{"id":"button","symbol":"Button"}]}"#).unwrap();
    fs::write(remote_dir.join("systems/acme/1/.wax/wax.config.json"), br#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme","registries":{"compose":{"source":".wax/registries/compose.json"}}}}}"#).unwrap();
    fs::write(remote_dir.join("systems/acme/1/provenance.json"), b"{}").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "bundle"]);
    let commit = git(&["rev-parse", "HEAD"]);
    git(&["tag", "v1.0.0"]);
    let remote = remote_dir.display().to_string();
    let bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"button","symbol":"Button"}]}"#;
    let digest = sha256_hex(bytes);
    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(r#"{{"schema_version":2,"languages":{{"compose":{{"roots":["src"],"registry":{{"git":"{remote}","tag":"v1.0.0","subdir":"systems/acme/1","system":"acme"}}}}}}}}"#),
    )
    .unwrap();
    let definition_digest =
        sha256_hex(&fs::read(remote_dir.join("systems/acme/1/.wax/wax.config.json")).unwrap());
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(r#"{{"schema_version":3,"engine_api_version":1,"wax_version":"0.1.0","registries":{{"compose":{{"source":"{}","sha256":"{digest}","git":"{remote}","tag":"v1.0.0","commit":"{commit}","subdir":"systems/acme/1","definition":{{"system_id":"acme","path":"systems/acme/1/.wax/wax.config.json","sha256":"{definition_digest}"}}}}}},"languages":{{}}}}"#, wax_core::registry_git::git_source_identity(&remote, "v1.0.0", Some("systems/acme/1"), Some("acme"))),
    )
    .unwrap();

    let global_cache = RegistryCache::open(root.0.join("wax-home/cache/registries"));
    let snapshots = global_cache
        .resolve(
            &RegistrySourceSpec::GitSystem {
                url: remote,
                tag: "v1.0.0".to_owned(),
                subdir: "systems/acme/1".to_owned(),
                system_id: "acme".to_owned(),
            },
            &Default::default(),
        )
        .expect("seed global cache snapshot");
    assert_eq!(snapshots[0].reference.digest, digest);

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: repo,
        selector: None,
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("fall back to exact global Git system snapshot");

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].reference.digest, digest);
}

#[test]
fn remembered_selector_pins_stored_snapshots_not_newer_cache_selection() {
    let root = TestDir::new("remembered-pin");
    let state_path = root.0.join("wax-home/state.json");
    let source_path = root.0.join("remote-compose.json");
    let original =
        br#"{"schema_version":1,"language":"compose","components":[{"id":"original","symbol":"Original"}]}"#;
    let newer =
        br#"{"schema_version":1,"language":"compose","components":[{"id":"newer","symbol":"Newer"}]}"#;
    fs::write(&source_path, original).expect("write original registry");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: source_path.clone(),
            language: None,
        },
    )
    .expect("remember original registry");
    fs::write(&source_path, newer).expect("update registry bytes");
    let cache = RegistryCache::open(root.0.join("wax-home/cache/registries"));
    cache
        .refresh(&RegistrySourceSpec::LocalFile {
            path: source_path,
            language: None,
        })
        .expect("advance cache selection without refreshing remembered snapshots");

    let snapshots = resolve_lookup_snapshots(&RegistryLookupOptions {
        repo_root: root.0.join("standalone"),
        selector: Some("acme".to_owned()),
        language: Some(language("compose")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    })
    .expect("remembered id should reuse stored snapshots");

    assert_eq!(snapshots[0].reference.digest, sha256_hex(original));
}

#[test]
fn explicit_relative_files_are_scoped_to_each_requesting_repo() {
    let root = TestDir::new("relative-repos");
    let state_path = root.0.join("home/state.json");
    let alpha = root.0.join("alpha");
    let beta = root.0.join("beta");
    let alpha_bytes = br#"{"schema_version":1,"language":"react","components":[{"id":"alpha","symbol":"Alpha"}]}"#;
    let beta_bytes =
        br#"{"schema_version":1,"language":"react","components":[{"id":"beta","symbol":"Beta"}]}"#;
    for (repo, bytes) in [
        (&alpha, alpha_bytes.as_slice()),
        (&beta, beta_bytes.as_slice()),
    ] {
        fs::create_dir_all(repo).unwrap();
        fs::write(repo.join("react.json"), bytes).unwrap();
    }
    let lookup = |repo: &Path, selector: String| {
        resolve_lookup_snapshots(&RegistryLookupOptions {
            repo_root: repo.to_owned(),
            selector: Some(selector),
            language: Some(language("react")),
            tag: None,
            snapshot: None,
            offline: false,
            state_path: Some(state_path.clone()),
        })
        .expect("resolve registry")
    };
    let first = lookup(&alpha, "./react.json".to_owned());
    let second = lookup(&beta, "./react.json".to_owned());
    let absolute = lookup(&beta, beta.join("react.json").display().to_string());
    assert_eq!(first[0].reference.digest, sha256_hex(alpha_bytes));
    assert_eq!(second[0].reference.digest, sha256_hex(beta_bytes));
    assert_eq!(second[0].reference, absolute[0].reference);
}

#[test]
fn scoped_offline_selection_and_pins_do_not_reuse_legacy_relative_records() {
    use wax_core::registry_cache::RegistryReadPolicy;
    let root = TestDir::new("scoped-offline");
    let state_path = root.0.join("home/state.json");
    let repo = root.0.join("app");
    fs::create_dir_all(&repo).unwrap();
    let bytes =
        br#"{"schema_version":1,"language":"react","components":[{"id":"app","symbol":"App"}]}"#;
    fs::write(repo.join("react.json"), bytes).unwrap();
    // Seed an old unscoped record through the original public parser/cache API.
    let legacy_path = PathBuf::from(format!("./wax-legacy-{}.json", std::process::id()));
    fs::write(
        &legacy_path,
        br#"{"schema_version":1,"language":"react","components":[]}"#,
    )
    .unwrap();
    let cache = RegistryCache::open(root.0.join("home/cache/registries"));
    let legacy =
        RegistrySourceSpec::parse(legacy_path.to_str().unwrap(), Some(language("react"))).unwrap();
    cache
        .resolve(&legacy, &RegistryReadPolicy::default())
        .unwrap();
    fs::remove_file(&legacy_path).unwrap();
    let mut options = RegistryLookupOptions {
        repo_root: repo.clone(),
        selector: Some(legacy_path.to_string_lossy().into_owned()),
        language: Some(language("react")),
        tag: None,
        snapshot: None,
        offline: true,
        state_path: Some(state_path),
    };
    let error = resolve_lookup_snapshots(&options)
        .expect_err("legacy selection must not satisfy scoped lookup");
    assert_eq!(error.code, RegistryLookupErrorCode::OfflineCacheMiss);
    options.selector = Some("./react.json".into());
    assert_eq!(
        resolve_lookup_snapshots(&options).unwrap_err().code,
        RegistryLookupErrorCode::OfflineCacheMiss
    );
    options.offline = false;
    let selected = resolve_lookup_snapshots(&options).unwrap().remove(0);
    fs::remove_file(repo.join("react.json")).unwrap();
    options.offline = true;
    assert_eq!(
        resolve_lookup_snapshots(&options).unwrap()[0].reference,
        selected.reference
    );
    options.snapshot = Some(selected.reference);
    assert_eq!(
        resolve_lookup_snapshots(&options).unwrap()[0]
            .reference
            .digest,
        sha256_hex(bytes)
    );
    assert!(
        cache
            .resolve(
                &legacy,
                &RegistryReadPolicy {
                    offline: true,
                    expected_snapshot: None
                }
            )
            .is_ok()
    );
}

#[test]
fn relative_publisher_directories_are_scoped_to_each_repo() {
    let root = TestDir::new("relative-publishers");
    let state_path = root.0.join("home/state.json");
    for name in ["alpha", "beta"] {
        let publisher = root.0.join(name).join("publisher");
        fs::create_dir_all(publisher.join(".wax/registries")).unwrap();
        fs::write(publisher.join(".wax/registries/react.json"), format!(r#"{{"schema_version":1,"language":"react","components":[{{"id":"{name}","symbol":"Button"}}]}}"#)).unwrap();
    }
    let lookup = |name: &str| {
        resolve_lookup_snapshots(&RegistryLookupOptions {
            repo_root: root.0.join(name),
            selector: Some("./publisher".into()),
            language: Some(language("react")),
            tag: None,
            snapshot: None,
            offline: false,
            state_path: Some(state_path.clone()),
        })
        .unwrap()
    };
    assert_ne!(
        lookup("alpha")[0].reference.digest,
        lookup("beta")[0].reference.digest
    );
}
