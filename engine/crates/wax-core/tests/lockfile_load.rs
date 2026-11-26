use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_contract::LanguageId;
use wax_core::config::lockfile::{
    LockfileError, WAX_LOCK_SCHEMA_VERSION, WaxLockLanguageReport, check_waxrc_lockfile_languages,
    load_lockfile, save_lockfile,
};
use wax_core::config::waxrc::{LanguageEntry, WaxRc};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/config")
        .join(name)
}

struct TestFile {
    path: PathBuf,
}

impl TestFile {
    fn new(name: &str, contents: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "wax-core-lockfile-load-{}-{name}-{nonce}.json",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn language_entry(id: &str) -> LanguageEntry {
    LanguageEntry {
        id: LanguageId::try_from(id).unwrap(),
        roots: Vec::new(),
        root_groups: Default::default(),
        registry_source: None,
        extra: serde_json::Map::new(),
    }
}

#[test]
fn loads_minimal_lockfile() {
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();

    assert_eq!(lock.schema_version, 2);
    assert_eq!(lock.engine_api_version, 1);
    assert_eq!(lock.wax_version, "0.1.0-alpha.1");
    assert_eq!(
        lock.locked_at.unwrap(),
        time::macros::datetime!(2026-05-16 12:00 UTC)
    );
    let registry = lock.registries.get("compose").unwrap();
    assert_eq!(registry.source, ".wax/compose.registry.json");
    assert_eq!(
        registry.sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );

    let compose = lock.languages.get("compose").unwrap();
    assert_eq!(compose.version, "0.4.2");
    assert_eq!(compose.api_version, 1);
    assert_eq!(compose.source, "https://packs.wax.dev/index.json");
    assert_eq!(compose.resolved.target, "aarch64-apple-darwin");
    assert_eq!(
        compose.resolved.url,
        "https://releases.wax.dev/compose/0.4.2/aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(
        compose.resolved.sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn saves_legacy_lockfile_in_current_format() {
    let fixture = fixture_path("git-registry.wax.lock.json");
    let lock = load_lockfile(&fixture).unwrap();
    let original = fs::read_to_string(&fixture).unwrap().replace("\r\n", "\n");
    let destination = TestFile::new("save-current-format", "");

    save_lockfile(destination.path(), &lock).unwrap();

    let contents = fs::read_to_string(destination.path()).unwrap();
    assert!(contents.ends_with('\n'));
    assert_eq!(
        contents,
        original.replacen(
            "\"schema_version\": 2",
            &format!("\"schema_version\": {WAX_LOCK_SCHEMA_VERSION}"),
            1,
        )
    );
    let mut expected = lock;
    expected.schema_version = WAX_LOCK_SCHEMA_VERSION;
    assert_eq!(load_lockfile(destination.path()).unwrap(), expected);
}

#[test]
fn save_rejects_invalid_bundle_lock_without_replacing_file() {
    let original = fs::read_to_string(fixture_path("git-registry.wax.lock.json")).unwrap();
    let destination = TestFile::new("invalid-bundle-save", &original);
    let mut lock = load_lockfile(destination.path()).unwrap();
    lock.registries.get_mut("compose").unwrap().subdir = Some("../outside".to_owned());

    let result = save_lockfile(destination.path(), &lock);

    assert!(matches!(
        result,
        Err(LockfileError::InvalidBundleLock { .. })
    ));
    assert_eq!(fs::read_to_string(destination.path()).unwrap(), original);
}

#[test]
fn lockfile_reserves_null_signature_slot() {
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();

    let compose = lock.languages.get("compose").unwrap();
    assert!(compose.resolved.signature.is_none());
}

#[test]
fn lockfile_rejects_unsupported_schema_version() {
    let err = load_lockfile(fixture_path("unsupported-schema.wax.lock.json")).unwrap_err();

    assert!(matches!(
        err,
        LockfileError::UnsupportedSchemaVersion {
            path: _,
            found: 999,
            min_supported: 1,
            max_supported: 3
        }
    ));
    assert!(
        err.to_string()
            .contains("unsupported wax.lock.json schema_version 999 in")
    );
    assert!(
        err.to_string()
            .contains("unsupported-schema.wax.lock.json; this engine supports 1 through 3")
    );
}

#[test]
fn lockfile_rejects_unsupported_schema_version_before_v1_shape() {
    let err = load_lockfile(fixture_path(
        "unsupported-schema-missing-v1-fields.wax.lock.json",
    ))
    .unwrap_err();

    assert!(matches!(
        err,
        LockfileError::UnsupportedSchemaVersion {
            path: _,
            found: 999,
            min_supported: 1,
            max_supported: 3
        }
    ));
}

#[test]
fn lockfile_rejects_schema_version_below_supported_range() {
    let lockfile = TestFile::new(
        "schema-zero",
        r#"{"schema_version":0,"engine_api_version":1,"wax_version":"0.0.0","locked_at":null,"languages":{}}"#,
    );
    let err = load_lockfile(lockfile.path()).unwrap_err();

    assert!(matches!(
        err,
        LockfileError::UnsupportedSchemaVersion {
            path: _,
            found: 0,
            min_supported: 1,
            max_supported: 3
        }
    ));
}

#[test]
fn parses_registry_locks() {
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();
    let registry = lock
        .registries
        .get(&LanguageId::try_from("compose").unwrap())
        .expect("compose registry lock should exist");

    assert_eq!(registry.source, ".wax/compose.registry.json");
    assert_eq!(
        registry.sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert!(registry.git.is_none());
}

#[test]
fn loads_git_registry_lockfile_fixture() {
    let lock = load_lockfile(fixture_path("git-registry.wax.lock.json")).unwrap();

    assert_eq!(lock.schema_version, 2);
    let registry = lock
        .registries
        .get(&LanguageId::try_from("compose").unwrap())
        .expect("compose registry lock should exist");
    assert_eq!(registry.source, "git:https://example.com/design.git#v1");
    assert_eq!(
        registry.sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        registry.git.as_deref(),
        Some("https://example.com/design.git")
    );
    assert_eq!(registry.tag.as_deref(), Some("v1"));
    assert_eq!(
        registry.commit.as_deref(),
        Some("0123456789012345678901234567890123456789")
    );
}

#[test]
fn git_registry_lock_round_trips_all_metadata() {
    let lock = load_lockfile(fixture_path("git-registry.wax.lock.json")).unwrap();
    let registry = &lock.registries[&LanguageId::try_from("compose").unwrap()];
    assert_eq!(
        registry.git.as_deref(),
        Some("https://example.com/design.git")
    );
    assert_eq!(registry.tag.as_deref(), Some("v1"));
    assert_eq!(
        registry.commit.as_deref(),
        Some("0123456789012345678901234567890123456789")
    );
    let encoded = serde_json::to_value(&lock).unwrap();
    assert_eq!(
        encoded["registries"]["compose"]["commit"],
        "0123456789012345678901234567890123456789"
    );
    assert_eq!(
        encoded["registries"]["compose"]["git"],
        "https://example.com/design.git"
    );
    assert_eq!(encoded["registries"]["compose"]["tag"], "v1");
}

#[test]
fn partial_git_registry_lock_is_rejected() {
    let lockfile = TestFile::new(
        "partial-git-lock",
        r#"{"schema_version":2,"engine_api_version":1,"wax_version":"0.1.0","locked_at":null,"registries":{"compose":{"source":"git:https://example.com/design.git#v1","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","git":"https://example.com/design.git"}},"languages":{}}"#,
    );
    let err = load_lockfile(lockfile.path()).unwrap_err();
    assert!(err.to_string().contains("partial git metadata"));
}

#[test]
fn parses_v1_lockfile_without_registry_locks_for_migration() {
    let lock = load_lockfile(fixture_path("minimal-v1-no-registries.wax.lock.json")).unwrap();

    assert_eq!(lock.schema_version, 1);
    assert!(lock.registries.is_empty());
}

#[test]
fn schema_v2_requires_explicit_registries_field() {
    let lockfile = TestFile::new(
        "schema-v2-missing-registries",
        r#"{"schema_version":2,"engine_api_version":1,"wax_version":"0.1.0-alpha.1","locked_at":"2026-05-16T12:00:00Z","languages":{"compose":{"version":"0.4.2","api_version":1,"source":"https://packs.wax.dev/index.json","resolved":{"target":"aarch64-apple-darwin","url":"https://releases.wax.dev/compose/0.4.2/aarch64-apple-darwin.tar.gz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","signature":null}}}}"#,
    );
    let err = load_lockfile(lockfile.path()).unwrap_err();

    assert!(matches!(err, LockfileError::InvalidConfig { .. }));
}

#[test]
fn lockfile_distinguishes_malformed_json_from_invalid_config() {
    let malformed = load_lockfile(fixture_path("malformed.wax.lock.json")).unwrap_err();
    let invalid_config =
        load_lockfile(fixture_path("missing-languages.wax.lock.json")).unwrap_err();

    assert!(matches!(malformed, LockfileError::MalformedJson { .. }));
    assert!(matches!(
        invalid_config,
        LockfileError::InvalidConfig { .. }
    ));
}

#[test]
fn lockfile_reports_missing_file_as_read_error() {
    let err = load_lockfile(fixture_path("does-not-exist.wax.lock.json")).unwrap_err();

    assert!(matches!(err, LockfileError::Read { .. }));
    assert!(err.to_string().contains("failed to read wax.lock.json"));
    assert!(err.to_string().contains("does-not-exist.wax.lock.json"));
}

#[test]
fn lockfile_doctor_reports_missing_enabled_languages() {
    let rc = WaxRc {
        schema_version: 2,
        engine: Default::default(),
        adoption: Default::default(),
        token_inference: Default::default(),
        languages: vec![language_entry("compose"), language_entry("react")],
        design_systems: Default::default(),
        outputs: Vec::new(),
    };
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();

    let report = check_waxrc_lockfile_languages(&rc, &lock);

    assert_eq!(
        report,
        WaxLockLanguageReport {
            missing_enabled_languages: [LanguageId::try_from("react").unwrap()].into(),
            stale_locked_languages: BTreeSet::new(),
        }
    );
}

#[test]
fn lockfile_doctor_reports_stale_entries_for_absent_languages() {
    let absent_rc = WaxRc {
        schema_version: 2,
        engine: Default::default(),
        adoption: Default::default(),
        token_inference: Default::default(),
        languages: Vec::new(),
        design_systems: Default::default(),
        outputs: Vec::new(),
    };
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();
    let absent_report = check_waxrc_lockfile_languages(&absent_rc, &lock);

    assert_eq!(
        absent_report,
        WaxLockLanguageReport {
            missing_enabled_languages: BTreeSet::new(),
            stale_locked_languages: [LanguageId::try_from("compose").unwrap()].into(),
        }
    );
}

#[test]
fn schema_three_roundtrips_nested_bundle_pins_and_rejects_partial_metadata() {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/config/nested-git.wax.lock.json"
    ))
    .unwrap();
    let file = TestFile::new("bundle-v3", &input.to_string());
    let lock = load_lockfile(file.path()).unwrap();
    assert_eq!(serde_json::to_value(lock).unwrap(), input);
    for field in ["subdir", "commit"] {
        let mut partial = input.clone();
        partial["registries"]["react"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        let file = TestFile::new("partial-bundle", &partial.to_string());
        assert!(
            load_lockfile(file.path()).is_err(),
            "accepted missing {field}"
        );
    }
}

#[test]
fn nested_lock_rejects_identity_that_disagrees_with_its_typed_pins() {
    let input = serde_json::json!({
        "schema_version": 3, "engine_api_version": 1, "wax_version": "0.1.0",
        "languages": {}, "registries": {"react": {
            "source": "git-system:4e6b915907fa2167ed3201ef4020c74e033fde42996fb0cf2243f4304f104c2c",
            "sha256": "a".repeat(64), "git": "https://example.com/ds.git", "tag": "v2",
            "commit": "b".repeat(40), "subdir": "systems/acme/1",
            "definition": {"system_id": "acme", "path": "systems/acme/1/.wax/wax.config.json", "sha256": "c".repeat(64)}
        }}
    });
    let file = TestFile::new("mismatched-bundle-identity", &input.to_string());
    assert!(load_lockfile(file.path()).is_err());
}

#[test]
fn nested_source_identity_requires_bundle_metadata() {
    let file = TestFile::new(
        "missing-bundle-metadata",
        &serde_json::json!({
            "schema_version": 3, "engine_api_version": 1, "wax_version": "0.1.0",
            "languages": {}, "registries": {"react": {
                "source": "git-system:incomplete", "sha256": "a".repeat(64)
            }}
        })
        .to_string(),
    );
    assert!(load_lockfile(file.path()).is_err());
}

#[test]
fn nested_languages_cannot_pin_different_definitions_via_different_tags() {
    let mut registries = serde_json::Map::new();
    for (language, tag, commit) in [("compose", "v1", "a"), ("react", "other", "b")] {
        registries.insert(language.into(), serde_json::json!({"source":wax_core::registry_git::git_source_identity("https://example.com/ds.git",tag,Some("systems/acme/1"),Some("acme")),"git":"https://example.com/ds.git","tag":tag,"commit":commit.repeat(40),"sha256":"c".repeat(64),"subdir":"systems/acme/1","definition":{"system_id":"acme","path":"systems/acme/1/.wax/wax.config.json","sha256":"d".repeat(64)}}));
    }
    let file = TestFile::new("mixed-bundle", &serde_json::json!({"schema_version":3,"engine_api_version":1,"wax_version":"0.1.0","languages":{},"registries":registries}).to_string());
    assert!(load_lockfile(file.path()).is_err());
}
