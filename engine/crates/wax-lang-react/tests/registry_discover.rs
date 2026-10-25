use std::path::PathBuf;
use std::{fs, io};

use wax_contract::{RegistryDocument, RegistryParameterTypeKind};
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION,
};
use wax_lang_react::{
    ReactDiscoverError, ReactLanguage, discover_registry_details, discover_registry_symbols,
};

#[test]
fn discover_registry_details_returns_typed_error_for_invalid_registry() {
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/src".to_owned()],
        registry: serde_json::from_value(serde_json::json!({"schema_version": 2})).unwrap(),
    };

    let error = discover_registry_details(&request).expect_err("registry should be rejected");

    assert!(matches!(error, ReactDiscoverError::InvalidRegistry(_)));
}

#[test]
fn discover_registry_details_extracts_direct_component_api_facts() {
    // Keep this rich fixture aligned with Compose and Swift so parity tests compare
    // the same structured facts across parser-backed packs.
    let repo_root = fixture_root();
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "react.rich-button",
            "symbol": "RichButton",
            "package": "@acme/design-system"
        }]
    }))
    .expect("registry fixture should deserialize");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        roots: vec!["design-system/src".to_owned()],
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
                "path": "design-system/src/rich.tsx",
                "start_line": 24,
                "end_line": 26
            },
            "documentation": "A button used for important actions.",
            "signature": {
                "type_text": "export function RichButton({ tone = \"strong\", disabled }: ButtonProps)",
                "parameters": [
                    {
                        "name": "tone",
                        "type_text": "\"strong\" | \"quiet\"",
                        "type_kind": "enum",
                        "allowed_values": ["strong", "quiet"],
                        "description": "Visual emphasis.",
                        "default": "\"strong\"",
                        "required": false
                    },
                    {
                        "name": "disabled",
                        "type_text": "boolean",
                        "type_kind": "boolean",
                        "description": "Prevents interaction.",
                        "required": true
                    }
                ]
            },
            "deprecation": {
                "message": "Use ActionButton instead."
            },
            "attachments": [{
                "kind": "example",
                "path": "design-system/src/rich.tsx",
                "content": "<Button tone=\"quiet\" />",
                "language": "tsx"
            }],
            "best_practices": [
                {"description": "Prefer touch-first controls."},
                {"description": "Keep labels concise."}
            ]
        })
    );
}

#[test]
fn discover_registry_details_marks_imported_props_unavailable() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("ImportedButton.tsx"),
        r#"import type { ImportedProps } from "./types";
export const ImportedButton = ({ size }: ImportedProps) => <button data-size={size} />;
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "imported", "symbol": "ImportedButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
                "path": "src/ImportedButton.tsx",
                "start_line": 2,
                "end_line": 2
            },
            "signature": {
                "type_text": "export const ImportedButton = ({ size }: ImportedProps) =>"
            },
            "field_gaps": [{
                "field": "signature.parameters",
                "reason": "props type `ImportedProps` is imported or unavailable locally"
            }]
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_omits_structured_kind_for_mixed_union() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("MixedButton.tsx"),
        r#"interface MixedButtonProps {
  tone: "strong" | CustomTone;
}
export function MixedButton({ tone }: MixedButtonProps) {
  return <button data-tone={tone} />;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "mixed", "symbol": "MixedButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
        serde_json::to_value(parameter).unwrap(),
        serde_json::json!({
            "name": "tone",
            "type_text": "\"strong\" | CustomTone",
            "required": true
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_deduplicates_enum_values() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("DuplicateButton.tsx"),
        r#"interface DuplicateButtonProps {
  tone: "strong" | "quiet" | "strong";
}
export function DuplicateButton({ tone }: DuplicateButtonProps) {
  return <button data-tone={tone} />;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "duplicate", "symbol": "DuplicateButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, vec!["strong", "quiet"]);
    Ok(())
}

#[test]
fn discover_registry_details_does_not_classify_open_ended_template_literal_as_enum()
-> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("TemplateButton.tsx"),
        r#"interface TemplateButtonProps {
  size: `size-${string}` | "auto";
}
export function TemplateButton({ size }: TemplateButtonProps) {
  return <button data-size={size} />;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "template", "symbol": "TemplateButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
        parameter.type_text.as_deref(),
        Some("`size-${string}` | \"auto\"")
    );
    assert_eq!(parameter.type_kind, None);
    assert!(parameter.allowed_values.is_empty());
    Ok(())
}

#[test]
fn discover_registry_details_classifies_syntax_proven_parameter_kinds() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("FactButton.tsx"),
        r#"interface FactButtonProps {
  label: string;
  count: number;
  onPress: () => void;
  items: string[];
  pair: [string, number];
  settings: { compact: boolean };
  metadata: object;
}
export function FactButton(props: FactButtonProps) {
  return <button>{props.label}</button>;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "facts", "symbol": "FactButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
        parameters
            .iter()
            .map(|parameter| parameter.type_kind)
            .collect::<Vec<_>>(),
        vec![
            Some(RegistryParameterTypeKind::String),
            Some(RegistryParameterTypeKind::Number),
            Some(RegistryParameterTypeKind::Function),
            Some(RegistryParameterTypeKind::Collection),
            Some(RegistryParameterTypeKind::Collection),
            Some(RegistryParameterTypeKind::Object),
            Some(RegistryParameterTypeKind::Object),
        ]
    );
    Ok(())
}

#[test]
fn discover_registry_details_marks_inherited_props_partial() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("ExtendedButton.tsx"),
        r#"interface BaseProps {
  id: string;
}
export interface ExtendedProps extends BaseProps {
  label: string;
}
export function ExtendedButton({ label }: ExtendedProps) {
  return <button>{label}</button>;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "extended", "symbol": "ExtendedButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
                "path": "src/ExtendedButton.tsx",
                "start_line": 7,
                "end_line": 9
            },
            "signature": {
                "type_text": "export function ExtendedButton({ label }: ExtendedProps)",
                "parameters": [{
                    "name": "label",
                    "type_text": "string",
                    "required": true
                }]
            },
            "field_gaps": [{
                "field": "signature.parameters",
                "reason": "props type `ExtendedProps` inherits fields that require type resolution"
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
        src.join("tokens.ts"),
        "export const tokens = { color: { primary: '#fff' }, spacing: { sm: '4px' } };\n",
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "tokens": [
            {
                "id": "color.primary",
                "key": "tokens.color.primary",
                "category": "color",
                "value": "#000",
                "metadata": {"sources": [{
                    "kind": "declaration",
                    "path": "src/tokens.ts",
                    "start_line": 1,
                    "end_line": 1,
                    "language": "react"
                }]}
            },
            {
                "id": "spacing.sm",
                "key": "tokens.spacing.sm",
                "category": "spacing",
                "value": "8px"
            }
        ]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
                "key": "tokens.color.primary",
                "category": "color",
                "value": "#000",
                "metadata": {"sources": [{
                    "kind": "declaration",
                    "path": "src/tokens.ts",
                    "start_line": 1,
                    "end_line": 1,
                    "language": "react"
                }]},
                "extracted": {
                    "status": "complete",
                    "source_evidence": {
                        "path": "src/tokens.ts",
                        "start_line": 1,
                        "end_line": 1
                    }
                }
            },
            {
                "id": "spacing.sm",
                "key": "tokens.spacing.sm",
                "category": "spacing",
                "value": "8px"
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
    fs::write(src.join("Broken.tsx"), "export const Broken = (")?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "broken",
            "symbol": "Broken",
            "extracted": {
                "status": "complete",
                "documentation": "Prior documentation",
                "source_evidence": {
                    "path": "src/Broken.tsx",
                    "start_line": 1,
                    "end_line": 1
                }
            }
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
                "path": "src/Broken.tsx",
                "start_line": 1,
                "end_line": 1
            }
        })
    );
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}

#[test]
fn discover_registry_details_does_not_stale_unrelated_components_on_damage() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(src.join("Broken.tsx"), "export const Broken = (")?;
    fs::write(
        src.join("Intact.tsx"),
        "export function Intact() { return <span />; }",
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "other",
            "symbol": "Other",
            "extracted": {
                "status": "complete",
                "documentation": "Prior documentation",
                "source_evidence": {
                    "path": "src/Other.tsx",
                    "start_line": 1,
                    "end_line": 3
                }
            }
        }]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
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
fn discover_registry_details_extracts_inline_props_type_literal() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Inline.tsx"),
        r#"export function Inline({ label }: { label: string; optional?: boolean }) {
  return <button>{label}</button>;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "inline", "symbol": "Inline"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let extracted = result.document.components[0].extracted.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(extracted).unwrap(),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "src/Inline.tsx",
                "start_line": 1,
                "end_line": 3
            },
            "signature": {
                "type_text": "export function Inline({ label }: { label: string; optional?: boolean })",
                "parameters": [
                    {
                        "name": "label",
                        "type_text": "string",
                        "type_kind": "string",
                        "required": true
                    },
                    {
                        "name": "optional",
                        "type_text": "boolean",
                        "type_kind": "boolean",
                        "required": false
                    }
                ]
            }
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_extracts_memo_forward_ref_and_default_exports() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("Wrapped.tsx"),
        r#"import React, { forwardRef, memo } from "react";

interface MemoProps {
  label: string;
}

function MemoBase({ label }: MemoProps) {
  return <button>{label}</button>;
}

export const MemoButton = memo(MemoBase);

export const RefInput = forwardRef(function RefInput(
  { value }: { value: string },
  _ref: React.Ref<HTMLInputElement>,
) {
  return <input value={value} />;
});

export default function DefaultPanel({ title }: { title: string }) {
  return <aside>{title}</aside>;
}
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [
            {"id": "memo", "symbol": "MemoButton"},
            {"id": "ref", "symbol": "RefInput"},
            {"id": "default", "symbol": "DefaultPanel"}
        ]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    let by_symbol = |symbol: &str| {
        result
            .document
            .components
            .iter()
            .find(|component| component.symbol == symbol)
            .and_then(|component| component.extracted.as_ref())
            .expect("component should be enriched")
    };

    let memo = by_symbol("MemoButton");
    assert_eq!(
        memo.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert_eq!(
        memo.signature
            .as_ref()
            .map(|signature| signature.parameters.len()),
        Some(1)
    );
    assert_eq!(
        memo.signature.as_ref().and_then(|signature| {
            signature
                .parameters
                .iter()
                .find(|parameter| parameter.name == "label")
                .and_then(|parameter| parameter.type_text.as_deref())
        }),
        Some("string")
    );

    let ref_input = by_symbol("RefInput");
    assert_eq!(
        ref_input.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert_eq!(
        ref_input.signature.as_ref().and_then(|signature| {
            signature
                .parameters
                .iter()
                .find(|parameter| parameter.name == "value")
                .and_then(|parameter| parameter.type_text.as_deref())
        }),
        Some("string")
    );

    let default_panel = by_symbol("DefaultPanel");
    assert_eq!(
        default_panel.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert_eq!(
        default_panel.signature.as_ref().and_then(|signature| {
            signature
                .parameters
                .iter()
                .find(|parameter| parameter.name == "title")
                .and_then(|parameter| parameter.type_text.as_deref())
        }),
        Some("string")
    );
    Ok(())
}

#[test]
fn discover_registry_details_is_idempotent_on_clean_reruns() {
    let repo_root = fixture_root();
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "rich", "symbol": "RichButton"}]
    }))
    .unwrap();
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        roots: vec!["design-system/src".to_owned()],
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
fn discover_registry_symbols_emits_exported_public_components() {
    let root = fixture_root().join("design-system/src");

    let result =
        discover_registry_symbols(&fixture_root(), &[root]).expect("discover should succeed");

    assert_eq!(
        result.symbols(),
        vec![
            "Button".to_owned(),
            "Card".to_owned(),
            "DefaultMemo".to_owned(),
            "DefaultPanel".to_owned(),
            "DefaultReactMemo".to_owned(),
            "Dialog".to_owned(),
            "InlineMemo".to_owned(),
            "InlineRef".to_owned(),
            "MemoButton".to_owned(),
            "RichButton".to_owned(),
            "TextInput".to_owned(),
        ]
    );
}

#[test]
fn react_language_discover_resolves_repo_relative_roots() {
    let repo_root = fixture_root();
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: 1,
        language_id: "react".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().to_string(),
        roots: vec!["design-system/src".to_owned()],
    };

    let result = ReactLanguage::new()
        .discover(&request)
        .expect("discover should succeed");

    assert!(result.diagnostics.is_empty());
    assert!(result.symbols().contains(&"Button".to_owned()));
    assert_eq!(
        result
            .components
            .iter()
            .find(|component| component.symbol == "Button")
            .and_then(|component| component.package.as_deref()),
        Some("@acme/design-system")
    );
    assert!(!result.symbols().contains(&"PrivateBadge".to_owned()));
    assert!(!result.symbols().contains(&"lowerBadge".to_owned()));
    assert!(!result.symbols().contains(&"notComponent".to_owned()));
    assert!(!result.symbols().contains(&"FactoryMemo".to_owned()));
    assert!(!result.symbols().contains(&"FactoryRef".to_owned()));
}

#[test]
fn missing_root_fails() {
    let missing = fixture_root().join("missing");

    let err = discover_registry_symbols(&fixture_root(), &[missing])
        .expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_skipped_with_diagnostics() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let broken_file = tempdir.path().join("Broken.tsx");
    fs::write(&broken_file, "export const Broken = (")?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert!(result.symbols().is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert_eq!(
        result.diagnostics[0]
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some("Broken.tsx")
    );
    Ok(())
}

#[test]
fn parse_failures_do_not_block_symbols_from_other_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    fs::write(
        tempdir.path().join("Button.tsx"),
        "export function Button() { return <button />; }",
    )?;
    fs::write(tempdir.path().join("Broken.tsx"), "export const Broken = (")?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols(), vec!["Button".to_owned()]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}

#[test]
fn discover_detects_chained_memo_forward_ref_exports() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    fs::write(
        tempdir.path().join("Wrapped.tsx"),
        r#"
        import { forwardRef, memo } from "react";

        export const ChainedInput = memo(forwardRef(function ChainedInput() {
            return <input />;
        }));
        "#,
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols(), vec!["ChainedInput".to_owned()]);
    Ok(())
}

#[test]
fn discover_detects_default_chained_memo_forward_ref_exports() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    fs::write(
        tempdir.path().join("DefaultWrapped.tsx"),
        r#"
        import { forwardRef, memo } from "react";

        export default memo(forwardRef(function DefaultChainedInput() {
            return <input />;
        }));
        "#,
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols(), vec!["DefaultChainedInput".to_owned()]);
    Ok(())
}

#[test]
fn discover_detects_default_forward_ref_exports() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    fs::write(
        tempdir.path().join("DefaultInput.tsx"),
        r#"
        import { forwardRef } from "react";

        export default forwardRef(function DefaultInput() {
            return <input />;
        });
        "#,
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols(), vec!["DefaultInput".to_owned()]);
    Ok(())
}

#[test]
fn discover_defers_aliased_named_exports() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    fs::write(
        tempdir.path().join("Button.tsx"),
        r#"
        function Button() {
            return <button />;
        }

        export { Button as Alias };
        "#,
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert!(result.symbols().is_empty());
    Ok(())
}

#[test]
fn invalid_language_id_fails() {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: 1,
        language_id: "compose".try_into().unwrap(),
        repo_root: fixture_root().to_string_lossy().to_string(),
        roots: vec!["design-system/src".to_owned()],
    };

    let err = ReactLanguage::new()
        .discover(&request)
        .expect_err("wrong language should fail");

    assert!(err.to_string().contains("invalid react language id"));
}

#[test]
fn discover_merges_symbols_from_multiple_roots_with_per_package_metadata() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let pkg_a = tempdir.path().join("packages/pkg-a");
    let pkg_b = tempdir.path().join("packages/pkg-b");
    let root_a = pkg_a.join("src");
    let root_b = pkg_b.join("src");
    fs::create_dir_all(&root_a)?;
    fs::create_dir_all(&root_b)?;
    fs::write(pkg_a.join("package.json"), r#"{"name":"@acme/pkg-a"}"#)?;
    fs::write(pkg_b.join("package.json"), r#"{"name":"@acme/pkg-b"}"#)?;
    fs::write(
        root_a.join("Alpha.tsx"),
        "export function Alpha() { return <a />; }",
    )?;
    fs::write(
        root_b.join("Beta.tsx"),
        "export function Beta() { return <b />; }",
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[root_a, root_b])
        .expect("discover should succeed");

    assert_eq!(
        result.symbols(),
        vec!["Alpha".to_owned(), "Beta".to_owned()]
    );
    let package_for = |symbol: &str| {
        result
            .components
            .iter()
            .find(|component| component.symbol == symbol)
            .and_then(|component| component.package.as_deref())
    };
    assert_eq!(package_for("Alpha"), Some("@acme/pkg-a"));
    assert_eq!(package_for("Beta"), Some("@acme/pkg-b"));
    assert!(result.diagnostics.is_empty());
    Ok(())
}

#[test]
fn discover_omits_package_when_symbol_appears_in_multiple_npm_packages() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let pkg_a = tempdir.path().join("packages/pkg-a");
    let pkg_b = tempdir.path().join("packages/pkg-b");
    let root_a = pkg_a.join("src");
    let root_b = pkg_b.join("src");
    fs::create_dir_all(&root_a)?;
    fs::create_dir_all(&root_b)?;
    fs::write(pkg_a.join("package.json"), r#"{"name":"@acme/pkg-a"}"#)?;
    fs::write(pkg_b.join("package.json"), r#"{"name":"@acme/pkg-b"}"#)?;
    fs::write(
        root_a.join("Shared.tsx"),
        "export function Shared() { return <a />; }",
    )?;
    fs::write(
        root_b.join("Shared.tsx"),
        "export function Shared() { return <b />; }",
    )?;

    let result = discover_registry_symbols(tempdir.path(), &[root_a, root_b])
        .expect("discover should succeed");

    assert_eq!(result.symbols(), vec!["Shared".to_owned()]);
    assert_eq!(
        result
            .components
            .iter()
            .find(|component| component.symbol == "Shared")
            .and_then(|component| component.package.as_deref()),
        None
    );
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "discover_package_conflict");
    Ok(())
}

#[test]
fn discover_skips_excluded_source_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root = tempdir.path().join("src");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("Button.tsx"),
        "export function Button() { return <button />; }",
    )?;
    fs::write(
        root.join("Button.test.tsx"),
        "export function ButtonTest() { return <button />; }",
    )?;
    fs::write(
        root.join("Card.stories.tsx"),
        "export function CardStory() { return <section />; }",
    )?;

    let result =
        discover_registry_symbols(tempdir.path(), &[root]).expect("discover should succeed");

    assert_eq!(result.symbols(), vec!["Button".to_owned()]);
    Ok(())
}

#[test]
fn discover_skips_barrel_only_roots_without_implementation_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root = tempdir.path().join("src");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("index.ts"),
        r#"export { Button } from "./components";"#,
    )?;

    let result =
        discover_registry_symbols(tempdir.path(), &[root]).expect("discover should succeed");

    assert!(result.symbols().is_empty());
    Ok(())
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}
