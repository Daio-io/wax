use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_contract::LanguageId;
use wax_core::config::lockfile::{
    LockfileError, WaxLockLanguageReport, check_waxrc_lockfile_languages, load_lockfile,
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
            max_supported: 2
        }
    ));
    assert!(
        err.to_string()
            .contains("unsupported wax.lock.json schema_version 999 in")
    );
    assert!(
        err.to_string()
            .contains("unsupported-schema.wax.lock.json; this engine supports 1 through 2")
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
            max_supported: 2
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
            max_supported: 2
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
