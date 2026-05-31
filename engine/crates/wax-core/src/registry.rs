//! Registry index loading and target artifact selection for language packs.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use wax_contract::LanguageId;

/// One pack-index manifest entry for a language/version.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryManifest {
    /// Stable language id.
    pub id: LanguageId,
    /// Pack version string.
    pub version: String,
    /// Language pack wire API version.
    pub api_version: u32,
    /// Artifact variants keyed by target triple.
    pub targets: BTreeMap<String, RegistryArtifact>,
}

/// Download metadata for a target-specific artifact.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryArtifact {
    /// Artifact download URL.
    pub url: String,
    /// Expected SHA-256 digest for integrity verification.
    pub sha256: String,
}

/// Typed failures while loading registry index manifests and selecting artifacts.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Registry URL does not use a supported scheme.
    #[error(
        "unsupported registry URL scheme in {url}; only file://, http://, and https:// are supported"
    )]
    UnsupportedScheme {
        /// The URL passed to [`fetch_pack_index`].
        url: String,
    },
    /// Registry URL is malformed and cannot be converted to a file path.
    #[error("invalid file:// registry URL {url}: {reason}")]
    InvalidFileUrl {
        /// The URL passed to [`fetch_pack_index`].
        url: String,
        /// Human-readable parse reason.
        reason: &'static str,
    },
    /// Registry index file could not be read.
    #[error("failed to read registry manifest index from {path}: {source}")]
    Read {
        /// Filesystem path derived from the provided URL.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Registry index request exceeded timeout.
    #[error("timed out fetching registry manifest index from {url}")]
    Timeout {
        /// URL passed to [`fetch_pack_index`].
        url: String,
    },
    /// Registry index request returned a non-success HTTP status.
    #[error("failed to fetch registry manifest index from {url}: HTTP {status}")]
    HttpStatus {
        /// URL passed to [`fetch_pack_index`].
        url: String,
        /// HTTP response status code.
        status: reqwest::StatusCode,
    },
    /// Registry index request failed.
    #[error("failed to fetch registry manifest index from {url}: {source}")]
    HttpRequest {
        /// URL passed to [`fetch_pack_index`].
        url: String,
        /// Underlying HTTP error.
        #[source]
        source: reqwest::Error,
    },
    /// Registry index contains malformed JSON.
    #[error("malformed registry manifest index JSON in {path}: {source}")]
    MalformedJson {
        /// Filesystem path derived from the provided URL.
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// Remote registry index contains malformed JSON.
    #[error("malformed remote registry manifest index JSON from {url}: {source}")]
    MalformedRemoteJson {
        /// URL passed to [`fetch_pack_index`].
        url: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// Registry index JSON does not match the expected manifest shape.
    #[error("invalid registry manifest index in {path}: {source}")]
    InvalidManifest {
        /// Filesystem path or URL passed to [`fetch_pack_index`].
        path: String,
        /// Underlying JSON decode error.
        #[source]
        source: serde_json::Error,
    },
    /// No artifact was published for the requested host target.
    #[error(
        "language {language_id} version {version} has no artifact for host target {host_target}"
    )]
    MissingTarget {
        /// Language id being resolved.
        language_id: LanguageId,
        /// Version being resolved.
        version: String,
        /// Host target triple requested by the caller.
        host_target: String,
    },
}

/// Loads a pack-index manifest list from a `file://`, `http://`, or `https://` URL.
pub fn fetch_pack_index(url: &str) -> Result<Vec<RegistryManifest>, RegistryError> {
    if url.starts_with("file://") {
        return fetch_file_pack_index(url);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return fetch_remote_pack_index(url, Duration::from_secs(30));
    }
    Err(RegistryError::UnsupportedScheme {
        url: url.to_owned(),
    })
}

/// Selects the target artifact for the current host triple.
pub fn select_target_artifact<'a>(
    manifest: &'a RegistryManifest,
    host_target: &str,
) -> Result<&'a RegistryArtifact, RegistryError> {
    manifest
        .targets
        .get(host_target)
        .ok_or_else(|| RegistryError::MissingTarget {
            language_id: manifest.id.clone(),
            version: manifest.version.clone(),
            host_target: host_target.to_owned(),
        })
}

fn fetch_file_pack_index(url: &str) -> Result<Vec<RegistryManifest>, RegistryError> {
    let path = file_url_to_path(url)?;
    let path_display = path.display().to_string();
    let contents = fs::read_to_string(&path).map_err(|source| RegistryError::Read {
        path: path_display.clone(),
        source,
    })?;

    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|source| RegistryError::MalformedJson {
            path: path_display.clone(),
            source,
        })?;

    serde_json::from_value(value).map_err(|source| RegistryError::InvalidManifest {
        path: path_display,
        source,
    })
}

fn fetch_remote_pack_index(
    url: &str,
    timeout: Duration,
) -> Result<Vec<RegistryManifest>, RegistryError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|source| RegistryError::HttpRequest {
            url: url.to_owned(),
            source,
        })?;

    let response = client.get(url).send().map_err(|source| {
        if source.is_timeout() {
            RegistryError::Timeout {
                url: url.to_owned(),
            }
        } else {
            RegistryError::HttpRequest {
                url: url.to_owned(),
                source,
            }
        }
    })?;

    if !response.status().is_success() {
        return Err(RegistryError::HttpStatus {
            url: url.to_owned(),
            status: response.status(),
        });
    }

    let body = response.text().map_err(|source| {
        if source.is_timeout() {
            RegistryError::Timeout {
                url: url.to_owned(),
            }
        } else {
            RegistryError::HttpRequest {
                url: url.to_owned(),
                source,
            }
        }
    })?;

    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|source| RegistryError::MalformedRemoteJson {
            url: url.to_owned(),
            source,
        })?;

    serde_json::from_value(value).map_err(|source| RegistryError::InvalidManifest {
        path: url.to_owned(),
        source,
    })
}

fn file_url_to_path(url: &str) -> Result<PathBuf, RegistryError> {
    let Some(rest) = url.strip_prefix("file://") else {
        return Err(RegistryError::UnsupportedScheme {
            url: url.to_owned(),
        });
    };

    let path_part = if rest.starts_with('/') {
        rest.to_owned()
    } else {
        let Some((host, path)) = rest.split_once('/') else {
            return Err(RegistryError::InvalidFileUrl {
                url: url.to_owned(),
                reason: "missing absolute path",
            });
        };

        if host != "localhost" {
            return Err(RegistryError::InvalidFileUrl {
                url: url.to_owned(),
                reason: "only empty host or localhost are supported",
            });
        }

        format!("/{path}")
    };

    if !path_part.starts_with('/') {
        return Err(RegistryError::InvalidFileUrl {
            url: url.to_owned(),
            reason: "path must be absolute",
        });
    }

    let decoded = percent_decode(&path_part).map_err(|reason| RegistryError::InvalidFileUrl {
        url: url.to_owned(),
        reason,
    })?;

    Ok(PathBuf::from(decoded))
}

fn percent_decode(input: &str) -> Result<String, &'static str> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("incomplete percent-encoding");
                }
                let hi = from_hex(bytes[i + 1]).ok_or("invalid percent-encoding")?;
                let lo = from_hex(bytes[i + 2]).ok_or("invalid percent-encoding")?;
                out.push(hi << 4 | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(out).map_err(|_| "invalid UTF-8 in decoded path")
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_file_url() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let fixture = root
            .join("fixtures")
            .join("registry")
            .join("official-manifest.json");

        format!("file://{}", fixture.display())
    }

    fn alpha_fixture_file_url() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let fixture = root
            .join("fixtures")
            .join("registry")
            .join("alpha-index.json");

        format!("file://{}", fixture.display())
    }

    #[test]
    fn parses_manifest_entry_from_fixture() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");
        assert_eq!(manifests.len(), 2);

        let entry = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "compose")
            .expect("compose entry should exist");

        assert_eq!(entry.version, "0.1.0-alpha.0");
        assert_eq!(entry.api_version, 1);

        let linux = entry
            .targets
            .get("x86_64-unknown-linux-gnu")
            .expect("linux artifact should exist");
        assert_eq!(
            linux.url,
            "https://github.com/Daio-io/wax/releases/latest/download/wax-lang-compose-0.1.0-alpha.0-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            linux.sha256,
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn official_manifest_urls_stay_aligned_with_alpha_index() {
        let official = fetch_pack_index(&fixture_file_url()).expect("official fixture parses");
        let alpha = fetch_pack_index(&alpha_fixture_file_url()).expect("alpha fixture parses");

        for official_manifest in official {
            let alpha_manifest = alpha
                .iter()
                .find(|manifest| manifest.id == official_manifest.id)
                .expect("official fixture ids should exist in alpha index");

            assert_eq!(alpha_manifest.version, official_manifest.version);
            assert_eq!(alpha_manifest.api_version, official_manifest.api_version);

            for (target, official_artifact) in official_manifest.targets {
                let alpha_artifact = alpha_manifest
                    .targets
                    .get(&target)
                    .expect("official fixture targets should exist in alpha index");
                assert_eq!(alpha_artifact.url, official_artifact.url);
            }
        }
    }

    #[test]
    #[ignore = "post-release smoke hits the published default pack index"]
    fn fetches_published_default_pack_index() {
        let manifests = fetch_pack_index(crate::defaults::DEFAULT_WAX_LANG_INDEX)
            .expect("default published pack index should fetch and parse");
        let expected_release_tag = std::env::var("WAX_EXPECTED_RELEASE_TAG").ok();

        assert_alpha_index_matches_release(&manifests, expected_release_tag.as_deref())
            .expect("published default pack index should match the current release");
    }

    #[test]
    #[ignore = "release workflow provides WAX_PACK_INDEX_URL and WAX_EXPECTED_RELEASE_TAG"]
    fn validates_pack_index_from_env() {
        let index_url = std::env::var("WAX_PACK_INDEX_URL")
            .expect("WAX_PACK_INDEX_URL should point at generated index.json");
        let expected_release_tag = std::env::var("WAX_EXPECTED_RELEASE_TAG")
            .expect("WAX_EXPECTED_RELEASE_TAG should name the current release tag");
        let manifests = fetch_pack_index(&index_url).expect("pack index should fetch and parse");

        assert_alpha_index_matches_release(&manifests, Some(&expected_release_tag))
            .expect("pack index should match the current release");
    }

    #[test]
    fn current_release_validation_rejects_stale_index() {
        let manifests = registry_manifests_for_release("v0.1.0-alpha.0");
        let err = assert_alpha_index_matches_release(&manifests, Some("v0.1.0-alpha.1"))
            .expect_err("stale index should be rejected");

        assert!(
            err.contains("version"),
            "expected version mismatch, got: {err}"
        );
    }

    #[test]
    fn current_release_validation_accepts_matching_index() {
        let manifests = registry_manifests_for_release("v0.1.0-alpha.1");
        assert_alpha_index_matches_release(&manifests, Some("v0.1.0-alpha.1"))
            .expect("matching release index should pass");
    }

    #[test]
    fn current_release_validation_rejects_extra_target() {
        let mut manifests = registry_manifests_for_release("v0.1.0-alpha.1");
        manifests[0].targets.insert(
            "wasm32-unknown-unknown".to_owned(),
            RegistryArtifact {
                url: "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-wasm32-unknown-unknown.tar.gz".to_owned(),
                sha256: "3333333333333333333333333333333333333333333333333333333333333333".to_owned(),
            },
        );

        let err = assert_alpha_index_matches_release(&manifests, Some("v0.1.0-alpha.1"))
            .expect_err("extra target should be rejected");

        assert!(
            err.contains("targets"),
            "expected target set mismatch, got: {err}"
        );
    }

    #[test]
    fn current_release_validation_rejects_wrong_artifact_host() {
        let mut manifests = registry_manifests_for_release("v0.1.0-alpha.1");
        manifests[0]
            .targets
            .get_mut("x86_64-unknown-linux-gnu")
            .expect("target should exist")
            .url = "https://example.invalid/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz".to_owned();

        let err = assert_alpha_index_matches_release(&manifests, Some("v0.1.0-alpha.1"))
            .expect_err("wrong artifact host should be rejected");

        assert!(err.contains("URL"), "expected URL mismatch, got: {err}");
    }

    fn registry_manifests_for_release(release_tag: &str) -> Vec<RegistryManifest> {
        let version = release_tag.trim_start_matches('v');
        let json = format!(
            r#"
[
  {{
    "id": "compose",
    "version": "{version}",
    "api_version": 1,
    "targets": {{
      "x86_64-unknown-linux-gnu": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-compose-{version}-x86_64-unknown-linux-gnu.tar.gz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }},
      "aarch64-apple-darwin": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-compose-{version}-aarch64-apple-darwin.tar.gz",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      }},
      "x86_64-apple-darwin": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-compose-{version}-x86_64-apple-darwin.tar.gz",
        "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
      }},
      "aarch64-unknown-linux-gnu": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-compose-{version}-aarch64-unknown-linux-gnu.tar.gz",
        "sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
      }}
    }}
  }},
  {{
    "id": "basic",
    "version": "{version}",
    "api_version": 1,
    "targets": {{
      "x86_64-unknown-linux-gnu": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-basic-{version}-x86_64-unknown-linux-gnu.tar.gz",
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      }},
      "aarch64-apple-darwin": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-basic-{version}-aarch64-apple-darwin.tar.gz",
        "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
      }},
      "x86_64-apple-darwin": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-basic-{version}-x86_64-apple-darwin.tar.gz",
        "sha256": "1111111111111111111111111111111111111111111111111111111111111111"
      }},
      "aarch64-unknown-linux-gnu": {{
        "url": "https://github.com/Daio-io/wax/releases/download/{release_tag}/wax-lang-basic-{version}-aarch64-unknown-linux-gnu.tar.gz",
        "sha256": "2222222222222222222222222222222222222222222222222222222222222222"
      }}
    }}
  }}
]
"#
        );
        serde_json::from_str(&json).expect("synthetic manifest should parse")
    }

    fn assert_alpha_index_matches_release(
        manifests: &[RegistryManifest],
        expected_release_tag: Option<&str>,
    ) -> Result<(), String> {
        let ids: Vec<_> = manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect();
        if ids != ["compose", "basic"] {
            return Err(format!("expected compose/basic only, got {ids:?}"));
        }

        let Some(expected_release_tag) = expected_release_tag else {
            return Ok(());
        };
        let expected_version = expected_release_tag.trim_start_matches('v');
        let expected_targets = [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
        ];

        for manifest in manifests {
            assert_eq!(manifest.api_version, 1);
            let targets: Vec<_> = manifest.targets.keys().map(String::as_str).collect();
            if targets != expected_targets {
                return Err(format!(
                    "pack {} targets {:?} did not match expected {:?}",
                    manifest.id, targets, expected_targets
                ));
            }
            if manifest.version != expected_version {
                return Err(format!(
                    "pack {} version {} did not match expected release version {}",
                    manifest.id, manifest.version, expected_version
                ));
            }
            let binary = format!("wax-lang-{}", manifest.id);
            for (target, artifact) in &manifest.targets {
                let expected_asset = format!("{binary}-{expected_version}-{target}.tar.gz");
                let expected_url = format!(
                    "https://github.com/Daio-io/wax/releases/download/{expected_release_tag}/{expected_asset}"
                );
                if artifact.url != expected_url {
                    return Err(format!(
                        "pack {} target {} URL {} did not match {}",
                        manifest.id, target, artifact.url, expected_url
                    ));
                }
            }
        }

        Ok(())
    }

    #[test]
    fn rejects_unsupported_registry_url_scheme() {
        let err = fetch_pack_index("s3://registry.example.dev/index.json")
            .expect_err("unsupported URL should be rejected");
        assert!(matches!(err, RegistryError::UnsupportedScheme { .. }));
    }

    #[test]
    fn fetches_manifest_entry_from_http() {
        let body = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("fixtures")
                .join("registry")
                .join("official-manifest.json"),
        )
        .expect("fixture should load");
        let url = serve_one_response("HTTP/1.1 200 OK", "application/json", &body);

        let manifests = fetch_pack_index(&url).expect("http manifest should parse");
        assert_eq!(manifests.len(), 2);
    }

    #[test]
    fn returns_typed_http_status_error() {
        let url = serve_one_response("HTTP/1.1 404 Not Found", "text/plain", "missing");
        let err = fetch_pack_index(&url).expect_err("404 should fail");
        assert!(matches!(
            err,
            RegistryError::HttpStatus { status, .. } if status == reqwest::StatusCode::NOT_FOUND
        ));
    }

    #[test]
    fn returns_typed_timeout_error() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind should work");
        let addr = listener.local_addr().expect("local addr should resolve");
        let handle = thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept should work");
            thread::sleep(Duration::from_millis(100));
        });

        let url = format!("http://{addr}/index.json");
        let err = fetch_remote_pack_index(&url, Duration::from_millis(10))
            .expect_err("slow response should time out");
        assert!(matches!(err, RegistryError::Timeout { .. }));
        handle.join().expect("thread should join");
    }

    #[test]
    fn returns_typed_malformed_remote_json_error() {
        let url = serve_one_response("HTTP/1.1 200 OK", "application/json", "{bad-json");
        let err = fetch_pack_index(&url).expect_err("invalid JSON should fail");
        assert!(matches!(err, RegistryError::MalformedRemoteJson { .. }));
    }

    #[test]
    fn selects_target_artifact_for_host_triple() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");
        let compose_manifest = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "compose")
            .expect("compose entry should exist");

        let artifact = select_target_artifact(compose_manifest, "aarch64-apple-darwin")
            .expect("darwin artifact should exist");

        assert_eq!(
            artifact.url,
            "https://github.com/Daio-io/wax/releases/latest/download/wax-lang-compose-0.1.0-alpha.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            artifact.sha256,
            "2222222222222222222222222222222222222222222222222222222222222222"
        );
    }

    #[test]
    fn returns_typed_error_when_target_missing() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");
        let basic_manifest = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "basic")
            .expect("basic entry should exist");

        let err = select_target_artifact(basic_manifest, "x86_64-pc-windows-msvc")
            .expect_err("missing host target should return an error");

        assert!(matches!(
            err,
            RegistryError::MissingTarget {
                language_id,
                version,
                host_target
            } if language_id.as_str() == "basic" && version == "0.1.0-alpha.0" && host_target == "x86_64-pc-windows-msvc"
        ));
    }

    #[test]
    fn accepts_file_localhost_url() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("registry")
            .join("official-manifest.json");
        let url = format!("file://localhost{}", fixture.display());

        let manifests = fetch_pack_index(&url).expect("localhost file URL should parse");
        assert_eq!(manifests.len(), 2);
    }

    #[test]
    fn decodes_percent_encoded_utf8_path() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wax-registry-{nonce}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");

        let file_path = dir.join("manifést.json");
        fs::write(&file_path, "[]").expect("temp fixture should be written");

        let encoded_url = format!("file://{}/manif%C3%A9st.json", dir.display());

        let manifests = fetch_pack_index(&encoded_url).expect("utf-8 path should decode");
        assert!(manifests.is_empty());

        fs::remove_file(file_path).expect("temp fixture should be removed");
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn rejects_manifest_with_invalid_language_id() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wax-registry-{nonce}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");

        let file_path = dir.join("invalid-language-id.json");
        let manifest = r#"
[
  {
    "id": "React",
    "version": "1.0.0",
    "api_version": 1,
    "targets": {
      "x86_64-unknown-linux-gnu": {
        "url": "https://registry.example.dev/react/1.0.0/react-linux.tar.zst",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }
    }
  }
]
"#;
        fs::write(&file_path, manifest).expect("temp fixture should be written");

        let url = format!("file://{}", file_path.display());
        let err = fetch_pack_index(&url).expect_err("invalid language id should fail");
        assert!(matches!(err, RegistryError::InvalidManifest { .. }));

        fs::remove_file(file_path).expect("temp fixture should be removed");
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    fn serve_one_response(status_line: &str, content_type: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind should work");
        let addr = listener.local_addr().expect("local addr should resolve");
        let status_line = status_line.to_owned();
        let content_type = content_type.to_owned();
        let body = body.to_owned();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept should work");
            drain_request(&mut stream);
            let response = format!(
                "{status_line}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("response write should work");
        });

        format!("http://{addr}/index.json")
    }

    fn drain_request(stream: &mut TcpStream) {
        let mut buf = [0_u8; 1024];
        let _ = stream.read(&mut buf);
    }
}
