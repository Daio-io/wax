use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use wax_contract::LanguageId;
use wax_core::config::waxrc::{
    DesignSystemConfig, DesignSystemRegistry, LanguageRegistrySource, WaxRcError, load_waxrc,
};

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
            "wax-core-config-v2-{name}-{}-{nonce}.json",
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

#[test]
fn config_v2_parses_language_map_and_design_systems() {
    let file = TestFile::new(
        "full",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  },
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json",
          "published_source": "https://cdn.example.com/acme/react.registry.json"
        }
      }
    }
  }
}"#,
    );

    let rc = load_waxrc(file.path()).unwrap();

    assert_eq!(rc.schema_version, 2);
    assert_eq!(rc.engine.scan_concurrency, 2);
    assert!(rc.adoption.track_local_invocations);
    assert_eq!(rc.languages.len(), 1);

    let language = &rc.languages[0];
    assert_eq!(language.id.as_str(), "react");
    assert_eq!(language.roots, ["src"]);
    assert_eq!(
        language.registry_source.as_ref().unwrap(),
        &LanguageRegistrySource::PathOrUrl {
            source: ".wax/registries/acme/react.json".to_owned(),
            upstream: Some("acme/react".to_owned()),
        }
    );
    assert!(language.extra.is_empty());

    let mut expected_design_systems = BTreeMap::new();
    expected_design_systems.insert(
        "acme".to_owned(),
        DesignSystemConfig {
            name: "Acme Design System".to_owned(),
            registries: BTreeMap::from([(
                LanguageId::try_from("react").unwrap(),
                DesignSystemRegistry {
                    source: ".wax/registries/react.json".to_owned(),
                    published_source: Some(
                        "https://cdn.example.com/acme/react.registry.json".to_owned(),
                    ),
                },
            )]),
        },
    );
    assert_eq!(rc.design_systems, expected_design_systems);
}

#[test]
fn config_v2_preserves_pack_specific_extra_fields() {
    let file = TestFile::new(
        "extra",
        r#"{
  "schema_version": 2,
  "languages": {
    "basic": {
      "roots": ["app/src"],
      "registry": "design-system/registry.json",
      "file_extensions": [".kt", ".kts"]
    }
  }
}"#,
    );

    let rc = load_waxrc(file.path()).unwrap();
    let language = &rc.languages[0];

    assert_eq!(language.id.as_str(), "basic");
    assert_eq!(language.roots, ["app/src"]);
    assert_eq!(
        language
            .registry_source
            .as_ref()
            .unwrap()
            .path_or_url_parts()
            .unwrap()
            .0,
        "design-system/registry.json"
    );
    assert_eq!(
        language.extra["file_extensions"],
        serde_json::json!([".kt", ".kts"])
    );
    assert!(!language.extra.contains_key("roots"));
    assert!(!language.extra.contains_key("registry"));
}

#[test]
fn config_v2_parses_git_registry() {
    let file = TestFile::new(
        "git-registry",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "registry": {
        "git": "git@github.com:acme/design-system.git",
        "tag": "v2.4.1"
      }
    }
  }
}"#,
    );

    assert_eq!(
        load_waxrc(file.path()).unwrap().languages[0].registry_source,
        Some(LanguageRegistrySource::Git {
            git: "git@github.com:acme/design-system.git".to_owned(),
            tag: "v2.4.1".to_owned(),
        })
    );
}

#[test]
fn config_v2_accepts_git_urls_and_commit_tags() {
    for (git, tag) in [
        ("https://github.com/acme/design-system.git", "release"),
        (
            "git@github.com:acme/design-system.git",
            "0123456789abcdef0123456789abcdef01234567",
        ),
    ] {
        let contents = serde_json::json!({
            "schema_version": 2,
            "languages": { "react": { "registry": { "git": git, "tag": tag } } }
        })
        .to_string();
        let file = TestFile::new("git-opaque-values", &contents);
        assert!(matches!(
            load_waxrc(file.path()).unwrap().languages[0].registry_source,
            Some(LanguageRegistrySource::Git { .. })
        ));
    }
}

#[test]
fn config_v2_rejects_invalid_git_registry_shapes() {
    for (name, registry, expected) in [
        ("missing-git", r#"{"tag":"v1"}"#, "git is required"),
        (
            "missing-tag",
            r#"{"git":"https://example.com/repo.git"}"#,
            "tag is required",
        ),
        (
            "empty-git",
            r#"{"git":"","tag":"v1"}"#,
            "git must be a non-empty string",
        ),
        (
            "blank-tag",
            r#"{"git":"repo","tag":"   "}"#,
            "tag must be a non-empty string",
        ),
        (
            "null-git",
            r#"{"git":null,"tag":"v1"}"#,
            "git cannot be null",
        ),
        (
            "null-tag",
            r#"{"git":"repo","tag":null}"#,
            "tag cannot be null",
        ),
        (
            "mixed-source",
            r#"{"git":"repo","tag":"v1","source":"x"}"#,
            "cannot mix `git` with `source`",
        ),
        (
            "mixed-upstream",
            r#"{"git":"repo","tag":"v1","upstream":"acme/react"}"#,
            "cannot mix `tag` with `upstream`",
        ),
        (
            "unexpected-path",
            r#"{"git":"repo","tag":"v1","path":"x"}"#,
            "unknown field `path`",
        ),
    ] {
        let file = TestFile::new(
            name,
            &format!(r#"{{"schema_version":2,"languages":{{"react":{{"registry":{registry}}}}}}}"#),
        );
        let error = load_waxrc(file.path()).unwrap_err().to_string();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn config_v2_allows_design_systems_without_languages() {
    let file = TestFile::new(
        "design-system-only",
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json"
        }
      }
    }
  }
}"#,
    );

    let rc = load_waxrc(file.path()).unwrap();

    assert!(rc.languages.is_empty());
    assert_eq!(rc.design_systems["acme"].name, "Acme Design System");
    assert_eq!(
        rc.design_systems["acme"].registries[&LanguageId::try_from("react").unwrap()].source,
        ".wax/registries/react.json"
    );
    assert!(
        rc.design_systems["acme"].registries[&LanguageId::try_from("react").unwrap()]
            .published_source
            .is_none()
    );
}

#[test]
fn config_v2_rejects_schema_version_1() {
    let file = TestFile::new(
        "v1",
        r#"{
  "schema_version": 1,
  "languages": {
    "react": {
      "roots": ["src"]
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(
        err,
        WaxRcError::UnsupportedSchemaVersion {
            found: 1,
            supported: 2,
            ..
        }
    ));
}

#[test]
fn config_v2_rejects_invalid_design_system_id() {
    let file = TestFile::new(
        "invalid-design-system-id",
        r#"{
  "schema_version": 2,
  "design_systems": {
    "Acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json"
        }
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("design_systems"));
}

#[test]
fn config_v2_rejects_empty_design_system_name() {
    let file = TestFile::new(
        "empty-design-system-name",
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json"
        }
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("design_systems.acme.name"));
}

#[test]
fn config_v2_rejects_empty_design_system_registry_source() {
    let file = TestFile::new(
        "empty-design-system-source",
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ""
        }
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(
        err.to_string()
            .contains("design_systems.acme.registries.react.source")
    );
}

#[test]
fn config_v2_rejects_unknown_registry_fields() {
    let file = TestFile::new(
        "unknown-registry-field",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/react.json",
        "upsteam": "acme/react"
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("unknown field"));
    assert!(err.to_string().contains("upsteam"));
}

#[test]
fn config_v2_rejects_null_registry() {
    let file = TestFile::new(
        "null-registry",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": null
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("registry cannot be null"));
}

#[test]
fn config_v2_rejects_null_registry_upstream() {
    let file = TestFile::new(
        "null-upstream",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/react.json",
        "upstream": null
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("upstream cannot be null"));
}

#[test]
fn config_v2_rejects_null_published_source() {
    let file = TestFile::new(
        "null-published-source",
        r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json",
          "published_source": null
        }
      }
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("published_source cannot be null"));
}

#[test]
fn config_v2_rejects_design_system_registry_field() {
    let file = TestFile::new(
        "legacy-registry",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "design_system_registry": "design-system/registry.json"
    }
  }
}"#,
    );

    let err = load_waxrc(file.path()).unwrap_err();
    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("design_system_registry"));
}

#[test]
fn token_inference_defaults_numeric_tolerance_to_two() {
    let minimal = TestFile::new(
        "token-inference-default",
        r#"{
  "schema_version": 2,
  "languages": {
    "react": { "roots": ["src"] }
  }
}"#,
    );

    assert_eq!(
        load_waxrc(minimal.path())
            .unwrap()
            .token_inference
            .numeric_tolerance,
        2.0
    );
}

#[test]
fn token_inference_parses_custom_and_zero_numeric_tolerance() {
    let custom = TestFile::new(
        "token-inference-custom",
        r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": 0.5 },
  "languages": {
    "react": { "roots": ["src"] }
  }
}"#,
    );
    let exact_only = TestFile::new(
        "token-inference-zero",
        r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": 0 },
  "languages": {
    "react": { "roots": ["src"] }
  }
}"#,
    );

    assert_eq!(
        load_waxrc(custom.path())
            .unwrap()
            .token_inference
            .numeric_tolerance,
        0.5
    );
    assert_eq!(
        load_waxrc(exact_only.path())
            .unwrap()
            .token_inference
            .numeric_tolerance,
        0.0
    );
}

#[test]
fn token_inference_rejects_invalid_numeric_tolerance_and_unknown_keys() {
    for (name, contents) in [
        (
            "token-inference-negative",
            r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": -1 },
  "languages": { "react": { "roots": ["src"] } }
}"#,
        ),
        (
            "token-inference-null",
            r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": null },
  "languages": { "react": { "roots": ["src"] } }
}"#,
        ),
        (
            "token-inference-string",
            r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": "2" },
  "languages": { "react": { "roots": ["src"] } }
}"#,
        ),
        (
            "token-inference-unknown-key",
            r#"{
  "schema_version": 2,
  "token_inference": { "numeric_tolerance": 2, "extra": true },
  "languages": { "react": { "roots": ["src"] } }
}"#,
        ),
    ] {
        let file = TestFile::new(name, contents);
        let err = load_waxrc(file.path()).unwrap_err();
        assert!(
            matches!(err, WaxRcError::InvalidConfig { .. }),
            "{name}: {err}"
        );
    }
}
