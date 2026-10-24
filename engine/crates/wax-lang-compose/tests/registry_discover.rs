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
                "start_line": 23,
                "end_line": 31
            },
            "documentation": "A button used for important actions.",
            "signature": {
                "type_text": "fun RichButton(\n    text: String,\n    enabled: Boolean = true,\n    onClick: () -> Unit,\n    style: Style = Style.Compact,\n)",
                "parameters": [
                    {
                        "name": "text",
                        "type_text": "String",
                        "type_kind": "string",
                        "description": "Visible button label.",
                        "required": true
                    },
                    {
                        "name": "enabled",
                        "type_text": "Boolean",
                        "type_kind": "boolean",
                        "description": "Whether the button accepts input.",
                        "default": "true",
                        "required": false
                    },
                    {
                        "name": "onClick",
                        "type_text": "() -> Unit",
                        "type_kind": "function",
                        "description": "Invoked when the button is pressed.",
                        "required": true
                    },
                    {
                        "name": "style",
                        "type_text": "Style",
                        "type_kind": "enum",
                        "allowed_values": ["Compact", "Comfortable"],
                        "description": "Visual density of the button.",
                        "default": "Style.Compact",
                        "required": false
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
            }],
            "best_practices": [
                {"description": "Use one primary action per surface."},
                {"description": "Keep labels concise."}
            ]
        })
    );
}

#[test]
fn discover_registry_details_extracts_kdoc_before_opt_in_annotation() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("SeekBar.kt"),
        r#"package com.example

import androidx.compose.runtime.Composable

/**
 * A SeekBar component that allows users to select a value.
 */
@OptIn(ExperimentalWildApi::class)
@Composable
fun SeekBar() {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "seek-bar", "symbol": "SeekBar", "package": "com.example"}]
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
        Some("A SeekBar component that allows users to select a value.")
    );
    Ok(())
}

#[test]
fn discover_registry_details_extracts_kdoc_before_opt_in_on_overloads() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("DialogRepro.kt"),
        r#"package com.example
import androidx.compose.runtime.Composable

/**
 * Primary overload docs.
 */
@OptIn(ExperimentalWildApi::class)
@Composable
fun DialogRepro(onDismissRequest: () -> Unit, content: @Composable () -> Unit) {}

/**
 * Secondary overload docs.
 */
@Composable
fun DialogRepro(title: String, onDismissRequest: () -> Unit) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "dialog-repro", "symbol": "DialogRepro", "package": "com.example"}]
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
        Some("Primary overload docs.")
    );
    let start_line = extracted
        .source_evidence
        .as_ref()
        .and_then(|evidence| evidence.start_line)
        .expect("source evidence start_line");
    // Span must include leading @OptIn so the line before start_line is the KDoc closer.
    assert_eq!(start_line, 7);
    Ok(())
}

#[test]
fn discover_registry_details_extracts_kdoc_before_opt_in_on_extension_after_sibling()
-> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("BarItemRepro.kt"),
        r#"package com.example
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.RowScope

@Composable
fun NavigationBarX(content: @Composable RowScope.() -> Unit) {}

/**
 * A navigation bar item for use within a NavigationBar.
 */
@OptIn(ExperimentalWildApi::class)
@Composable
fun RowScope.BarItemX(selected: Boolean, onClick: () -> Unit) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "bar-item-x", "symbol": "BarItemX", "package": "com.example"}]
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
        Some("A navigation bar item for use within a NavigationBar.")
    );
    let start_line = extracted
        .source_evidence
        .as_ref()
        .and_then(|evidence| evidence.start_line)
        .expect("source evidence start_line");
    // Span must include leading @OptIn so the line before start_line is the KDoc closer.
    assert_eq!(start_line, 11);
    Ok(())
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
fn discover_registry_details_ignores_unknown_kdoc_parameter() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("DocumentedButton.kt"),
        r#"package com.example

import androidx.compose.runtime.Composable

/**
 * A documented button.
 * @param label First line
 *   continued here.
 * @param missing Must not synthesize a parameter.
 * @param label Duplicate text is ignored.
 *   Duplicate continuation is ignored too.
 * @guidance
 * @guidanceNote This is a different tag and must be ignored.
 */
@Composable
fun DocumentedButton(label: String) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "documented",
            "symbol": "DocumentedButton",
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
    let extracted = result.document.components[0].extracted.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(extracted.signature.as_ref().unwrap()).unwrap(),
        serde_json::json!({
            "type_text": "fun DocumentedButton(label: String)",
            "parameters": [{
                "name": "label",
                "type_text": "String",
                "type_kind": "string",
                "description": "First line continued here.",
                "required": true
            }]
        })
    );
    assert!(extracted.best_practices.is_empty());
    Ok(())
}

#[test]
fn discover_registry_details_omits_empty_or_imported_enum_facts() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Kinds.kt"),
        r#"package com.example

import androidx.compose.runtime.Composable
import com.acme.ImportedStyle

enum class Empty
data class Config(val name: String)

@Composable
fun Kinds(
    empty: Empty,
    imported: ImportedStyle,
    qualified: com.acme.QualifiedStyle,
    generic: Wrapper<String>,
    count: Int,
    items: List<String>,
    config: Config,
) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "kinds", "symbol": "Kinds", "package": "com.example"}]
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
    let parameters = &result.document.components[0]
        .extracted
        .as_ref()
        .unwrap()
        .signature
        .as_ref()
        .unwrap()
        .parameters;

    assert_eq!(
        serde_json::to_value(parameters).unwrap(),
        serde_json::json!([
            {"name": "empty", "type_text": "Empty", "required": true},
            {"name": "imported", "type_text": "ImportedStyle", "required": true},
            {"name": "qualified", "type_text": "com.acme.QualifiedStyle", "required": true},
            {"name": "generic", "type_text": "Wrapper<String>", "required": true},
            {
                "name": "count",
                "type_text": "Int",
                "type_kind": "number",
                "required": true
            },
            {
                "name": "items",
                "type_text": "List<String>",
                "type_kind": "collection",
                "required": true
            },
            {
                "name": "config",
                "type_text": "Config",
                "type_kind": "object",
                "required": true
            }
        ])
    );
    Ok(())
}

#[test]
fn discover_registry_details_classifies_outer_collection_with_function_argument() -> io::Result<()>
{
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Callbacks.kt"),
        r#"package com.example

@Composable
fun Callbacks(callbacks: List<() -> Unit>) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "callbacks", "symbol": "Callbacks", "package": "com.example"}]
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
    let parameter = &result.document.components[0]
        .extracted
        .as_ref()
        .unwrap()
        .signature
        .as_ref()
        .unwrap()
        .parameters[0];

    assert_eq!(
        parameter.type_kind,
        Some(wax_contract::RegistryParameterTypeKind::Collection)
    );
    Ok(())
}

#[test]
fn discover_registry_details_classifies_nullable_parenthesized_function() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("NullableCallback.kt"),
        r#"package com.example

@Composable
fun NullableCallback(callback: (() -> Unit)?) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "nullable-callback", "symbol": "NullableCallback", "package": "com.example"}]
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
    let parameter = &result.document.components[0]
        .extracted
        .as_ref()
        .unwrap()
        .signature
        .as_ref()
        .unwrap()
        .parameters[0];

    assert_eq!(
        parameter.type_kind,
        Some(wax_contract::RegistryParameterTypeKind::Function)
    );
    Ok(())
}

#[test]
fn discover_registry_details_does_not_use_nested_enum_with_same_name() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Styles.kt"),
        r#"package com.example

enum class Style { Compact, Comfortable }

object Unrelated {
    enum class Style { Wrong, AlsoWrong }
}

@Composable
fun Styled(style: Style) {}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "styled", "symbol": "Styled", "package": "com.example"}]
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
    let parameter = &result.document.components[0]
        .extracted
        .as_ref()
        .unwrap()
        .signature
        .as_ref()
        .unwrap()
        .parameters[0];

    assert_eq!(parameter.allowed_values, vec!["Compact", "Comfortable"]);
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
