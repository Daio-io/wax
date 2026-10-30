#![cfg(unix)]

use serde_json::{Map, Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use wax_contract::{
    REGISTRY_SCHEMA_VERSION, RegistryComponent, RegistryDocument, RegistryMetadata, RegistryToken,
    TokenCategory,
};
use wax_core::subprocess_discover::SubprocessLanguageDiscoverer;
use wax_core::subprocess_lang::SubprocessLanguageManifest;
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRequest, DiscoverRequestType, DiscoveredRegistrySymbol,
    WIRE_API_VERSION,
};

#[test]
fn replacement_discovery_keeps_only_observed_components_and_preserves_tokens() {
    let temp_dir = TestDir::new("replacement-current-symbols");
    let requests_path = temp_dir.path().join("requests.jsonl");
    let script_path = temp_dir.path().join("mock-replacement-pack.sh");
    write_replacement_script(&script_path);
    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            requests_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry: replacement_registry(),
    };

    discoverer.discover_replacement_registry(request).unwrap();

    let requests = fs::read_to_string(requests_path).unwrap();
    let rich_request: Value = serde_json::from_str(requests.lines().nth(2).unwrap()).unwrap();
    assert_eq!(
        rich_request["registry"],
        json!({
            "schema_version": REGISTRY_SCHEMA_VERSION,
            "components": [{
                "id": "primary-button",
                "symbol": "PrimaryButton",
                "package": "com.acme.designsystem",
            }],
            "tokens": [{
                "id": "color.brand.primary",
                "key": "BrandPrimary",
                "category": "color",
            }],
            "owner": "design-systems",
        })
    );
}

#[test]
fn replacement_discovery_preserves_unique_authored_identity_and_alias_match() {
    let temp_dir = TestDir::new("replacement-alias-match");
    let requests_path = temp_dir.path().join("requests.jsonl");
    let script_path = temp_dir.path().join("mock-replacement-pack.sh");
    write_replacement_script(&script_path);
    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            requests_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });
    let mut registry = replacement_registry();
    registry.components[0] = RegistryComponent {
        id: "component.primary-action".to_owned(),
        symbol: "PrimaryAction".to_owned(),
        package: Some("com.acme.designsystem".to_owned()),
        aliases: vec!["PrimaryButton".to_owned()],
        metadata: Some(RegistryMetadata {
            description: Some("Use for the main action".to_owned()),
            tags: vec!["stable".to_owned()],
            sources: Vec::new(),
            extra: Map::new(),
        }),
        details: None,
        extra: Map::new(),
    };
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry,
    };

    discoverer.discover_replacement_registry(request).unwrap();

    let requests = fs::read_to_string(requests_path).unwrap();
    let rich_request: Value = serde_json::from_str(requests.lines().nth(2).unwrap()).unwrap();
    assert_eq!(
        rich_request["registry"]["components"],
        json!([{
            "id": "component.primary-action",
            "symbol": "PrimaryAction",
            "package": "com.acme.designsystem",
            "aliases": ["PrimaryButton"],
            "metadata": {
                "description": "Use for the main action",
                "tags": ["stable"],
            },
        }])
    );
}

#[test]
fn replacement_discovery_does_not_copy_ambiguous_authored_metadata() {
    let temp_dir = TestDir::new("replacement-ambiguous-match");
    let requests_path = temp_dir.path().join("requests.jsonl");
    let script_path = temp_dir.path().join("mock-replacement-pack.sh");
    write_replacement_script(&script_path);
    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            requests_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });
    let ambiguous_component = |id: &str, description: &str| RegistryComponent {
        id: id.to_owned(),
        symbol: format!("{id}Canonical"),
        package: Some("com.acme.designsystem".to_owned()),
        aliases: vec!["PrimaryButton".to_owned()],
        metadata: Some(RegistryMetadata {
            description: Some(description.to_owned()),
            tags: Vec::new(),
            sources: Vec::new(),
            extra: Map::new(),
        }),
        details: None,
        extra: Map::new(),
    };
    let mut registry = replacement_registry();
    registry.components = vec![
        ambiguous_component("first", "First metadata"),
        ambiguous_component("second", "Second metadata"),
    ];
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry,
    };

    discoverer.discover_replacement_registry(request).unwrap();

    let requests = fs::read_to_string(requests_path).unwrap();
    let rich_request: Value = serde_json::from_str(requests.lines().nth(2).unwrap()).unwrap();
    assert_eq!(
        rich_request["registry"]["components"],
        json!([{
            "id": "ds.primary-button",
            "symbol": "PrimaryButton",
            "package": "com.acme.designsystem",
        }])
    );
}

#[test]
fn subprocess_discover_parses_discover_symbols_response() {
    let temp_dir = TestDir::new("discover-success");
    let request_path = temp_dir.path().join("request.json");
    let script_path = temp_dir.path().join("mock-discover-pack.sh");
    write_discover_script(&script_path);

    let extractor = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            request_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });

    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let result = extractor.discover(request).unwrap();

    assert_eq!(
        DiscoveredRegistrySymbol::symbol_names(&result.components),
        vec!["PrimaryButton".to_owned()]
    );
    assert_eq!(
        result.components[0].package.as_deref(),
        Some("com.acme.designsystem")
    );
    assert!(result.diagnostics.is_empty());

    let request_json: Value =
        serde_json::from_str(&fs::read_to_string(&request_path).unwrap()).unwrap();
    assert_eq!(
        request_json,
        json!({
            "type": "discover",
            "api_version": WIRE_API_VERSION,
            "language_id": "compose",
            "repo_root": "/tmp/repo",
            "roots": ["design-system/src/main/kotlin"],
        })
    );
}

fn write_discover_script(script_path: &Path) {
    write_script(
        script_path,
        &format!(
            r#"#!/bin/sh
cat > "$1"
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "discover_symbols",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "symbols": ["PrimaryButton"],
                "components": [{
                    "symbol": "PrimaryButton",
                    "package": "com.acme.designsystem"
                }],
                "diagnostics": [],
            }))
            .unwrap()
        ),
    );
}

fn write_replacement_script(script_path: &Path) {
    write_script(
        script_path,
        &format!(
            r#"#!/bin/sh
request=$(cat)
printf '%s\n' "$request" >> "$1"
case "$request" in
  *'"type":"discover"'*)
    printf '%s\n' '{}'
    ;;
  *'"type":"capabilities"'*)
    printf '%s\n' '{}'
    ;;
  *)
    printf '%s\n' '{}'
    ;;
esac
"#,
            serde_json::to_string(&json!({
                "type": "discover_symbols",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "symbols": ["PrimaryButton"],
                "components": [{
                    "symbol": "PrimaryButton",
                    "package": "com.acme.designsystem",
                }],
                "diagnostics": [],
            }))
            .unwrap(),
            serde_json::to_string(&json!({
                "type": "capabilities",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "registry_details_v1": true,
            }))
            .unwrap(),
            serde_json::to_string(&json!({
                "type": "registry_details",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "document": {
                    "schema_version": REGISTRY_SCHEMA_VERSION,
                    "components": [{
                        "id": "primary-button",
                        "symbol": "PrimaryButton",
                    }],
                    "tokens": [{
                        "id": "color.brand.primary",
                        "key": "BrandPrimary",
                        "category": "color",
                    }],
                    "owner": "design-systems",
                },
                "diagnostics": [],
            }))
            .unwrap(),
        ),
    );
}

fn replacement_registry() -> RegistryDocument {
    RegistryDocument {
        schema_version: REGISTRY_SCHEMA_VERSION,
        components: vec![
            RegistryComponent {
                id: "primary-button".to_owned(),
                symbol: "PrimaryButton".to_owned(),
                package: Some("com.acme.designsystem".to_owned()),
                aliases: Vec::new(),
                metadata: None,
                details: None,
                extra: Map::new(),
            },
            RegistryComponent {
                id: "legacy-control".to_owned(),
                symbol: "LegacyControl".to_owned(),
                package: None,
                aliases: Vec::new(),
                metadata: None,
                details: None,
                extra: Map::new(),
            },
        ],
        tokens: vec![RegistryToken {
            id: "color.brand.primary".to_owned(),
            key: "BrandPrimary".to_owned(),
            category: TokenCategory::Color,
            aliases: Vec::new(),
            value: None,
            metadata: None,
            details: None,
            extra: Map::new(),
        }],
        extra: Map::from_iter([("owner".to_owned(), json!("design-systems"))]),
    }
}

fn write_script(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("wax-core-subprocess-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
