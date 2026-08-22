use std::fs;
use std::path::PathBuf;

use wax_contract::{ScanStatus, SourceLocation};
use wax_lang_api::{
    DiscoverRequest, DiscoverRequestType, ScanConfig, ScanRequest, ScanRequestType,
};
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

fn parse_source(source: &str) -> bool {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources).expect("sources");
    fs::write(sources.join("Test.swift"), source).expect("write");

    let result = discover_registry_symbols(tempdir.path(), &[sources]).expect("discover");
    result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "parse_failed")
}

fn scan_source(source: &str) -> wax_contract::ScanFacts {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources).expect("sources");
    fs::write(sources.join("Test.swift"), source).expect("write source");
    fs::write(
        tempdir.path().join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton","targets":["swift"]}]}"#,
    )
    .expect("write registry");

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: 1,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        snapshot_id: "swift-unit-expression-recovery".to_owned(),
        config,
    };

    SwiftLanguage::new().scan(&request).expect("scan")
}

fn continuation_reproduction(optional_binding: &str) -> String {
    format!(
        "import SwiftUI\n\
func wait(_ continuation: CheckedContinuation<Void, any Swift.Error>, error: Error?) async {{\n\
    Task {{\n\
        {optional_binding} {{\n\
            continuation.resume(throwing: error)\n\
            return\n\
        }}\n\
        continuation.resume(returning: ())\n\
    }}\n\
}}\n\
struct AfterContinuation: View {{\n\
    var body: some View {{ PrimaryButton(title: \"after\") }}\n\
}}\n"
    )
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
fn available_on_preview_parses_cleanly_after_recovery() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n@available(iOS 18.0, *)\n#Preview { V() }\n";
    assert!(
        parse_source(source),
        "@available immediately before #Preview should be recovered"
    );
}

#[test]
fn named_preview_parses_cleanly_after_recovery() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n#Preview(\"Card\") { V() }\n";

    assert!(parse_source(source));
}

#[test]
fn preview_traits_parse_cleanly_after_recovery() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n#Preview(\"Card\", traits: .fixedLayout(width: 100, height: 100)) { V() }\n";

    assert!(parse_source(source));
}

#[test]
fn debug_preview_parses_cleanly_after_recovery() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n#if DEBUG\n#Preview { V() }\n#endif\n";

    assert!(parse_source(source));
}

#[test]
fn debug_available_preview_parses_cleanly_after_recovery() {
    let source = "import SwiftUI\nstruct V: View { var body: some View { Text(\"x\") } }\n#if DEBUG\n@available(iOS 18.0, *)\n#Preview { V() }\n#endif\n";

    assert!(parse_source(source));
}

#[test]
fn isolated_existential_any_parses_cleanly_without_recovery() {
    assert!(parse_source("func use(_ error: any Swift.Error) {}\n"));
}

#[test]
fn typed_checked_continuation_parses_cleanly_without_recovery() {
    assert!(parse_source(
        "func use(_ continuation: CheckedContinuation<Void, any Swift.Error>) {}\n"
    ));
}

#[test]
fn shorthand_optional_binding_parses_cleanly_without_recovery() {
    assert!(parse_source(
        "func use(_ error: Error?) { if let error { _ = error } }\n"
    ));
}

#[test]
fn expanded_optional_binding_parses_cleanly_without_recovery() {
    assert!(parse_source(
        "func use(_ error: Error?) { if let error = error { _ = error } }\n"
    ));
}

#[test]
fn continuation_reproductions_scan_completely_and_retain_later_facts() {
    for optional_binding in ["if let error", "if let error = error"] {
        let source = continuation_reproduction(optional_binding);
        let facts = scan_source(&source);
        let baseline_source = source.replacen("returning: ()", "returning: {}", 1);
        assert_ne!(baseline_source, source);
        let baseline = scan_source(&baseline_source);

        assert_eq!(facts.status, ScanStatus::Complete);
        assert!(
            facts
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "parse_failed")
        );
        assert!(facts.usage_sites.iter().any(|usage| {
            usage.symbol == "PrimaryButton"
                && usage.location.file == "Sources/Test.swift"
                && usage.location.line == 12
        }));
        assert_eq!(facts.usage_sites, baseline.usage_sites);
        assert_eq!(facts.local_components, baseline.local_components);
    }
}

#[test]
fn shorthand_and_expanded_optional_binding_have_identical_scan_facts() {
    let shorthand = scan_source(&continuation_reproduction("if let error"));
    let expanded = scan_source(&continuation_reproduction("if let error = error"));

    assert_eq!(shorthand.status, expanded.status);
    assert_eq!(shorthand.local_components, expanded.local_components);
    assert_eq!(shorthand.usage_sites, expanded.usage_sites);
    assert_eq!(
        shorthand.hardcoded_style_sites,
        expanded.hardcoded_style_sites
    );
    assert_eq!(shorthand.diagnostics, expanded.diagnostics);
}

#[test]
fn unit_expression_recovery_does_not_emit_parser_only_style_observations() {
    let facts = scan_source(
        "let value: Void = ()\nstruct After: View { var body: some View { PrimaryButton(title: \"after\") } }\n",
    );

    assert_eq!(facts.status, ScanStatus::Complete);
    assert!(facts.hardcoded_style_sites.is_empty());
}

#[test]
fn unknown_syntax_emits_one_parse_gap_and_keeps_later_facts() {
    let facts = scan_source(
        "func broken() { let value = ; }\nstruct After: View { var body: some View { PrimaryButton(title: \"after\") } }\n",
    );

    assert_eq!(facts.status, ScanStatus::Partial);
    let parse_failure_count = facts
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "parse_failed")
        .count();
    assert_eq!(parse_failure_count, 1);
    let diagnostic = facts
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "parse_failed")
        .expect("parse failure diagnostic");
    let location = diagnostic.location.as_ref().expect("diagnostic location");
    assert_eq!(location.file, "Sources/Test.swift");
    assert_eq!(location.line, 1);
    assert!(location.column.is_some());
    assert!(
        facts
            .usage_sites
            .iter()
            .any(|usage| { usage.symbol == "PrimaryButton" && usage.location.line == 2 })
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
    assert_eq!(
        result.symbols(),
        vec!["AfterAvailablePreviewCard", "AvailablePreviewCard"]
    );
    assert!(result.diagnostics.is_empty());
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
        vec![
            "AfterAvailablePreviewCard",
            "AvailablePreviewCard",
            "EmptyParenAttributeCard"
        ]
    );
    assert_eq!(result.diagnostics.len(), 1);

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

#[test]
fn scan_recovers_available_preview_without_preview_adoption_or_location_shift() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources).expect("sources");
    fs::copy(
        fixture_root().join("Sources/AvailablePreview.swift"),
        sources.join("AvailablePreview.swift"),
    )
    .expect("copy fixture");
    let registry = tempdir.path().join("registry.json");
    fs::write(
        &registry,
        r#"{"schema_version":1,"components":[{"id":"ds.preview-only","symbol":"PreviewOnlyButton","targets":["swift"]}]}"#,
    )
    .expect("write registry");

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: 1,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        snapshot_id: "available-preview-recovery".to_owned(),
        config,
    };

    let facts = SwiftLanguage::new().scan(&request).expect("scan");

    assert_eq!(facts.status, ScanStatus::Complete);
    assert!(
        facts
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "parse_failed")
    );
    assert_eq!(facts.local_components.len(), 2);
    assert!(facts.local_components.iter().any(|component| {
        component.symbol == "AvailablePreviewCard"
            && component.location
                == SourceLocation {
                    file: "Sources/AvailablePreview.swift".to_owned(),
                    line: 3,
                    column: Some(8),
                }
    }));
    assert!(facts.local_components.iter().any(|component| {
        component.symbol == "AfterAvailablePreviewCard"
            && component.location
                == SourceLocation {
                    file: "Sources/AvailablePreview.swift".to_owned(),
                    line: 15,
                    column: Some(8),
                }
    }));
    assert_eq!(facts.usage_sites.len(), 2);
    assert!(facts.usage_sites.iter().all(|usage| {
        usage.callee_origin == wax_contract::CalleeOrigin::Framework
            && usage.registry_symbol.is_none()
    }));
}
