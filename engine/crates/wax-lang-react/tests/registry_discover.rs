use std::path::PathBuf;
use std::{fs, io};

use wax_contract::RegistryDocument;
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
                "start_line": 22,
                "end_line": 24
            },
            "documentation": "A button used for important actions.",
            "signature": {
                "type_text": "export function RichButton({ tone = \"strong\", disabled }: ButtonProps)",
                "parameters": [
                    {
                        "name": "tone",
                        "type_text": "\"strong\" | \"quiet\"",
                        "default": "\"strong\"",
                        "required": false
                    },
                    {
                        "name": "disabled",
                        "type_text": "boolean",
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
            }]
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
                "documentation": "Prior documentation"
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
            "documentation": "Prior documentation"
        })
    );
    assert_eq!(result.diagnostics[0].code, "parse_failed");
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
