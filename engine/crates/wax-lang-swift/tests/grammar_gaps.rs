use std::fs;
use std::path::PathBuf;

use wax_lang_api::{DiscoverRequest, DiscoverRequestType};
use wax_lang_swift::{SwiftLanguage, discover::discover_registry_symbols};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover/grammar-gaps")
}

fn assert_parse_gap_diagnostic(
    diagnostic: &wax_contract::Diagnostic,
    expected_file: &str,
    expected_line: u32,
    expected_column: u32,
) {
    assert_eq!(diagnostic.code, "parse_failed");
    assert!(
        diagnostic.message.contains("file scanned with gaps"),
        "unexpected message: {}",
        diagnostic.message
    );
    let location = diagnostic
        .location
        .as_ref()
        .unwrap_or_else(|| panic!("expected location for {expected_file}"));
    assert_eq!(location.file, expected_file);
    assert_eq!(location.line, expected_line);
    assert_eq!(location.column, Some(expected_column));
}

#[expect(
    unsafe_code,
    reason = "tree-sitter-swift exposes the generated grammar through a raw C ABI entrypoint, so this grammar-regression helper must call that function and wrap the returned TSLanguage pointer"
)]
fn parse_source(source: &str) -> bool {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("Test.swift");
    fs::write(&path, source).expect("write");

    let mut parser = tree_sitter::Parser::new();
    let language_fn = tree_sitter_swift::LANGUAGE.into_raw();
    let language_ptr = unsafe { language_fn() };
    let language = unsafe {
        tree_sitter::Language::from_raw(language_ptr as *const tree_sitter::ffi::TSLanguage)
    };
    parser.set_language(&language).expect("language");
    let tree = parser.parse(source.as_bytes(), None).expect("tree");
    !tree.root_node().has_error()
}

#[test]
fn bare_preview_parses_cleanly_in_tree_sitter_swift() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n#Preview { V() }\n";
    assert!(
        parse_source(source),
        "bare #Preview should parse without error nodes"
    );
}

#[test]
fn available_on_preview_has_error_nodes_in_current_grammar() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n@available(iOS 18.0, *)\n#Preview { V() }\n";
    assert!(
        !parse_source(source),
        "tree-sitter-swift 0.7.3 still reports error nodes for @available + #Preview"
    );
}

#[test]
fn empty_paren_attribute_has_error_nodes_in_current_grammar() {
    let source = "import SwiftUI\nstruct V: View {\n  @Themed() private var theme\n  var body: some View { Text(\"x\") }\n}\n";
    assert!(
        !parse_source(source),
        "tree-sitter-swift 0.7.3 still reports error nodes for @Name()"
    );
}

#[test]
fn discover_finds_component_with_available_preview_fixture() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources).expect("sources dir");
    fs::copy(
        fixture_root().join("Sources/AvailablePreview.swift"),
        sources.join("AvailablePreview.swift"),
    )
    .expect("copy fixture");

    let result = discover_registry_symbols(tempdir.path(), &[sources]).expect("discover symbols");
    assert_eq!(result.symbols(), vec!["AvailablePreviewCard"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_parse_gap_diagnostic(
        &result.diagnostics[0],
        "Sources/AvailablePreview.swift",
        9,
        1,
    );
}

#[test]
fn discover_finds_component_with_empty_paren_attribute_fixture() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources).expect("sources dir");
    fs::copy(
        fixture_root().join("Sources/EmptyParenAttribute.swift"),
        sources.join("EmptyParenAttribute.swift"),
    )
    .expect("copy fixture");

    let result = discover_registry_symbols(tempdir.path(), &[sources]).expect("discover symbols");
    assert_eq!(result.symbols(), vec!["EmptyParenAttributeCard"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_parse_gap_diagnostic(
        &result.diagnostics[0],
        "Sources/EmptyParenAttribute.swift",
        4,
        13,
    );
}

#[test]
fn discover_via_stdio_finds_grammar_gap_components() {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: 1,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: fixture_root().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
    };

    let result = SwiftLanguage::new()
        .discover(&request)
        .expect("discover via language wrapper");
    assert_eq!(
        result.symbols(),
        vec!["AvailablePreviewCard", "EmptyParenAttributeCard"]
    );
    assert_eq!(result.diagnostics.len(), 2);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "parse_failed")
    );

    let available_preview = result
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .location
                .as_ref()
                .is_some_and(|location| location.file.ends_with("AvailablePreview.swift"))
        })
        .expect("AvailablePreview diagnostic");
    assert_parse_gap_diagnostic(available_preview, "Sources/AvailablePreview.swift", 9, 1);

    let empty_paren = result
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .location
                .as_ref()
                .is_some_and(|location| location.file.ends_with("EmptyParenAttribute.swift"))
        })
        .expect("EmptyParenAttribute diagnostic");
    assert_parse_gap_diagnostic(empty_paren, "Sources/EmptyParenAttribute.swift", 4, 13);
}
