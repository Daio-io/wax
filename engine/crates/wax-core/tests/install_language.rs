//! Integration tests for installing and managing language packs through PackStore.

use std::fs;
use std::path::PathBuf;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use wax_contract::LanguageId;
#[cfg(unix)]
use wax_core::global_state::GlobalStateError;
use wax_core::install::InstallError;
use wax_core::pack_store::{PackArtifact, PackStore, PackStoreError};
use wax_core::paths::WaxHome;

struct TestHome {
    root: PathBuf,
}

impl TestHome {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wax-core-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp wax home");

        Self { root }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn gzip_tar(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let gz = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = tar::Builder::new(gz);
        for (path, body, mode) in entries {
            let header = tar_header_with_path(path, body.len(), *mode);
            tar.append(&header, *body).expect("tar append");
        }
        tar.finish().expect("tar finish");
    }
    buffer
}

/// Builds a tar header with an arbitrary path string, including unsafe `..` segments
/// that the tar crate's `set_path` helper rejects at archive creation time.
fn tar_header_with_path(path: &str, body_len: usize, mode: u32) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    let path_bytes = path.as_bytes();
    assert!(
        path_bytes.len() <= 100,
        "ustar name field supports at most 100 bytes"
    );
    let gnu = header.as_gnu_mut().expect("gnu header");
    gnu.name[..path_bytes.len()].copy_from_slice(path_bytes);
    header.set_size(body_len as u64);
    header.set_mode(mode);
    header.set_cksum();
    header
}

#[derive(Debug, Deserialize)]
struct ManifestJson {
    id: LanguageId,
    version: String,
    api_version: u32,
    command: Vec<String>,
    ecosystem: String,
    parser_name: String,
    parser_version: String,
    target: String,
    sha256: String,
}

#[test]
fn pack_store_records_fresh_install() {
    let home = TestHome::new("record-fresh");
    let (store, id, url, sha256) = pack_store_fixture(&home);

    let installed = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();

    let state = wax_core::global_state::load_global_state(home.root.join("state.json")).unwrap();
    assert_eq!(
        state.installed_languages[&id]["0.4.2"].install_dir,
        installed.install_dir
    );
    assert_eq!(
        installed.command,
        [
            installed
                .install_dir
                .join("wax-lang-compose")
                .display()
                .to_string(),
            "--stdio".to_owned(),
        ]
    );
}

#[test]
fn pack_store_records_reused_directory_without_fetching_the_artifact() {
    let home = TestHome::new("record-reused");
    let (store, id, url, sha256) = pack_store_fixture(&home);
    let artifact_path = home.root.join("store-pack.tgz");
    let state_path = home.root.join("state.json");
    let first = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();
    fs::remove_file(artifact_path).unwrap();
    fs::remove_file(&state_path).unwrap();

    let reused = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();

    let state = wax_core::global_state::load_global_state(state_path).unwrap();
    assert_eq!(
        state.installed_languages[&id]["0.4.2"].install_dir,
        first.install_dir
    );
    assert_eq!(reused.command, first.command);
    assert_eq!(
        fs::read(reused.install_dir.join("wax-lang-compose")).unwrap(),
        b"#!/bin/sh\n"
    );
}

#[cfg(unix)]
#[test]
fn pack_store_removes_fresh_install_when_state_write_fails() {
    let home = TestHome::new("fresh-write-failure");
    let (store, id, url, sha256) = pack_store_fixture(&home);
    fs::create_dir_all(home.root.join("langs")).unwrap();

    let Some(result) = with_unwritable_state(&home, || {
        store.install(&id, "0.4.2", pack_artifact(&url, &sha256))
    }) else {
        return;
    };

    assert!(matches!(
        result,
        Err(PackStoreError::GlobalState(GlobalStateError::AtomicWrite(
            _
        )))
    ));
    assert!(!home.root.join("langs/compose/0.4.2").exists());
}

#[cfg(unix)]
#[test]
fn pack_store_preserves_reused_install_when_state_write_fails() {
    let home = TestHome::new("reused-write-failure");
    let (store, id, url, sha256) = pack_store_fixture(&home);
    let installed = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();
    fs::remove_file(home.root.join("state.json")).unwrap();

    let Some(result) = with_unwritable_state(&home, || {
        store.install(&id, "0.4.2", pack_artifact(&url, &sha256))
    }) else {
        return;
    };

    assert!(matches!(
        result,
        Err(PackStoreError::GlobalState(GlobalStateError::AtomicWrite(
            _
        )))
    ));
    assert!(installed.install_dir.join("wax-lang-compose").is_file());
}

#[test]
fn pack_store_lookup_rejects_manifest_identity_mismatches() {
    let home = TestHome::new("manifest-mismatch");
    let (store, id, url, sha256) = pack_store_fixture(&home);
    let installed = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();
    let found = store.installed(&id, "0.4.2").unwrap();
    assert_eq!(found.command, installed.command);
    let manifest_path = installed.install_dir.join("manifest.json");
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    for (field, value) in [("id", "react"), ("version", "0.4.1")] {
        let mut manifest = original.clone();
        manifest[field] = value.into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert!(matches!(
            store.installed(&id, "0.4.2"),
            Err(PackStoreError::ManifestMismatch { .. })
        ));
    }
}

#[test]
fn pack_store_removes_only_requested_versions() {
    let home = TestHome::new("remove");
    let (store, id, url, sha256) = pack_store_fixture(&home);
    let removed = store
        .install(&id, "0.4.1", pack_artifact(&url, &sha256))
        .unwrap();
    let kept = store
        .install(&id, "0.4.2", pack_artifact(&url, &sha256))
        .unwrap();

    store.remove(&id, &["0.4.1".to_owned()]).unwrap();

    assert!(!removed.install_dir.exists());
    assert!(matches!(
        store.installed(&id, "0.4.1"),
        Err(PackStoreError::ManifestMismatch { .. })
    ));
    assert_eq!(
        store.installed(&id, "0.4.2").unwrap().install_dir,
        kept.install_dir
    );
}

#[cfg(unix)]
fn with_unwritable_state<T>(home: &TestHome, run: impl FnOnce() -> T) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;

    let original = fs::metadata(&home.root).unwrap().permissions();
    fs::set_permissions(&home.root, fs::Permissions::from_mode(0o500)).unwrap();
    // A privileged test runner can bypass directory permissions.
    let probe = home.root.join("write-probe");
    let result = if fs::File::create(&probe).is_err() {
        Some(run())
    } else {
        None
    };
    fs::set_permissions(&home.root, original).unwrap();
    result
}

fn pack_store_fixture(home: &TestHome) -> (PackStore, LanguageId, String, String) {
    let bytes = gzip_tar(&[("wax-lang-compose", b"#!/bin/sh\n", 0o644)]);
    let path = home.root.join("store-pack.tgz");
    fs::write(&path, &bytes).unwrap();
    (
        PackStore::new(WaxHome::new(&home.root)),
        LanguageId::try_from("compose").unwrap(),
        format!("file://{}", path.display()),
        sha256_hex(&bytes),
    )
}

fn pack_artifact<'a>(url: &'a str, sha256: &'a str) -> PackArtifact<'a> {
    PackArtifact {
        target: "aarch64-apple-darwin",
        url,
        sha256,
        pack_index_sha256: None,
        api_version: 1,
    }
}

#[test]
fn install_language_writes_manifest_and_unpacks_pack() {
    let home = TestHome::new("happy");

    let id = LanguageId::try_from("compose").expect("language id");

    let body = b"#!/bin/sh\necho hello\n";
    let artifact_bytes = gzip_tar(&[("wax-lang-compose", body.as_slice(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("pack.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let destination = PackStore::new(WaxHome::new(&home.root))
        .install(&id, "0.4.2", pack_artifact(&url, &digest))
        .expect("install succeeds")
        .install_dir;

    assert_eq!(destination, home.root.join("langs/compose/0.4.2"));

    let manifest_path = destination.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).expect("manifest exists");
    let parsed: ManifestJson = serde_json::from_str(&raw).expect("manifest parses");

    assert_eq!(parsed.id.as_str(), "compose");
    assert_eq!(parsed.version, "0.4.2");
    assert_eq!(parsed.api_version, 1);
    assert_eq!(parsed.command, ["./wax-lang-compose", "--stdio"]);
    assert_eq!(parsed.ecosystem, "compose");
    assert_eq!(parsed.parser_name, "compose");
    assert_eq!(parsed.parser_version, "0.4.2");
    assert_eq!(parsed.target, "aarch64-apple-darwin");
    assert_eq!(parsed.sha256, digest);

    let langs_language_dir = destination.parent().expect("language version parent");
    let stray_staging = fs::read_dir(langs_language_dir)
        .expect("read langs dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".install-"));
    assert_eq!(
        stray_staging.count(),
        0,
        "staging directories must not linger after promote"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin_path = destination.join("wax-lang-compose");
        let mode = fs::metadata(&bin_path)
            .expect("binary exists")
            .permissions()
            .mode();
        assert_ne!(
            mode & 0o111,
            0,
            "installed primary binary must be executable on unix"
        );
    }
}

#[test]
fn install_language_accepts_matching_pack_index_digest() {
    let home = TestHome::new("pack-match");

    let id = LanguageId::try_from("compose").expect("language id");

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", b"x".as_slice(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("pack-match.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    PackStore::new(WaxHome::new(&home.root))
        .install(
            &id,
            "0.4.2",
            PackArtifact {
                pack_index_sha256: Some(&digest),
                ..pack_artifact(&url, &digest)
            },
        )
        .expect("matching pack-index digest must install");

    assert!(
        home.root
            .join("langs/compose/0.4.2")
            .join("manifest.json")
            .exists(),
        "install should succeed when pack-index digest matches lock digest"
    );
}

#[test]
fn install_language_sha_mismatch_refuses_before_promotion() {
    let home = TestHome::new("sha");

    let id = LanguageId::try_from("compose").expect("language id");

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", b"payload".as_slice(), 0o644)]);

    let artifact_path = home.root.join("bad_digest.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let wrong_digest = "0".repeat(64);

    let err = PackStore::new(WaxHome::new(&home.root))
        .install(&id, "0.4.2", pack_artifact(&url, &wrong_digest))
        .expect_err("digest mismatch must abort");

    assert!(
        matches!(
            err,
            PackStoreError::Install(InstallError::ShaMismatch { .. })
        ),
        "unexpected error: {err:?}"
    );

    let destination = home.root.join("langs/compose/0.4.2");
    assert!(
        !destination.exists(),
        "failed installs must not leave the destination directory"
    );
}

#[test]
fn install_language_digest_drift_refuses_before_fetch() {
    let home = TestHome::new("drift");

    let id = LanguageId::try_from("compose").expect("language id");

    let lock_digest = "a".repeat(64);
    let index_digest = "b".repeat(64);

    let err = PackStore::new(WaxHome::new(&home.root))
        .install(
            &id,
            "0.4.2",
            PackArtifact {
                pack_index_sha256: Some(&index_digest),
                ..pack_artifact("file:///this/file/should/not/be/read", &lock_digest)
            },
        )
        .expect_err("digest drift must abort");

    assert!(
        matches!(
            err,
            PackStoreError::Install(InstallError::DigestDrift { .. })
        ),
        "unexpected error: {err:?}"
    );
}

#[test]
fn install_language_rejects_tar_path_traversal() {
    let home = TestHome::new("traversal");

    let id = LanguageId::try_from("compose").expect("language id");

    let artifact_bytes = gzip_tar(&[
        ("wax-lang-compose", b"ok".as_slice(), 0o644),
        ("../escape.txt", b"nope".as_slice(), 0o644),
    ]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("evil.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let err = PackStore::new(WaxHome::new(&home.root))
        .install(&id, "0.4.2", pack_artifact(&url, &digest))
        .expect_err("unsafe archive paths must abort");

    assert!(
        matches!(
            err,
            PackStoreError::Install(InstallError::PathTraversal { .. })
        ),
        "unexpected error: {err:?}"
    );

    let destination = home.root.join("langs/compose/0.4.2");
    assert!(
        !destination.exists(),
        "path traversal attempts must not promote partial installs"
    );
}

#[test]
fn install_language_cleans_staging_after_unpack_failure() {
    let home = TestHome::new("partial");

    let id = LanguageId::try_from("compose").expect("language id");

    let artifact_bytes = gzip_tar(&[
        ("wax-lang-compose", b"ok".as_slice(), 0o644),
        ("../escape.txt", b"nope".as_slice(), 0o644),
    ]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("partial.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let langs_parent = home.root.join("langs").join(id.as_str());
    fs::create_dir_all(&langs_parent).expect("seed langs tree");

    let url = format!("file://{}", artifact_path.display());

    PackStore::new(WaxHome::new(&home.root))
        .install(&id, "0.4.2", pack_artifact(&url, &digest))
        .expect_err("install should fail before promotion");

    let leftovers = fs::read_dir(&langs_parent)
        .expect("list langs compose dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".install-"));
    assert_eq!(
        leftovers.count(),
        0,
        "partial installs must delete staging directories after failures"
    );
}

#[test]
fn install_language_rejects_manifest_command_pointing_at_directory() {
    let home = TestHome::new("manifest-dir");

    let id = LanguageId::try_from("compose").expect("language id");

    let mut buffer = Vec::new();
    {
        let gz = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_path("wax-lang-compose").expect("tar path");
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append(&header, &[] as &[u8]).expect("tar append dir");
        tar.finish().expect("tar finish");
    }
    let digest = sha256_hex(&buffer);

    let artifact_path = home.root.join("manifest-dir.tgz");
    fs::write(&artifact_path, &buffer).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let err = PackStore::new(WaxHome::new(&home.root))
        .install(&id, "0.4.2", pack_artifact(&url, &digest))
        .expect_err("manifest command pointing at directory must abort");

    assert!(
        matches!(
            err,
            PackStoreError::Install(InstallError::InvalidPrimaryBinary { .. })
        ),
        "unexpected error: {err:?}"
    );
}
