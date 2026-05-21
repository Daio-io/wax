//! Integration tests for [`wax_core::install::install_language`].

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use wax_contract::LanguageId;
use wax_core::install::{InstallError, LanguagePackManifestSpec, install_language};
use wax_core::paths::lang_install_dir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

struct TestHome {
    root: PathBuf,
    _wax_guard: EnvVarGuard,
}

impl TestHome {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wax-core-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp wax home");

        let wax_guard = EnvVarGuard::set("WAX_HOME", &root);

        Self {
            root,
            _wax_guard: wax_guard,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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

fn sample_manifest_with_command(id: LanguageId, command: Vec<String>) -> LanguagePackManifestSpec {
    LanguagePackManifestSpec {
        id,
        version: "0.4.2".to_owned(),
        api_version: 1,
        command,
        ecosystem: "jetpack-compose".to_owned(),
        parser_name: "tree-sitter-kotlin".to_owned(),
        parser_version: "0.3.8".to_owned(),
    }
}

fn sample_manifest(id: LanguageId) -> LanguagePackManifestSpec {
    sample_manifest_with_command(
        id,
        vec!["./wax-lang-compose".to_owned(), "--stdio".to_owned()],
    )
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
}

#[test]
fn install_language_writes_manifest_and_unpacks_pack() {
    let _guard = env_lock();
    let home = TestHome::new("happy");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let body = b"#!/bin/sh\necho hello\n";
    let artifact_bytes = gzip_tar(&[("wax-lang-compose", body.as_slice(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("pack.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let destination = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        None,
        &manifest,
    )
    .expect("install succeeds");

    assert_eq!(
        destination,
        lang_install_dir(&id, "0.4.2").expect("destination path")
    );

    let manifest_path = destination.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).expect("manifest exists");
    let parsed: ManifestJson = serde_json::from_str(&raw).expect("manifest parses");

    assert_eq!(parsed.id.as_str(), "compose");
    assert_eq!(parsed.version, "0.4.2");
    assert_eq!(parsed.api_version, 1);
    assert_eq!(parsed.command, manifest.command);
    assert_eq!(parsed.ecosystem, "jetpack-compose");
    assert_eq!(parsed.parser_name, "tree-sitter-kotlin");
    assert_eq!(parsed.parser_version, "0.3.8");

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
    let _guard = env_lock();
    let home = TestHome::new("pack-match");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", b"x".as_slice(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("pack-match.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        Some(&digest),
        &manifest,
    )
    .expect("matching pack-index digest must install");

    assert!(
        lang_install_dir(&id, "0.4.2")
            .expect("destination path")
            .join("manifest.json")
            .exists(),
        "install should succeed when pack-index digest matches lock digest"
    );
}

#[test]
fn install_language_sha_mismatch_refuses_before_promotion() {
    let _guard = env_lock();
    let home = TestHome::new("sha");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", b"payload".as_slice(), 0o644)]);

    let artifact_path = home.root.join("bad_digest.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let wrong_digest = "0".repeat(64);

    let err = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &wrong_digest,
        None,
        &manifest,
    )
    .expect_err("digest mismatch must abort");

    assert!(
        matches!(err, InstallError::ShaMismatch { .. }),
        "unexpected error: {err:?}"
    );

    let destination = lang_install_dir(&id, "0.4.2").expect("destination path");
    assert!(
        !destination.exists(),
        "failed installs must not leave the destination directory"
    );
}

#[test]
fn install_language_digest_drift_refuses_before_fetch() {
    let _guard = env_lock();
    let _home = TestHome::new("drift");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let lock_digest = "a".repeat(64);
    let index_digest = "b".repeat(64);

    let err = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        "file:///this/file/should/not/be/read",
        &lock_digest,
        Some(&index_digest),
        &manifest,
    )
    .expect_err("digest drift must abort");

    assert!(
        matches!(err, InstallError::DigestDrift { .. }),
        "unexpected error: {err:?}"
    );
}

#[test]
fn install_language_rejects_tar_path_traversal() {
    let _guard = env_lock();
    let home = TestHome::new("traversal");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let artifact_bytes = gzip_tar(&[
        ("wax-lang-compose", b"ok".as_slice(), 0o644),
        ("../escape.txt", b"nope".as_slice(), 0o644),
    ]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("evil.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let err = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        None,
        &manifest,
    )
    .expect_err("unsafe archive paths must abort");

    assert!(
        matches!(err, InstallError::PathTraversal { .. }),
        "unexpected error: {err:?}"
    );

    let destination = lang_install_dir(&id, "0.4.2").expect("destination path");
    assert!(
        !destination.exists(),
        "path traversal attempts must not promote partial installs"
    );
}

#[test]
fn install_language_cleans_staging_after_unpack_failure() {
    let _guard = env_lock();
    let home = TestHome::new("partial");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

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

    install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        None,
        &manifest,
    )
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
fn install_language_rejects_unsafe_manifest_command_path() {
    let _guard = env_lock();
    let home = TestHome::new("manifest-traversal");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest_with_command(
        id.clone(),
        vec!["./../outside".to_owned(), "--stdio".to_owned()],
    );

    let artifact_bytes = gzip_tar(&[("wax-lang-compose", b"ok".as_slice(), 0o644)]);
    let digest = sha256_hex(&artifact_bytes);

    let artifact_path = home.root.join("manifest-traversal.tgz");
    fs::write(&artifact_path, &artifact_bytes).expect("artifact");

    let url = format!("file://{}", artifact_path.display());

    let err = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        None,
        &manifest,
    )
    .expect_err("unsafe manifest command paths must abort");

    assert!(
        matches!(
            err,
            InstallError::PathTraversal { .. } | InstallError::InvalidPrimaryBinary { .. }
        ),
        "unexpected error: {err:?}"
    );

    let destination = lang_install_dir(&id, "0.4.2").expect("destination path");
    assert!(
        !destination.exists(),
        "unsafe manifest command paths must not promote partial installs"
    );
}

#[test]
fn install_language_rejects_manifest_command_pointing_at_directory() {
    let _guard = env_lock();
    let home = TestHome::new("manifest-dir");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

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

    let err = install_language(
        &id,
        "0.4.2",
        "aarch64-apple-darwin",
        &url,
        &digest,
        None,
        &manifest,
    )
    .expect_err("manifest command pointing at directory must abort");

    assert!(
        matches!(err, InstallError::InvalidPrimaryBinary { .. }),
        "unexpected error: {err:?}"
    );
}

#[test]
fn install_language_replaces_existing_install() {
    let _guard = env_lock();
    let home = TestHome::new("replace");

    let id = LanguageId::try_from("compose").expect("language id");
    let manifest = sample_manifest(id.clone());

    let install_once = |body: &[u8]| {
        let artifact_bytes = gzip_tar(&[("wax-lang-compose", body, 0o644)]);
        let digest = sha256_hex(&artifact_bytes);
        let artifact_path = home.root.join(format!("replace-{digest}.tgz"));
        fs::write(&artifact_path, &artifact_bytes).expect("artifact");
        let url = format!("file://{}", artifact_path.display());
        install_language(
            &id,
            "0.4.2",
            "aarch64-apple-darwin",
            &url,
            &digest,
            None,
            &manifest,
        )
        .expect("install succeeds")
    };

    let first = install_once(b"version-one");
    let second = install_once(b"version-two");

    assert_eq!(first, second);

    let bin_path = second.join("wax-lang-compose");
    let contents = fs::read(&bin_path).expect("binary exists after replacement");
    assert_eq!(contents, b"version-two");

    let langs_language_dir = second.parent().expect("language version parent");
    let replaced_backups = fs::read_dir(langs_language_dir)
        .expect("read langs dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".replaced-")
        });
    assert_eq!(
        replaced_backups.count(),
        0,
        "replaced-install backup directories must not linger after promotion"
    );
}
