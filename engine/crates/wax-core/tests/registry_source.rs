use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wax_core::registry_source::{
    RegistrySourceError, RegistrySourceInput, resolve_registry_source,
    resolve_registry_source_with_deprecation,
};

const REGISTRY_JSON: &str =
    r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton"}]}"#;

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "wax-core-registry-source-{}-{nonce}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct HttpFixtureServer {
    url: String,
    cancel: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HttpFixtureServer {
    fn spawn(body: &'static str) -> Self {
        Self::spawn_with_delay(body, Duration::ZERO)
    }

    fn spawn_with_delay(body: &'static str, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let url = format!("http://{address}/registry.json");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            if !delay.is_zero() {
                let sleep_step = Duration::from_millis(10);
                let started = Instant::now();
                while started.elapsed() < delay {
                    if cancel_for_thread.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(sleep_step.min(delay - started.elapsed()));
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        Self {
            url,
            cancel,
            handle: Some(handle),
        }
    }

    fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for HttpFixtureServer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn repo_relative_symlink_cannot_escape_repo() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new();
    let outside = repo.path().with_extension("outside-registry.json");
    fs::write(&outside, REGISTRY_JSON).unwrap();
    symlink(&outside, repo.path().join("linked.registry.json")).unwrap();

    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some("linked.registry.json"),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::PathEscapesRepo { .. }));
}

#[test]
fn missing_registry_defaults_to_per_language_registry() {
    let repo = TestRepo::new();
    fs::create_dir_all(repo.path().join(".wax")).unwrap();
    fs::write(
        repo.path().join(".wax/compose.registry.json"),
        REGISTRY_JSON,
    )
    .unwrap();

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: None,
    })
    .unwrap();

    assert_eq!(resolved.source, ".wax/compose.registry.json");
    assert_eq!(resolved.repo_relative_path, ".wax/compose.registry.json");
    assert_eq!(resolved.sha256.len(), 64);
    assert!(!resolved.deprecated);
}

#[test]
fn registry_string_resolves_repo_relative_path() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("compose.registry.json"), REGISTRY_JSON).unwrap();

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some("compose.registry.json"),
    })
    .unwrap();

    assert_eq!(resolved.source, "compose.registry.json");
    assert_eq!(resolved.repo_relative_path, "compose.registry.json");
    assert_eq!(resolved.sha256.len(), 64);
}

#[test]
fn file_url_materializes_under_cache() {
    let repo = TestRepo::new();
    let outside = repo.path().with_extension("outside-registry.json");
    fs::write(&outside, REGISTRY_JSON).unwrap();

    let source = format!("file://{}", outside.display());
    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some(&source),
    })
    .unwrap();

    assert_eq!(resolved.source, source);
    assert!(
        resolved
            .repo_relative_path
            .starts_with(".wax/cache/registries/compose-")
    );
    assert!(resolved.repo_relative_path.ends_with(".json"));
    assert!(repo.path().join(&resolved.repo_relative_path).is_file());
}

#[test]
fn http_url_materializes_under_cache() {
    let repo = TestRepo::new();
    let server = HttpFixtureServer::spawn(REGISTRY_JSON);

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some(server.url()),
    })
    .unwrap();

    assert_eq!(resolved.source, server.url());
    assert!(
        resolved
            .repo_relative_path
            .starts_with(".wax/cache/registries/compose-")
    );
    assert!(resolved.repo_relative_path.ends_with(".json"));
    assert!(repo.path().join(&resolved.repo_relative_path).is_file());
}

#[test]
fn http_url_uses_a_request_timeout() {
    let repo = TestRepo::new();
    let server = HttpFixtureServer::spawn_with_delay(REGISTRY_JSON, Duration::from_secs(6));
    let started = Instant::now();

    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some(server.url()),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::Fetch { .. }));
    let elapsed = started.elapsed();
    drop(server);
    assert!(elapsed < Duration::from_secs(6));
}

#[test]
fn absolute_path_is_rejected() {
    let repo = TestRepo::new();
    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some("/tmp/registry.json"),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::PlainAbsolutePath { .. }));
}

#[test]
fn malformed_registry_is_rejected() {
    let repo = TestRepo::new();
    fs::create_dir_all(repo.path().join(".wax")).unwrap();
    fs::write(
        repo.path().join(".wax/compose.registry.json"),
        "{\"components\":[]}",
    )
    .unwrap();

    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: None,
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::InvalidShape { .. }));
}

#[test]
fn preserves_deprecated_source_marker() {
    let repo = TestRepo::new();
    fs::create_dir_all(repo.path().join(".wax")).unwrap();
    fs::write(
        repo.path().join(".wax/compose.registry.json"),
        REGISTRY_JSON,
    )
    .unwrap();

    let resolved = resolve_registry_source_with_deprecation(
        RegistrySourceInput {
            repo_root: repo.path(),
            language_id: "compose",
            source: None,
        },
        true,
    )
    .unwrap();

    assert!(resolved.deprecated);
}

#[test]
fn language_id_cannot_escape_registry_cache_path() {
    let repo = TestRepo::new();
    let outside = repo.path().with_extension("outside-registry.json");
    fs::write(&outside, REGISTRY_JSON).unwrap();

    let source = format!("file://{}", outside.display());
    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "../../../../escape",
        source: Some(&source),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::PathEscapesRepo { .. }));
    assert!(!repo.path().join(".wax/cache/registries").exists());
}

#[cfg(unix)]
#[test]
fn cache_parent_symlink_cannot_escape_repo_on_write() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new();
    let outside_registry = repo.path().with_extension("outside-registry.json");
    let outside_cache_dir = repo.path().with_extension("outside-cache-dir");
    fs::write(&outside_registry, REGISTRY_JSON).unwrap();
    fs::create_dir_all(&outside_cache_dir).unwrap();
    fs::create_dir_all(repo.path().join(".wax/cache")).unwrap();
    symlink(
        &outside_cache_dir,
        repo.path().join(".wax/cache/registries"),
    )
    .unwrap();

    let source = format!("file://{}", outside_registry.display());
    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some(&source),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::PathEscapesRepo { .. }));
    assert_eq!(fs::read_dir(&outside_cache_dir).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn cache_file_symlink_is_not_followed_on_write() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new();
    let outside_registry = repo.path().with_extension("outside-registry.json");
    let outside_target = repo.path().with_extension("outside-cache-target.json");
    fs::write(&outside_registry, REGISTRY_JSON).unwrap();
    fs::write(&outside_target, "do not overwrite").unwrap();

    let sha256 = {
        use sha2::{Digest, Sha256};

        let digest = Sha256::digest(REGISTRY_JSON.as_bytes());
        digest
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    };

    let cache_dir = repo.path().join(".wax/cache/registries");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join(format!("compose-{sha256}.json"));
    symlink(&outside_target, &cache_path).unwrap();

    let source = format!("file://{}", outside_registry.display());
    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: repo.path(),
        language_id: "compose",
        source: Some(&source),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::CacheWrite { .. }));
    assert_eq!(
        fs::read_to_string(&outside_target).unwrap(),
        "do not overwrite"
    );
}
