use std::path::PathBuf;
use std::{fs, io};

use wax_contract::{DiagnosticSeverity, RegistryDocument};
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION,
};
use wax_lang_compose::discover::discover_registry_symbols;
use wax_lang_compose::{ComposeDiscoverError, ComposeLanguage, discover_registry_details};

fn fixture_parse_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}

fn fixture_root() -> PathBuf {
    fixture_parse_root().join("design-system/src/main/kotlin")
}

#[test]
fn discover_registry_details_returns_typed_error_for_invalid_registry() {
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: fixture_parse_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry: serde_json::from_value(serde_json::json!({"schema_version": 2})).unwrap(),
    };

    let error = discover_registry_details(&request).expect_err("registry should be rejected");

    assert!(matches!(error, ComposeDiscoverError::InvalidRegistry(_)));
}

#[test]
fn discover_registry_details_extracts_direct_component_api_facts() {
    let repo_root = fixture_parse_root();
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "compose.rich-button",
            "symbol": "RichButton",
            "package": "com.example.ds"
        }]
    }))
    .expect("registry fixture should deserialize");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("matching component should be enriched");

    assert_eq!(
        serde_json::to_value(extracted).unwrap(),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "design-system/src/main/kotlin/Rich.kt",
                "start_line": 15,
                "end_line": 22
            },
            "documentation": "A button used for important actions.",
            "signature": {
                "type_text": "fun RichButton(\n    text: String,\n    enabled: Boolean = true,\n    onClick: () -> Unit,\n)",
                "parameters": [
                    {
                        "name": "text",
                        "type_text": "String",
                        "required": true
                    },
                    {
                        "name": "enabled",
                        "type_text": "Boolean",
                        "default": "true",
                        "required": false
                    },
                    {
                        "name": "onClick",
                        "type_text": "() -> Unit",
                        "required": true
                    }
                ]
            },
            "deprecation": {
                "message": "Prefer ActionButton instead."
            },
            "attachments": [{
                "kind": "example",
                "path": "design-system/src/main/kotlin/Rich.kt",
                "content": "RichButton(text = \"Go\") {}",
                "language": "kotlin"
            }]
        })
    );
}

#[test]
fn discover_registry_details_marks_imported_parameter_type_unavailable() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("ImportedButton.kt"),
        r#"package com.example

import com.acme.ImportedProps
import androidx.compose.runtime.Composable

@Composable
fun ImportedButton(props: ImportedProps) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "imported", "symbol": "ImportedButton", "package": "com.example"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0].extracted.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(extracted).unwrap(),
        serde_json::json!({
            "status": "partial",
            "source_evidence": {
                "path": "src/ImportedButton.kt",
                "start_line": 6,
                "end_line": 7
            },
            "signature": {
                "type_text": "fun ImportedButton(props: ImportedProps)",
                "parameters": [{
                    "name": "props",
                    "type_text": "ImportedProps",
                    "required": true
                }]
            },
            "field_gaps": [{
                "field": "signature.parameters",
                "reason": "parameter type `ImportedProps` is imported or unavailable locally"
            }]
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_enriches_only_explicitly_associated_tokens() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Tokens.kt"),
        "object Tokens {\n    val ColorPrimary = \"#fff\"\n    val SpacingSm = \"4\"\n}\n",
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "tokens": [
            {
                "id": "color.primary",
                "key": "Tokens.ColorPrimary",
                "category": "color",
                "value": "#000",
                "metadata": {"sources": [{
                    "kind": "declaration",
                    "path": "src/Tokens.kt",
                    "start_line": 2,
                    "end_line": 2,
                    "language": "kotlin"
                }]}
            },
            {
                "id": "spacing.sm",
                "key": "Tokens.SpacingSm",
                "category": "spacing",
                "value": "8"
            }
        ]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");

    assert_eq!(
        serde_json::to_value(&result.document.tokens).unwrap(),
        serde_json::json!([
            {
                "id": "color.primary",
                "key": "Tokens.ColorPrimary",
                "category": "color",
                "value": "#000",
                "metadata": {"sources": [{
                    "kind": "declaration",
                    "path": "src/Tokens.kt",
                    "start_line": 2,
                    "end_line": 2,
                    "language": "kotlin"
                }]},
                "extracted": {
                    "status": "complete",
                    "source_evidence": {
                        "path": "src/Tokens.kt",
                        "start_line": 2,
                        "end_line": 2
                    }
                }
            },
            {
                "id": "spacing.sm",
                "key": "Tokens.SpacingSm",
                "category": "spacing",
                "value": "8"
            }
        ])
    );
    Ok(())
}

#[test]
fn discover_registry_details_preserves_prior_facts_as_stale_on_damaged_syntax() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(src.join("Broken.kt"), "@Composable\nfun Broken(")?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "broken",
            "symbol": "Broken",
            "extracted": {
                "status": "complete",
                "documentation": "Prior documentation",
                "source_evidence": {
                    "path": "src/Broken.kt",
                    "start_line": 1,
                    "end_line": 1
                }
            }
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("damaged source should be recoverable");

    assert_eq!(
        serde_json::to_value(result.document.components[0].extracted.as_ref().unwrap()).unwrap(),
        serde_json::json!({
            "status": "stale",
            "documentation": "Prior documentation",
            "source_evidence": {
                "path": "src/Broken.kt",
                "start_line": 1,
                "end_line": 1
            }
        })
    );
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}

#[test]
fn discover_registry_details_keeps_prior_stale_when_partial_parse_still_finds_declaration()
-> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    // Recoverable damage: PrimaryButton remains in a clean range, but the file is partial.
    fs::write(
        src.join("Partial.kt"),
        r#"import androidx.compose.runtime.Composable

/**
 * Prior documentation that a damaged sibling can make hard to re-extract.
 */
@Composable
fun PrimaryButton() {}

fun Broken(
@Composable
fun SecondaryButton() {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "primary",
            "symbol": "PrimaryButton",
            "extracted": {
                "status": "complete",
                "documentation": "Prior documentation that a damaged sibling can make hard to re-extract.",
                "source_evidence": {
                    "path": "src/Partial.kt",
                    "start_line": 6,
                    "end_line": 6
                }
            }
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("partial parse should be recoverable");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("prior facts should be retained");

    assert_eq!(
        extracted.status,
        wax_contract::RegistryExtractionStatus::Stale
    );
    assert_eq!(
        extracted.documentation.as_deref(),
        Some("Prior documentation that a damaged sibling can make hard to re-extract.")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "parse_failed"),
        "expected partial-tree damage diagnostics, got {:?}",
        result.diagnostics
    );
    Ok(())
}

#[test]
fn discover_registry_details_omits_sample_tag_as_example_body() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("SampleOnly.kt"),
        r#"package com.example

import androidx.compose.runtime.Composable

/**
 * Documented button.
 *
 * @sample com.example.samples.SampleOnlyButtonSample
 */
@Composable
fun SampleOnlyButton() {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "sample-only",
            "symbol": "SampleOnlyButton",
            "package": "com.example"
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0]
        .extracted
        .as_ref()
        .expect("matching component should be enriched");

    assert_eq!(
        extracted.documentation.as_deref(),
        Some("Documented button.")
    );
    assert!(
        extracted.attachments.is_empty(),
        "@sample FQN is a file reference for Phase 3d, not embedded example body: {:?}",
        extracted.attachments
    );
    Ok(())
}

#[test]
fn discover_registry_details_does_not_stale_unrelated_components_on_damage() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(src.join("Broken.kt"), "@Composable\nfun Broken(")?;
    fs::write(src.join("Intact.kt"), "@Composable\nfun Intact() {}\n")?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "other",
            "symbol": "Other",
            "extracted": {
                "status": "complete",
                "documentation": "Prior documentation",
                "source_evidence": {
                    "path": "src/Other.kt",
                    "start_line": 1,
                    "end_line": 3
                }
            }
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("damaged source should be recoverable");
    let extracted = result.document.components[0].extracted.as_ref().unwrap();

    assert_eq!(
        extracted.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert_eq!(
        extracted.documentation.as_deref(),
        Some("Prior documentation")
    );
    assert!(result.diagnostics.iter().any(|d| d.code == "parse_failed"));
    Ok(())
}

#[test]
fn discover_registry_details_is_idempotent_on_clean_reruns() {
    let repo_root = fixture_parse_root();
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "compose.rich-button",
            "symbol": "RichButton",
            "package": "com.example.ds"
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
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
fn discovers_public_top_level_composables() {
    let result = discover_registry_symbols(&fixture_parse_root(), &[fixture_root()])
        .expect("discover symbols");

    assert!(!result.symbols().iter().any(|symbol| symbol == "NestedCard"));
    assert_eq!(
        result.symbols(),
        vec![
            "PrimaryButton",
            "QualifiedButton",
            "RichButton",
            "SecondaryButton"
        ]
    );
    let package_for = |symbol: &str| {
        result
            .components
            .iter()
            .find(|component| component.symbol == symbol)
            .and_then(|component| component.package.as_deref())
    };
    assert_eq!(package_for("SecondaryButton"), Some("com.example.ds"));
    assert_eq!(package_for("QualifiedButton"), Some("com.example.ds"));
    assert_eq!(package_for("RichButton"), Some("com.example.ds"));
    assert_eq!(package_for("PrimaryButton"), None);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "discover_package_conflict");
}

#[test]
fn compose_language_discover_resolves_repo_relative_roots() {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: fixture_parse_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let result = ComposeLanguage::new()
        .discover(&request)
        .expect("discover should succeed");

    assert!(result.symbols().contains(&"RichButton".to_owned()));
}

#[test]
fn missing_root_fails() {
    let missing = fixture_root().join("missing");

    let err = discover_registry_symbols(&fixture_parse_root(), &[missing])
        .expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_do_not_block_symbols_from_other_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let good_file = tempdir.path().join("Good.kt");
    let broken_file = tempdir.path().join("Broken.kt");
    fs::write(&good_file, "@Composable\nfun GoodButton() {}\n")?;
    fs::write(&broken_file, "@Composable\nfun Broken(")?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols(), vec!["GoodButton"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert_eq!(
        result.diagnostics[0]
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some("Broken.kt")
    );
    Ok(())
}

#[test]
fn skips_preview_provider_and_effect_composables() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let source_file = tempdir.path().join("Components.kt");
    fs::write(
        source_file,
        r#"
@Composable
fun PrimaryButton() {}

@Preview
@Composable
fun PrimaryButtonPreview() {}

@Composable
fun ProvideTheme() {}

@Composable
fun ScrollFrameDurationEffect() {}
"#,
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover symbols");

    assert_eq!(result.symbols(), vec!["PrimaryButton"]);
    Ok(())
}

#[test]
fn parse_failures_are_skipped_with_diagnostics() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let broken_file = tempdir.path().join("Broken.kt");
    fs::write(&broken_file, "@Composable\nfun Broken(")?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert!(result.symbols().is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].severity, DiagnosticSeverity::Error);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert_eq!(
        result.diagnostics[0]
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some("Broken.kt")
    );
    assert!(result.diagnostics[0].message.contains("Broken.kt"));
    Ok(())
}
