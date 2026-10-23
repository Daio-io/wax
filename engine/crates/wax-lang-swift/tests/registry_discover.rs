use std::path::PathBuf;
use std::{fs, io};

use wax_contract::RegistryDocument;
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION,
};
use wax_lang_swift::{
    SwiftLanguage, discover::discover_registry_symbols, discover_registry_details,
};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}

fn fixture_sources_root() -> PathBuf {
    fixture_root().join("design-system/Sources")
}

fn rich_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rich-discover")
}

#[test]
fn discover_registry_details_extracts_direct_component_api_facts() {
    let repo_root = rich_fixture_root();
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "swift.rich-button",
            "symbol": "RichButton",
            "package": "AcmeDesignSystem"
        }]
    }))
    .expect("registry fixture should deserialize");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: repo_root.to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("matching component should be enriched");

    assert_eq!(
        serde_json::to_value(extracted).expect("serialize extracted facts"),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "design-system/Sources/AcmeDesignSystem/RichComponents.swift",
                "start_line": 9,
                "end_line": 16
            },
            "documentation": "A button used for important actions.",
            "signature": {
                "type_text": "public init(tone: Tone = .strong, disabled: Bool)",
                "parameters": [
                    {
                        "name": "tone",
                        "type_text": "Tone",
                        "default": ".strong",
                        "required": false
                    },
                    {
                        "name": "disabled",
                        "type_text": "Bool",
                        "required": true
                    }
                ]
            },
            "deprecation": {
                "message": "Use ActionButton instead."
            },
            "attachments": [{
                "kind": "example",
                "path": "design-system/Sources/AcmeDesignSystem/RichComponents.swift",
                "content": "RichButton(tone: .quiet, disabled: false)",
                "language": "swift"
            }]
        })
    );
}

#[test]
fn discover_registry_details_marks_imported_type_facts_unavailable() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let sources = tempdir.path().join("Sources/DesignSystem");
    fs::create_dir_all(&sources)?;
    fs::write(
        sources.join("ImportedCard.swift"),
        r#"import SwiftUI
import VendorKit

public func ImportedCard(configuration: VendorKit.Configuration) -> some View {
    Text(configuration.title)
}

"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "imported", "symbol": "ImportedCard"}]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("component should be enriched");

    assert_eq!(
        extracted.signature.as_ref().and_then(|signature| {
            signature
                .parameters
                .first()
                .and_then(|parameter| parameter.type_text.as_deref())
        }),
        Some("VendorKit.Configuration")
    );
    assert_eq!(
        extracted.status,
        wax_contract::RegistryExtractionStatus::Partial
    );
    assert!(extracted.field_gaps.iter().any(|gap| {
        gap.field == "signature.parameters.configuration.type_details"
            && gap.reason.contains("imported")
    }));
    Ok(())
}

#[test]
fn discover_registry_details_extracts_swift_initializer_shapes_without_confusing_attributes()
-> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let sources = tempdir.path().join("Sources/DesignSystem");
    fs::create_dir_all(&sources)?;
    fs::write(
        sources.join("Shapes.swift"),
        r#"import SwiftUI

@available(iOS 17, *)
public struct AttributedCard: View {
    public init<T>(value: T = "Hello, world", disabled: Bool = false) {}
    public var body: some View { Text("card") }
}

public struct DeprecatedMemberCard: View {
    @available(*, deprecated, message: "Use replacement")
    public init?(value: String) {}
    public var body: some View { Text("card") }
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [
            {"id": "attributed", "symbol": "AttributedCard"},
            {"id": "deprecated-member", "symbol": "DeprecatedMemberCard"}
        ]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let attributed = result.document.components[0]
        .extracted
        .as_ref()
        .expect("attributed component should be enriched");
    assert_eq!(
        attributed.signature.as_ref().map(|signature| {
            signature
                .parameters
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.default.as_deref()))
                .collect::<Vec<_>>()
        }),
        Some(vec![
            ("value", Some("\"Hello, world\"")),
            ("disabled", Some("false")),
        ])
    );
    assert_eq!(attributed.deprecation, None);

    let deprecated_member = result.document.components[1]
        .extracted
        .as_ref()
        .expect("optional initializer component should be enriched");
    assert_eq!(
        deprecated_member
            .signature
            .as_ref()
            .map(|signature| signature.parameters.len()),
        Some(1)
    );
    assert_eq!(deprecated_member.deprecation, None);
    Ok(())
}

#[test]
fn discover_registry_details_marks_synthesized_initializer_unavailable() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let sources = tempdir.path().join("Sources/DesignSystem");
    fs::create_dir_all(&sources)?;
    fs::write(
        sources.join("SynthesizedCard.swift"),
        r#"import SwiftUI

public struct SynthesizedCard: View {
    public let title: String
    public var body: some View { Text(title) }
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "synthesized", "symbol": "SynthesizedCard"}]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("component should be enriched");

    assert_eq!(
        extracted.status,
        wax_contract::RegistryExtractionStatus::Partial
    );
    assert!(extracted.signature.is_none());
    assert!(
        extracted
            .field_gaps
            .iter()
            .any(|gap| { gap.field == "signature.parameters" && gap.reason.contains("compiler") })
    );
    Ok(())
}

#[test]
fn discover_registry_details_enriches_only_explicit_existing_tokens() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources)?;
    fs::write(
        sources.join("DesignTokens.swift"),
        "public let primaryColor = \"#3366ff\"\npublic let sourceFacingOnly = 12\n",
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "tokens": [
            {
                "id": "color.primary",
                "key": "primaryColor",
                "category": "color",
                "value": "#3366ff",
                "metadata": {
                    "sources": [{
                        "kind": "declaration",
                        "path": "Sources/DesignTokens.swift",
                        "start_line": 1,
                        "end_line": 1,
                        "language": "swift"
                    }]
                }
            },
            {
                "id": "spacing.unassociated",
                "key": "sourceFacingOnly",
                "category": "spacing",
                "value": "12"
            }
        ]
    }))
    .expect("registry fixture");
    let original_tokens = registry.tokens.clone();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");

    assert_eq!(result.document.tokens[0].id, original_tokens[0].id);
    assert_eq!(result.document.tokens[0].value, original_tokens[0].value);
    assert_eq!(result.document.tokens[1].id, original_tokens[1].id);
    assert_eq!(result.document.tokens[1].value, original_tokens[1].value);
    assert_eq!(
        result.document.tokens[0]
            .extracted
            .as_ref()
            .and_then(|extracted| extracted.source_evidence.as_ref())
            .map(|evidence| evidence.path.as_str()),
        Some("Sources/DesignTokens.swift")
    );
    assert!(result.document.tokens[1].extracted.is_none());
    assert_eq!(result.document.tokens.len(), 2);
    Ok(())
}

#[test]
fn discover_registry_details_preserves_facts_when_syntax_is_damaged() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let sources = tempdir.path().join("Sources");
    fs::create_dir_all(&sources)?;
    fs::write(
        sources.join("Damaged.swift"),
        "public struct DamagedCard: View {\n",
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [
            {
                "id": "damaged",
                "symbol": "DamagedCard",
                "extracted": {
                    "status": "complete",
                    "source_evidence": {"path": "Sources/Damaged.swift"},
                    "documentation": "Prior damaged documentation"
                }
            },
            {
                "id": "unrelated",
                "symbol": "UnrelatedCard",
                "extracted": {
                    "status": "complete",
                    "source_evidence": {"path": "Sources/Other.swift"},
                    "documentation": "Prior unrelated documentation"
                }
            },
            {"id": "unknown", "symbol": "UnknownCard"}
        ]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["Sources".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");

    assert_eq!(
        result.document.components[0]
            .extracted
            .as_ref()
            .map(|extracted| extracted.status),
        Some(wax_contract::RegistryExtractionStatus::Stale)
    );
    assert_eq!(
        result.document.components[1]
            .extracted
            .as_ref()
            .and_then(|extracted| extracted.documentation.as_deref()),
        Some("Prior unrelated documentation")
    );
    assert!(result.document.components[2].extracted.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "parse_failed"
            && diagnostic
                .location
                .as_ref()
                .is_some_and(|location| location.file == "Sources/Damaged.swift")
    }));
    Ok(())
}

#[test]
fn discover_registry_details_is_idempotent_on_clean_reruns() {
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "rich", "symbol": "RichButton"}]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: rich_fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
        registry,
    };
    let first = discover_registry_details(&request).expect("first discover should succeed");
    let rerun = DiscoverRegistryRequest {
        registry: first.document.clone(),
        ..request
    };

    let second = discover_registry_details(&rerun).expect("second discover should succeed");

    assert_eq!(second.document, first.document);
}

#[test]
fn discover_registry_details_returns_typed_error_for_invalid_registry() {
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
        registry: serde_json::from_value(serde_json::json!({"schema_version": 2}))
            .expect("registry fixture"),
    };

    let error = discover_registry_details(&request).expect_err("registry should be rejected");

    assert!(matches!(
        error,
        wax_lang_swift::SwiftDiscoverError::InvalidRegistry(_)
    ));
}

#[test]
fn discovers_public_and_package_swiftui_symbols() {
    let result = discover_registry_symbols(&fixture_root(), &[fixture_sources_root()])
        .expect("discover symbols");
    assert_eq!(
        result.symbols(),
        vec!["Badge", "PackageCard", "PrimaryButton"]
    );
    assert!(
        result
            .components
            .iter()
            .all(|component| component.package.as_deref() == Some("AcmeDesignSystem"))
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn missing_discovery_root_fails() {
    let missing = fixture_sources_root().join("missing");
    let err = discover_registry_symbols(&fixture_root(), &[missing])
        .expect_err("missing root should fail");
    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_reported_while_discovery_continues() {
    let root = fixture_root().join("broken/Sources");
    let result =
        discover_registry_symbols(&fixture_root(), &[root]).expect("discover should continue");
    assert!(result.symbols().is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert_eq!(
        result.diagnostics[0]
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some("broken/Sources/Broken.swift")
    );
}

#[test]
fn duplicate_symbols_are_deduped_and_nested_symbols_are_excluded() {
    let result = discover_registry_symbols(&fixture_root(), &[fixture_sources_root()])
        .expect("discover symbols");
    assert_eq!(
        result
            .symbols()
            .iter()
            .filter(|symbol| *symbol == "PrimaryButton")
            .count(),
        1
    );
    assert!(!result.symbols().iter().any(|symbol| symbol == "NestedCard"));
}

#[test]
fn swift_language_discover_joins_repo_relative_roots() {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: 1,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
    };

    let result = SwiftLanguage::new()
        .discover(&request)
        .expect("discover via language wrapper");
    assert_eq!(
        result.symbols(),
        vec!["Badge", "PackageCard", "PrimaryButton"]
    );
}

#[test]
fn parse_failures_do_not_block_symbols_from_other_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let good_file = tempdir.path().join("Good.swift");
    let broken_file = tempdir.path().join("Broken.swift");
    fs::write(
        &good_file,
        "@MainActor\nstruct GoodButton: View { var body: some View { Text(\"ok\") } }\n",
    )?;
    fs::write(&broken_file, "struct Broken(")?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols(), vec!["GoodButton"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}
