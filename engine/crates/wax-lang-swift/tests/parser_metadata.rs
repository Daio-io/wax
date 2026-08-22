use std::fs;
use std::path::Path;

use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_swift::{SwiftLanguage, TREE_SITTER_SWIFT_GRAMMAR_VERSION};

#[test]
fn parser_metadata_matches_dependency_and_scan_facts() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("crate manifest should be readable");
    let manifest: toml::Value =
        toml::from_str(&manifest).expect("crate manifest should be valid TOML");
    let dependency = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("tree-sitter-swift"))
        .expect("tree-sitter-swift dependency should be declared");
    let dependency_version = dependency
        .get("version")
        .and_then(toml::Value::as_str)
        .or_else(|| dependency.as_str())
        .expect("tree-sitter-swift dependency should specify a version");
    let dependency_version = dependency_version
        .strip_prefix('=')
        .unwrap_or(dependency_version);

    assert_eq!(
        dependency_version, TREE_SITTER_SWIFT_GRAMMAR_VERSION,
        "grammar metadata must match the pinned tree-sitter-swift dependency"
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: "swift"
            .try_into()
            .expect("swift language id should be valid"),
        repo_root: env!("CARGO_MANIFEST_DIR").to_owned(),
        snapshot_id: "parser-metadata".to_owned(),
        config: ScanConfig::new(),
    };
    let facts = SwiftLanguage::new()
        .scan(&request)
        .expect("minimal Swift scan should return facts");

    assert_eq!(
        facts.language.parser_version,
        TREE_SITTER_SWIFT_GRAMMAR_VERSION
    );
}
