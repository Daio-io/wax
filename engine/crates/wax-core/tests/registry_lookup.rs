use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::LanguageId;
use wax_contract::RegistryLookupErrorCode;
use wax_core::digest::sha256_hex;
use wax_core::registry_cache::RegistrySourceSpec;
use wax_core::registry_lookup::{RegistryLookupOptions, resolve_lookup_snapshots};
use wax_core::registry_memory::add_remembered_registry;

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
