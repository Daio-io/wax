use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use wax_contract::{LanguageId, RegistryError, RegistrySnapshotRef};
use wax_core::digest::sha256_hex;
use wax_core::registry_cache::{
    RegistryCache, RegistryCacheError, RegistryReadPolicy, RegistrySourceSpec,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "wax-registry-cache-{name}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn language(value: &str) -> LanguageId {
    LanguageId::try_from(value).expect("valid language id")
}

fn registry(symbol: &str) -> Vec<u8> {
    format!(r#"{{"schema_version":1,"components":[{{"id":"button","symbol":"{symbol}"}}]}}"#)
        .into_bytes()
}

fn registry_with_language(symbol: &str, language: &str) -> Vec<u8> {
    format!(
        r#"{{"schema_version":1,"language":"{language}","components":[{{"id":"button","symbol":"{symbol}"}}]}}"#
    )
    .into_bytes()
}

fn git(args: &[&str], cwd: &std::path::Path) -> Output {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Wax Test",
            "-c",
            "user.email=wax-test@example.com",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git_text(args: &[&str], cwd: &std::path::Path) -> String {
    String::from_utf8(git(args, cwd).stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

struct GitFixture {
    _root: TempDir,
    work: PathBuf,
    remote: PathBuf,
    first_commit: String,
}

impl GitFixture {
    fn new() -> Self {
        let root = TempDir::new("git");
        let work = root.0.join("work");
        let remote = root.0.join("remote.git");
        std::fs::create_dir_all(work.join(".wax/registries")).expect("registry directory");
        git(&["init", "-q", "-b", "main"], &work);
        git(
            &[
                "init",
                "--bare",
                "-q",
                remote.to_str().expect("remote path"),
            ],
            &root.0,
        );
        git(&["symbolic-ref", "HEAD", "refs/heads/main"], &remote);
        std::fs::write(work.join(".wax/registries/react.json"), registry("Button"))
            .expect("first registry");
        git(&["add", "."], &work);
        git(&["commit", "-q", "-m", "first"], &work);
        let first_commit = git_text(&["rev-parse", "HEAD"], &work);
        git(&["tag", "v1"], &work);
        git(
            &[
                "remote",
                "add",
                "origin",
                remote.to_str().expect("remote path"),
            ],
            &work,
        );
        git(&["push", "-q", "origin", "main", "--tags"], &work);
        Self {
            _root: root,
            work,
            remote,
            first_commit,
        }
    }

    fn publish_second(&self) -> String {
        std::fs::write(
            self.work.join(".wax/registries/react.json"),
            registry("NewButton"),
        )
        .expect("second registry");
        git(&["add", "."], &self.work);
        git(&["commit", "-q", "-m", "second"], &self.work);
        git(&["push", "-q", "origin", "main"], &self.work);
        git_text(&["rev-parse", "HEAD"], &self.work)
    }

    fn move_tag_to_head(&self, tag: &str) {
        git(&["tag", "-f", tag], &self.work);
        git(
            &[
                "push",
                "-q",
                "--force",
                "origin",
                &format!("refs/tags/{tag}"),
            ],
            &self.work,
        );
    }
}

fn serve_once(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    serve_responses(vec![body])
}

fn serve_responses(bodies: Vec<Vec<u8>>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
    let address = listener.local_addr().expect("fixture address");
    let handle = thread::spawn(move || {
        for body in bodies {
            let (mut stream, _) = listener.accept().expect("accept HTTP request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("read HTTP request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write HTTP headers");
            stream.write_all(&body).expect("write HTTP body");
        }
    });
    (format!("http://{address}/registry.json"), handle)
}

fn write_http_body(stream: &mut std::net::TcpStream, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write HTTP headers");
    stream.write_all(body).expect("write HTTP body");
}

fn serve_hold_second(
    first: Vec<u8>,
    second: Vec<u8>,
    accepted: Arc<Barrier>,
    release: Arc<Barrier>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
    let address = listener.local_addr().expect("fixture address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept first HTTP request");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).expect("read first HTTP request");
        write_http_body(&mut stream, &first);

        let (mut stream, _) = listener.accept().expect("accept refresh HTTP request");
        let mut request = [0_u8; 1024];
        let _ = stream
            .read(&mut request)
            .expect("read refresh HTTP request");
        accepted.wait();
        release.wait();
        write_http_body(&mut stream, &second);
    });
    (format!("http://{address}/registry.json"), handle)
}

#[test]
fn failed_refresh_keeps_selected_snapshot() {
    let root = TempDir::new("failed-refresh");
    let cache = RegistryCache::open(root.0.join("cache"));
    let (url, server) = serve_responses(vec![registry("Button"), b"not json".to_vec()]);
    let source = RegistrySourceSpec::HttpJson {
        url,
        language: Some(language("react")),
    };
    let selected = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("select first snapshot");

    let refresh_error = cache
        .refresh(&source)
        .expect_err("invalid refresh must fail validation");
    server.join().expect("HTTP fixture completes");
    let retained = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .expect("old selected snapshot remains available");

    assert!(matches!(
        refresh_error,
        RegistryCacheError::InvalidRegistry { .. }
    ));
    assert_eq!(retained, selected);
}

#[test]
fn default_branch_is_retained_until_refresh() {
    let fixture = GitFixture::new();
    let cache = RegistryCache::open(fixture._root.0.join("cache"));
    let source = RegistrySourceSpec::Git {
        url: fixture.remote.display().to_string(),
        tag: None,
        language: language("react"),
    };
    let first = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("resolve default branch");
    let second_commit = fixture.publish_second();

    let retained = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("reuse selected default-branch commit");
    let refreshed = cache.refresh(&source).expect("refresh default branch");

    assert_eq!(first[0].reference.git_commit, Some(fixture.first_commit));
    assert_eq!(retained, first);
    assert_eq!(refreshed[0].reference.git_commit, Some(second_commit));
    assert_eq!(refreshed[0].document.components[0].symbol, "NewButton");
}

#[test]
fn publisher_registry_mapping_identifies_language() {
    let root = TempDir::new("publisher-mapping");
    std::fs::create_dir_all(root.0.join(".wax/registries")).expect("publisher registry directory");
    std::fs::write(
        root.0.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "react": { "source": ".wax/registries/react.json" }
      }
    }
  }
}"#,
    )
    .expect("publisher config");
    std::fs::write(
        root.0.join(".wax/registries/react.json"),
        registry("Button"),
    )
    .expect("publisher registry");
    let cache = RegistryCache::open(root.0.join("cache"));
    let source = RegistrySourceSpec::PublisherRepository {
        root: root.0.clone(),
        system_id: None,
        language: None,
    };

    let snapshots = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("resolve publisher registry mapping");

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].reference.system_id.as_deref(), Some("acme"));
    assert_eq!(snapshots[0].reference.language, language("react"));
    assert_eq!(snapshots[0].document.components[0].symbol, "Button");
}

#[test]
fn expected_snapshot_rejects_other_publisher_root() {
    let root = TempDir::new("publisher-expected-root");
    let publisher_a = root.0.join("publisher-a");
    let publisher_b = root.0.join("publisher-b");
    for publisher in [&publisher_a, &publisher_b] {
        std::fs::create_dir_all(publisher.join(".wax/registries"))
            .expect("publisher registry directory");
        std::fs::write(
            publisher.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "react": { "source": ".wax/registries/react.json" }
      }
    }
  }
}"#,
        )
        .expect("publisher config");
        std::fs::write(
            publisher.join(".wax/registries/react.json"),
            registry("Button"),
        )
        .expect("publisher registry");
    }
    let cache = RegistryCache::open(root.0.join("cache"));
    let source_a = RegistrySourceSpec::PublisherRepository {
        root: publisher_a,
        system_id: Some("acme".to_owned()),
        language: Some(language("react")),
    };
    let source_b = RegistrySourceSpec::PublisherRepository {
        root: publisher_b,
        system_id: Some("acme".to_owned()),
        language: Some(language("react")),
    };

    let snapshots = cache
        .resolve(&source_a, &RegistryReadPolicy::default())
        .expect("resolve publisher A");
    let error = cache
        .resolve(
            &source_b,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: Some(snapshots[0].reference.clone()),
            },
        )
        .expect_err("expected snapshot from another publisher root must not match");

    assert!(matches!(error, RegistryCacheError::SnapshotMismatch { .. }));
}

#[test]
fn expected_snapshot_rejects_other_publisher_system() {
    let root = TempDir::new("publisher-expected-system");
    std::fs::create_dir_all(root.0.join(".wax/registries")).expect("publisher registry directory");
    std::fs::write(
        root.0.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "react": { "source": ".wax/registries/acme.json" }
      }
    },
    "beta": {
      "name": "Beta",
      "registries": {
        "react": { "source": ".wax/registries/beta.json" }
      }
    }
  }
}"#,
    )
    .expect("publisher config");
    std::fs::write(
        root.0.join(".wax/registries/acme.json"),
        registry("AcmeButton"),
    )
    .expect("acme registry");
    std::fs::write(
        root.0.join(".wax/registries/beta.json"),
        registry("BetaButton"),
    )
    .expect("beta registry");
    let cache = RegistryCache::open(root.0.join("cache"));
    let acme = RegistrySourceSpec::PublisherRepository {
        root: root.0.clone(),
        system_id: Some("acme".to_owned()),
        language: Some(language("react")),
    };
    let beta = RegistrySourceSpec::PublisherRepository {
        root: root.0.clone(),
        system_id: Some("beta".to_owned()),
        language: Some(language("react")),
    };

    let snapshots = cache
        .resolve(&acme, &RegistryReadPolicy::default())
        .expect("resolve acme publisher");
    let error = cache
        .resolve(
            &beta,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: Some(snapshots[0].reference.clone()),
            },
        )
        .expect_err("expected snapshot from another publisher system must not match");

    assert!(matches!(error, RegistryCacheError::SnapshotMismatch { .. }));
}

#[test]
fn invalid_cached_registry_preserves_typed_registry_error() {
    let root = TempDir::new("invalid-cached-typed");
    let cache_root = root.0.join("cache");
    let bytes = b"not-json";
    let digest = sha256_hex(bytes);
    let blob_path = cache_root.join("blobs").join(format!("{digest}.json"));
    std::fs::create_dir_all(blob_path.parent().expect("blob parent")).expect("blob directory");
    std::fs::write(&blob_path, bytes).expect("invalid cached blob");
    let path = root.0.join("registry.json");
    let source = RegistrySourceSpec::LocalFile {
        path: path.clone(),
        language: Some(language("react")),
    };
    let identity = format!("file:{}", path.display());
    let selection_path = cache_root.join("sources").join(format!(
        "{}.json",
        sha256_hex(format!("{identity}|language=react").as_bytes())
    ));
    std::fs::create_dir_all(selection_path.parent().expect("selection parent"))
        .expect("selection directory");
    let reference = RegistrySnapshotRef {
        source: identity,
        language: language("react"),
        digest,
        system_id: None,
        git_commit: None,
        extra: serde_json::Map::new(),
    };
    std::fs::write(
        &selection_path,
        serde_json::to_vec(&serde_json::json!({ "snapshots": [reference] }))
            .expect("encode selection"),
    )
    .expect("selection record");
    let cache = RegistryCache::open(cache_root);

    let error = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .expect_err("invalid cached registry must fail");

    match error {
        RegistryCacheError::InvalidCachedRegistry { source, .. } => {
            assert!(matches!(source, RegistryError::InvalidJson(_)));
        }
        other => panic!("expected InvalidCachedRegistry, got {other:?}"),
    }
}

#[test]
fn source_parser_distinguishes_http_json_and_git_forms() {
    let react = Some(language("react"));

    assert!(matches!(
        RegistrySourceSpec::parse("https://cdn.example.com/react.json", react.clone())
            .expect("parse HTTP JSON"),
        RegistrySourceSpec::HttpJson { .. }
    ));
    assert!(matches!(
        RegistrySourceSpec::parse("https://github.com/acme/design.git", react.clone())
            .expect("parse .git URL"),
        RegistrySourceSpec::Git { .. }
    ));
    assert!(matches!(
        RegistrySourceSpec::parse("github.com/acme/design", react)
            .expect("parse host/repository shorthand"),
        RegistrySourceSpec::Git { .. }
    ));
}

#[test]
fn concurrent_first_readers_share_one_fetch() {
    let root = TempDir::new("concurrent-readers");
    let cache = Arc::new(RegistryCache::open(root.0.join("cache")));
    let (url, server) = serve_once(registry("Button"));
    let source = Arc::new(RegistrySourceSpec::HttpJson {
        url,
        language: Some(language("react")),
    });
    let barrier = Arc::new(Barrier::new(6));
    #[expect(
        clippy::needless_collect,
        reason = "collect JoinHandles before joining so readers overlap at the barrier"
    )]
    let readers: Vec<_> = (0..6)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let source = Arc::clone(&source);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                cache.resolve(&source, &RegistryReadPolicy::default())
            })
        })
        .collect();

    let snapshots: Vec<_> = readers
        .into_iter()
        .map(|reader| {
            reader
                .join()
                .expect("reader thread")
                .expect("resolve registry")
        })
        .collect();
    server.join().expect("HTTP fixture completes");

    assert!(snapshots.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn offline_miss_is_explicit() {
    let root = TempDir::new("offline-miss");
    let cache = RegistryCache::open(root.0.join("cache"));
    let source = RegistrySourceSpec::HttpJson {
        url: "http://127.0.0.1:9/registry.json".to_owned(),
        language: Some(language("react")),
    };

    let error = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .expect_err("an uncached offline source must fail");

    assert!(matches!(error, RegistryCacheError::OfflineCacheMiss { .. }));
}

#[test]
fn first_remote_read_fetches_then_reads_offline() {
    let root = TempDir::new("remote-offline");
    let cache = RegistryCache::open(root.0.join("cache"));
    let (url, server) = serve_once(registry("Button"));
    let source = RegistrySourceSpec::HttpJson {
        url: url.clone(),
        language: Some(language("react")),
    };

    let fetched = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("fetch remote registry");
    server.join().expect("HTTP fixture completes");
    let offline = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .expect("read selected snapshot offline");

    assert_eq!(offline, fetched);
    assert_eq!(offline[0].reference.source, url);
    assert_eq!(offline[0].reference.language, language("react"));
    assert_eq!(offline[0].document.components[0].symbol, "Button");
}

#[test]
fn same_bytes_different_sources_keep_context() {
    let root = TempDir::new("same-bytes");
    let bytes = registry("Button");
    let left_path = root.0.join("left.json");
    let right_path = root.0.join("right.json");
    std::fs::write(&left_path, &bytes).expect("left registry");
    std::fs::write(&right_path, &bytes).expect("right registry");
    let cache = RegistryCache::open(root.0.join("cache"));
    let left = RegistrySourceSpec::LocalFile {
        path: left_path,
        language: Some(language("react")),
    };
    let right = RegistrySourceSpec::LocalFile {
        path: right_path,
        language: Some(language("react")),
    };

    let left_snapshots = cache
        .resolve(&left, &RegistryReadPolicy::default())
        .expect("resolve left source");
    let right_snapshots = cache
        .resolve(&right, &RegistryReadPolicy::default())
        .expect("resolve right source");

    assert_eq!(
        left_snapshots[0].reference.digest,
        right_snapshots[0].reference.digest
    );
    assert_ne!(
        left_snapshots[0].reference.source,
        right_snapshots[0].reference.source
    );
    assert_eq!(left_snapshots[0].document, right_snapshots[0].document);
}

#[test]
fn reader_keeps_snapshot_during_refresh() {
    let root = TempDir::new("reader-during-refresh");
    let cache = Arc::new(RegistryCache::open(root.0.join("cache")));
    let accepted = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let (url, server) = serve_hold_second(
        registry("Button"),
        registry("NewButton"),
        Arc::clone(&accepted),
        Arc::clone(&release),
    );
    let source = Arc::new(RegistrySourceSpec::HttpJson {
        url,
        language: Some(language("react")),
    });
    let selected = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("select first snapshot");

    let refresh_cache = Arc::clone(&cache);
    let refresh_source = Arc::clone(&source);
    let refresh = thread::spawn(move || refresh_cache.refresh(&refresh_source));
    accepted.wait();
    let during_refresh = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("selected snapshot remains readable during refresh");
    release.wait();
    let refreshed = refresh
        .join()
        .expect("refresh thread")
        .expect("refresh completes");
    server.join().expect("HTTP fixture completes");

    assert_eq!(during_refresh, selected);
    assert_eq!(selected[0].document.components[0].symbol, "Button");
    assert_eq!(refreshed[0].document.components[0].symbol, "NewButton");
}

#[test]
fn tagged_git_source_is_retained_until_refresh() {
    let fixture = GitFixture::new();
    let cache = RegistryCache::open(fixture._root.0.join("cache"));
    let source = RegistrySourceSpec::Git {
        url: fixture.remote.display().to_string(),
        tag: Some("v1".to_owned()),
        language: language("react"),
    };
    let first = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("resolve tagged registry");
    let second_commit = fixture.publish_second();
    fixture.move_tag_to_head("v1");

    let retained = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("reuse selected tag snapshot");
    let refreshed = cache.refresh(&source).expect("refresh moved tag");

    assert_eq!(first[0].reference.git_commit, Some(fixture.first_commit));
    assert_eq!(retained, first);
    assert_eq!(refreshed[0].reference.git_commit, Some(second_commit));
    assert_eq!(refreshed[0].document.components[0].symbol, "NewButton");
}

#[test]
fn commit_git_source_stays_pinned() {
    let fixture = GitFixture::new();
    let cache = RegistryCache::open(fixture._root.0.join("cache"));
    let source = RegistrySourceSpec::Git {
        url: fixture.remote.display().to_string(),
        tag: Some(fixture.first_commit.clone()),
        language: language("react"),
    };
    let first = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("resolve commit registry");
    fixture.publish_second();

    let retained = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("reuse selected commit snapshot");
    let refreshed = cache.refresh(&source).expect("refresh pinned commit");

    assert_eq!(
        first[0].reference.git_commit.as_deref(),
        Some(fixture.first_commit.as_str())
    );
    assert_eq!(retained, first);
    assert_eq!(refreshed, first);
}

#[test]
fn missing_conventional_git_registry_is_explicit() {
    let fixture = GitFixture::new();
    let cache = RegistryCache::open(fixture._root.0.join("cache"));
    let source = RegistrySourceSpec::Git {
        url: fixture.remote.display().to_string(),
        tag: Some("v1".to_owned()),
        language: language("compose"),
    };

    let error = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect_err("missing conventional registry must fail");

    assert!(matches!(
        error,
        RegistryCacheError::Git(
            wax_core::registry_git::RegistryGitError::RegistryFileAbsent { .. }
        )
    ));
}

#[test]
fn missing_published_registry_is_explicit() {
    let root = TempDir::new("missing-published");
    std::fs::create_dir_all(root.0.join(".wax/registries")).expect("publisher registry directory");
    std::fs::write(
        root.0.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme",
      "registries": {
        "react": { "source": ".wax/registries/react.json" }
      }
    }
  }
}"#,
    )
    .expect("publisher config");
    std::fs::write(
        root.0.join(".wax/registries/react.json"),
        registry("Button"),
    )
    .expect("publisher registry");
    let cache = RegistryCache::open(root.0.join("cache"));
    let source = RegistrySourceSpec::PublisherRepository {
        root: root.0.clone(),
        system_id: Some("acme".to_owned()),
        language: Some(language("compose")),
    };

    let error = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect_err("missing published language must fail");

    assert!(matches!(
        error,
        RegistryCacheError::MissingPublishedRegistry { .. }
    ));
}

#[test]
fn conflicting_source_and_artifact_language_is_rejected() {
    let root = TempDir::new("language-mismatch");
    let path = root.0.join("registry.json");
    std::fs::write(&path, registry_with_language("Button", "compose")).expect("registry");
    let cache = RegistryCache::open(root.0.join("cache"));
    let source = RegistrySourceSpec::LocalFile {
        path,
        language: Some(language("react")),
    };

    let error = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect_err("language mismatch must fail");

    assert!(matches!(error, RegistryCacheError::LanguageMismatch { .. }));
}

#[test]
fn credential_bearing_urls_are_redacted_from_records() {
    let root = TempDir::new("redact-creds");
    let cache = RegistryCache::open(root.0.join("cache"));
    let (url, server) = serve_once(registry("Button"));
    let credentialed = url.replacen("http://", "http://user:secret@", 1);
    let source = RegistrySourceSpec::HttpJson {
        url: credentialed,
        language: Some(language("react")),
    };

    let snapshots = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("fetch credentialed URL through local fixture");
    server.join().expect("HTTP fixture completes");

    assert!(!snapshots[0].reference.source.contains("secret"));
    assert!(!snapshots[0].reference.source.contains("user:"));
    assert!(
        snapshots[0]
            .reference
            .source
            .starts_with("http://127.0.0.1:")
    );
}

#[test]
fn invalid_http_json_is_validation_failure_not_git() {
    let root = TempDir::new("invalid-http-json");
    let cache = RegistryCache::open(root.0.join("cache"));
    let (url, server) = serve_once(b"not json".to_vec());
    let source = RegistrySourceSpec::parse(&url, Some(language("react")))
        .expect("HTTP JSON URL parses as HttpJson");
    assert!(matches!(source, RegistrySourceSpec::HttpJson { .. }));

    let error = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect_err("invalid HTTP JSON must fail validation");
    server.join().expect("HTTP fixture completes");

    assert!(matches!(error, RegistryCacheError::InvalidRegistry { .. }));
}

#[test]
fn concurrent_refresh_leaves_one_valid_selection() {
    let root = TempDir::new("concurrent-refresh");
    let cache = Arc::new(RegistryCache::open(root.0.join("cache")));
    let bodies = Arc::new(Mutex::new(vec![
        registry("Button"),
        registry("Second"),
        registry("Third"),
    ]));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
    let address = listener.local_addr().expect("fixture address");
    let server_bodies = Arc::clone(&bodies);
    let server = thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().expect("accept HTTP request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("read HTTP request");
            let body = server_bodies.lock().expect("body lock").remove(0);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write HTTP headers");
            stream.write_all(&body).expect("write HTTP body");
        }
    });
    let url = format!("http://{address}/registry.json");
    let source = Arc::new(RegistrySourceSpec::HttpJson {
        url,
        language: Some(language("react")),
    });
    let first = cache
        .resolve(&source, &RegistryReadPolicy::default())
        .expect("seed selection");
    assert_eq!(first[0].document.components[0].symbol, "Button");

    let barrier = Arc::new(Barrier::new(2));
    #[expect(
        clippy::needless_collect,
        reason = "collect JoinHandles before joining so refreshes overlap at the barrier"
    )]
    let refreshers: Vec<_> = (0..2)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let source = Arc::clone(&source);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                cache.refresh(&source)
            })
        })
        .collect();
    let refreshed: Vec<_> = refreshers
        .into_iter()
        .map(|thread| {
            thread
                .join()
                .expect("refresh thread")
                .expect("refresh succeeds")
        })
        .collect();
    server.join().expect("HTTP fixture completes");
    let selected = cache
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: true,
                expected_snapshot: None,
            },
        )
        .expect("offline selected snapshot");

    assert!(refreshed.iter().any(|snapshots| snapshots == &selected));
    assert!(matches!(
        selected[0].document.components[0].symbol.as_str(),
        "Second" | "Third"
    ));
}
