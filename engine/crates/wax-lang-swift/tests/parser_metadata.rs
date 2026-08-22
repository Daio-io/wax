use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_swift::{SwiftLanguage, TREE_SITTER_SWIFT_GRAMMAR_VERSION};

#[test]
fn parser_metadata_matches_dependency_and_scan_facts() {
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
