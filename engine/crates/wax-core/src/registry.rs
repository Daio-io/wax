//! Registry index loading and target artifact selection for language packs.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
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
    #[error("unsupported registry URL scheme in {url}; only file:// is supported")]
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
    /// Registry index contains malformed JSON.
    #[error("malformed registry manifest index JSON in {path}: {source}")]
    MalformedJson {
        /// Filesystem path derived from the provided URL.
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// Registry index JSON does not match the expected manifest shape.
    #[error("invalid registry manifest index in {path}: {source}")]
    InvalidManifest {
        /// Filesystem path derived from the provided URL.
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

/// Loads a pack-index manifest list from a `file://` URL.
pub fn fetch_pack_index(url: &str) -> Result<Vec<RegistryManifest>, RegistryError> {
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

    let manifests =
        serde_json::from_value(value).map_err(|source| RegistryError::InvalidManifest {
            path: path_display,
            source,
        })?;

    Ok(manifests)
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
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_file_url() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let fixture = root
            .join("fixtures")
            .join("registry")
            .join("official-manifest.json");

        format!("file://{}", fixture.display())
    }

    #[test]
    fn parses_manifest_entry_from_fixture() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");

        assert_eq!(manifests.len(), 2);

        let entry = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "rust")
            .expect("rust entry should exist");

        assert_eq!(entry.version, "1.2.3");
        assert_eq!(entry.api_version, 1);

        let linux = entry
            .targets
            .get("x86_64-unknown-linux-gnu")
            .expect("linux artifact should exist");
        assert_eq!(
            linux.url,
            "https://registry.example.dev/rust/1.2.3/rust-linux.tar.zst"
        );
        assert_eq!(
            linux.sha256,
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn rejects_non_file_registry_url() {
        let err = fetch_pack_index("https://registry.example.dev/index.json")
            .expect_err("https URL should be rejected in unit tests");

        assert!(matches!(err, RegistryError::UnsupportedScheme { .. }));
    }

    #[test]
    fn selects_target_artifact_for_host_triple() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");
        let rust_manifest = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "rust")
            .expect("rust entry should exist");

        let artifact = select_target_artifact(rust_manifest, "aarch64-apple-darwin")
            .expect("darwin artifact should exist");

        assert_eq!(
            artifact.url,
            "https://registry.example.dev/rust/1.2.3/rust-macos.tar.zst"
        );
        assert_eq!(
            artifact.sha256,
            "2222222222222222222222222222222222222222222222222222222222222222"
        );
    }

    #[test]
    fn returns_typed_error_when_target_missing() {
        let manifests = fetch_pack_index(&fixture_file_url()).expect("fixture should parse");
        let go_manifest = manifests
            .iter()
            .find(|manifest| manifest.id.as_str() == "go")
            .expect("go entry should exist");

        let err = select_target_artifact(go_manifest, "x86_64-pc-windows-msvc")
            .expect_err("missing host target should return an error");

        assert!(matches!(
            err,
            RegistryError::MissingTarget {
                language_id,
                version,
                host_target
            } if language_id.as_str() == "go" && version == "0.9.0" && host_target == "x86_64-pc-windows-msvc"
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
}
