use std::path::PathBuf;
use std::{fs, io};

use wax_contract::{RegistryDetails, RegistryDocument, RegistryParameterTypeKind};
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
    let details = result.document.components[0]
        .details
        .as_ref()
        .expect("matching component should be enriched");

    assert_eq!(
        serde_json::to_value(details).unwrap(),
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

fn discover_documented_button(source: &str) -> io::Result<RegistryDetails> {
    discover_button_with_sources(source, &[])
}

fn discover_button_with_sources(
    source: &str,
    siblings: &[(&str, &str)],
) -> io::Result<RegistryDetails> {
    let repo = tempfile::tempdir()?;
    fs::create_dir(repo.path().join("src"))?;
    fs::write(repo.path().join("src/Button.tsx"), source)?;
    for (path, contents) in siblings {
        let path = repo.path().join(path);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, contents)?;
    }
    let result = discover_registry_details(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: repo.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry: serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "components": [{"id": "button", "symbol": "Button"}]
        }))
        .unwrap(),
    })
    .expect("discover button");
    Ok(result
        .document
        .components
        .into_iter()
        .next()
        .unwrap()
        .details
        .unwrap())
}

#[test]
fn discover_registry_details_routes_jsdoc_tag_continuations() -> io::Result<()> {
    let details = discover_documented_button(
        r#"/** Shows a button.
 * @guidance Prefer concise labels
 * when space is limited.
 *
 * Use in toolbars.
 * @see AnotherButton
 * See its compact variant.
 * @returns A button
 * with the supplied label.
 * @remarks Internal notes
 * about the implementation.
 * @param props Button props
 * supplied by the caller.
 *
 * Use in dialogs too.
 */
export function Button() {
    return <button />;
}
"#,
    )?;

    assert_eq!(
        details
            .best_practices
            .iter()
            .map(|rule| rule.description.as_str())
            .collect::<Vec<_>>(),
        ["Prefer concise labels when space is limited."]
    );
    assert_eq!(
        details.documentation.as_deref(),
        Some("Shows a button.\nUse in toolbars.\nUse in dialogs too.")
    );
    Ok(())
}

#[test]
fn discover_registry_details_combines_fenced_and_unfenced_jsdoc_examples() -> io::Result<()> {
    let details = discover_documented_button(
        r#"/**
 * @example
 * ```tsx
 * <Button label="Save" />
 *
 * <Button label="Cancel" />
 * ```
 * @example <Button label="Close" />
 *
 * <Button label="Exit" />
 */
export function Button() {
    return <button />;
}
"#,
    )?;

    assert_eq!(
        details.attachments[0].content,
        "<Button label=\"Save\" />\n\n<Button label=\"Cancel\" />\n<Button label=\"Close\" />\n<Button label=\"Exit\" />"
    );
    Ok(())
}

#[test]
fn discover_registry_details_uses_same_file_named_export_declarations() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        tempdir.path().join("package.json"),
        r#"{"name": "@acme/design-system"}"#,
    )?;
    fs::write(
        src.join("button.tsx"),
        r#"/** A button for important actions. */
function Button({ label }: { label: string }) {
    return <button>{label}</button>;
}

export { Button };
"#,
    )?;
    fs::write(
        src.join("controls.tsx"),
        r#"import { memo } from "react";

export { ArrowButton, MemoButton };

/** An arrow button. */
const ArrowButton = ({ label }: { label: string }) =>
    <button>{label}</button>;

/** A value shared by the controls. */
const defaultLabel = "OK",
    GroupedButton = ({ label }: { label: string }) =>
        <button>{label || defaultLabel}</button>;

/** A memoized button. */
const MemoButton = memo(ArrowButton);

export { GroupedButton };
"#,
    )?;
    fs::write(
        src.join("excluded.tsx"),
        r#"import { ImportedButton } from "../external";

function TypeButton() { return <button />; }
function InlineTypeButton() { return <button />; }
function RemoteButton() { return <button />; }
function AliasedButton() { return <button />; }
declare function AmbientButton(): React.JSX.Element;

export type { TypeButton };
export { type InlineTypeButton, ImportedButton, AmbientButton };
export { RemoteButton } from "../external";
export { AliasedButton as PublicButton };
"#,
    )?;
    fs::write(
        tempdir.path().join("external.tsx"),
        r#"export function ImportedButton() { return <button />; }
export function RemoteButton() { return <button />; }
"#,
    )?;

    let symbols = [
        "Button",
        "ArrowButton",
        "GroupedButton",
        "MemoButton",
        "TypeButton",
        "InlineTypeButton",
        "ImportedButton",
        "AmbientButton",
        "RemoteButton",
        "AliasedButton",
        "PublicButton",
    ];
    let components: Vec<_> = symbols
        .iter()
        .map(|symbol| {
            serde_json::json!({
                "id": format!("react.{}", symbol.to_ascii_lowercase()),
                "symbol": symbol,
                "package": "@acme/design-system"
            })
        })
        .collect();

    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry: serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "components": components
        }))
        .unwrap(),
    };

    let result = discover_registry_details(&request).expect("rich discover should succeed");
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        ["discover_export_unresolved", "discover_export_unresolved"]
    );
    let expected_declarations = [
        (
            "src/button.tsx",
            2,
            4,
            Some("A button for important actions."),
            "function Button({ label }: { label: string })",
        ),
        (
            "src/controls.tsx",
            6,
            7,
            Some("An arrow button."),
            "const ArrowButton = ({ label }: { label: string }) =>",
        ),
        (
            "src/controls.tsx",
            11,
            12,
            None,
            "GroupedButton = ({ label }: { label: string }) =>",
        ),
        (
            "src/controls.tsx",
            15,
            15,
            Some("A memoized button."),
            "({ label }: { label: string }) =>",
        ),
    ];
    let (local, excluded) = result
        .document
        .components
        .split_at(expected_declarations.len());
    for (component, (path, start_line, end_line, documentation, signature)) in
        local.iter().zip(expected_declarations)
    {
        let details = component.details.as_ref().unwrap();
        let mut expected = serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": path,
                "start_line": start_line,
                "end_line": end_line
            },
            "signature": {
                "type_text": signature,
                "parameters": [{
                    "name": "label",
                    "type_text": "string",
                    "type_kind": "string",
                    "required": true
                }]
            }
        });
        if let Some(documentation) = documentation {
            expected["documentation"] = serde_json::json!(documentation);
        }
        assert_eq!(
            serde_json::to_value(details).unwrap(),
            expected,
            "{}",
            component.symbol
        );
    }

    let (public_alias, excluded) = excluded.split_last().unwrap();
    let alias_details = public_alias.details.as_ref().unwrap();
    assert_eq!(public_alias.symbol, "PublicButton");
    assert_eq!(
        alias_details.source_evidence.as_ref().unwrap().path,
        "src/excluded.tsx"
    );
    for component in excluded {
        let details = component.details.as_ref().unwrap();
        assert_eq!(
            details.status,
            wax_contract::RegistryExtractionStatus::Unavailable,
            "{}",
            component.symbol
        );
        assert!(details.source_evidence.is_none(), "{}", component.symbol);
        assert!(details.signature.is_none(), "{}", component.symbol);
        assert_eq!(details.field_gaps[0].field, "source_evidence");
    }

    Ok(())
}

#[test]
fn discover_registry_details_records_imported_props_package_without_a_gap() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("ImportedButton.tsx"),
        r#"import type { ImportedProps } from "@acme/ui";
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
    let details = result.document.components[0].details.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(details).unwrap(),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "src/ImportedButton.tsx",
                "start_line": 2,
                "end_line": 2
            },
            "signature": {
                "type_text": "export const ImportedButton = ({ size }: ImportedProps) =>",
                "parameters": [{
                    "name": "props",
                    "type_text": "ImportedProps",
                    "package": "@acme/ui",
                    "required": true
                }]
            }
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_omits_unobserved_qualified_props_package() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let src = tempdir.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("QualifiedButton.tsx"),
        r#"export const QualifiedButton = (props: React.HTMLAttributes<HTMLButtonElement>) => <button {...props} />;
"#,
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "qualified", "symbol": "QualifiedButton"}]
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
    let details = result.document.components[0].details.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(details).unwrap(),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "src/QualifiedButton.tsx",
                "start_line": 1,
                "end_line": 1
            },
            "signature": {
                "type_text": "export const QualifiedButton = (props: React.HTMLAttributes<HTMLButtonElement>) =>",
                "parameters": [{
                    "name": "props",
                    "type_text": "React.HTMLAttributes<HTMLButtonElement>",
                    "required": true
                }]
            }
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_decodes_string_literal_union_values() -> io::Result<()> {
    let details = discover_documented_button(
        r#"export function Button({ variant = 'primary' }: {
  variant?: 'primary' | 'secondary' | 'it\'s quoted' | "\u0071uiet";
}) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(
        parameter.allowed_values,
        ["primary", "secondary", "it's quoted", "quiet"]
    );
    assert_eq!(parameter.default.as_deref(), Some("'primary'"));
    Ok(())
}

#[test]
fn discover_registry_details_resolves_same_file_literal_union_alias() -> io::Result<()> {
    let details = discover_documented_button(
        r#"interface Props {
  variant?: Variant;
}
export function Button({ variant }: Props) {
  return <button data-variant={variant} />;
}
type Variant = 'primary' | 'secondary';"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_text.as_deref(), Some("Variant"));
    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_resolves_exported_literal_union_alias() -> io::Result<()> {
    let details = discover_documented_button(
        r#"export type Variant = "primary" | "secondary";
type Props = { variant: Variant };
export function Button({ variant }: Props) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_does_not_resolve_a_shadowing_generic_parameter() -> io::Result<()> {
    let details = discover_documented_button(
        r#"type Variant = 'primary' | 'secondary';
export function Button<Variant>({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_text.as_deref(), Some("Variant"));
    assert_eq!(parameter.type_kind, None);
    assert!(parameter.allowed_values.is_empty());
    Ok(())
}

#[test]
fn discover_registry_details_resolves_relative_literal_union_alias() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types';
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[(
            "src/types.ts",
            "export type Variant = 'primary' | 'secondary';",
        )],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    assert_eq!(parameter.package, None);
    Ok(())
}

#[test]
fn discover_registry_details_resolves_javascript_specifier_to_typescript_source() -> io::Result<()>
{
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types.js';
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[(
            "src/types.ts",
            "export type Variant = 'primary' | 'secondary';",
        )],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_prefers_declaration_over_javascript_implementation() -> io::Result<()>
{
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types';
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[
            ("src/types.js", "export const Variant = 'runtime';"),
            (
                "src/types.d.ts",
                "export type Variant = 'primary' | 'secondary';",
            ),
        ],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_resolves_relative_directory_index_enum() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import { Variant } from './variants';
interface Props { variant: Variant }
export function Button({ variant }: Props) {
  return <button data-variant={variant} />;
}"#,
        &[(
            "src/variants/index.ts",
            "export enum Variant { Primary, Secondary }",
        )],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["Primary", "Secondary"]);
    assert_eq!(parameter.package, None);
    Ok(())
}

#[test]
fn discover_registry_details_follows_renamed_relative_import() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import { type Variant as V } from './types.ts';
export function Button({ variant }: { variant: V }) {
  return <button data-variant={variant} />;
}"#,
        &[(
            "src/types.ts",
            "export type Variant = 'primary' | 'secondary';",
        )],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    assert_eq!(parameter.type_text.as_deref(), Some("V"));
    assert_eq!(parameter.package, None);
    Ok(())
}

#[test]
fn discover_registry_details_resolves_one_named_enum_reexport() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from './barrels/variants';
export function Button({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[
            (
                "src/barrels/variants.ts",
                "export type { Tone as Variant } from '../types';",
            ),
            (
                "src/types.ts",
                "export type Tone = 'primary' | 'secondary';",
            ),
        ],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    assert_eq!(parameter.package, None);
    Ok(())
}

#[test]
fn discover_registry_details_resolves_a_local_type_export_alias() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types';
export function Button({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[(
            "src/types.ts",
            "type Tone = 'primary' | 'secondary'; export type { Tone as Variant };",
        )],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["primary", "secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_keeps_outside_root_import_opaque() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from '../types';
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[("types.ts", "export type Variant = 'primary' | 'secondary';")],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(
        serde_json::to_value(parameter).unwrap(),
        serde_json::json!({
            "name": "variant",
            "type_text": "Variant",
            "package": "../types",
            "required": false
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_resolves_declaration_file_enums() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types';
import type { Size } from './sizes';
export function Button({ variant, size }: { variant: Variant; size: Size }) {
  return <button data-variant={variant} data-size={size} />;
}"#,
        &[
            (
                "src/types.d.ts",
                "export declare enum Variant { Primary, Secondary }",
            ),
            (
                "src/sizes/index.d.ts",
                "export type Size = 'small' | 'large';",
            ),
        ],
    )?;
    let parameters = details.signature.unwrap().parameters;

    assert_eq!(
        parameters[0].type_kind,
        Some(RegistryParameterTypeKind::Enum)
    );
    assert_eq!(parameters[0].allowed_values, ["Primary", "Secondary"]);
    assert_eq!(
        parameters[1].type_kind,
        Some(RegistryParameterTypeKind::Enum)
    );
    assert_eq!(parameters[1].allowed_values, ["small", "large"]);
    Ok(())
}

#[test]
fn discover_registry_details_ignores_damaged_relative_enum_targets() -> io::Result<()> {
    let details = discover_button_with_sources(
        r#"import type { Variant } from './types';
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
        &[
            (
                "src/types.ts",
                "export type Variant = 'primary' | 'secondary'; const = ;",
            ),
            ("src/types/index.ts", "export enum Variant { WrongTarget }"),
        ],
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(
        serde_json::to_value(parameter).unwrap(),
        serde_json::json!({
            "name": "variant",
            "type_text": "Variant",
            "package": "./types",
            "required": false
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_keeps_imported_alias_opaque() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import type { Variant } from "@acme/types";
export function Button({ variant }: { variant?: Variant }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(
        serde_json::to_value(parameter).unwrap(),
        serde_json::json!({
            "name": "variant",
            "type_text": "Variant",
            "package": "@acme/types",
            "required": false
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_uses_enum_member_names() -> io::Result<()> {
    let details = discover_documented_button(
        r#"export enum Variant {
  Primary = "primary",
  Secondary = "secondary",
  "high-contrast" = "contrast",
}
export function Button({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(
        parameter.allowed_values,
        ["Primary", "Secondary", "high-contrast"]
    );
    Ok(())
}

#[test]
fn discover_registry_details_combines_merged_enum_declarations() -> io::Result<()> {
    let details = discover_documented_button(
        r#"enum Variant { Primary = 'primary' }
enum Variant { Secondary = 'secondary' }
export function Button({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let parameter = &details.signature.unwrap().parameters[0];

    assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(parameter.allowed_values, ["Primary", "Secondary"]);
    Ok(())
}

#[test]
fn discover_registry_details_reads_const_and_ambient_enums() -> io::Result<()> {
    let details = discover_documented_button(
        r#"const enum Size { Small, Large }
declare enum Tone { Quiet, Strong }
export function Button({ size, tone }: { size: Size; tone: Tone }) {
  return <button data-size={size} data-tone={tone} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    for (parameter, names) in signature
        .parameters
        .iter()
        .zip([["Small", "Large"], ["Quiet", "Strong"]])
    {
        assert_eq!(parameter.type_kind, Some(RegistryParameterTypeKind::Enum));
        assert_eq!(parameter.allowed_values, names);
    }
    assert_eq!(signature.parameters.len(), 2);
    Ok(())
}

#[test]
fn discover_registry_details_omits_enum_facts_for_union_with_undefined() -> io::Result<()> {
    let details = discover_documented_button(
        r#"type Variant = 'primary' | undefined;
export function Button({ variant }: { variant: Variant }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_kind, None);
    assert!(parameter.allowed_values.is_empty());
    Ok(())
}

#[test]
fn discover_registry_details_omits_enum_facts_for_an_empty_string_value() -> io::Result<()> {
    let details = discover_documented_button(
        r#"export function Button({ variant }: { variant: '' | 'primary' }) {
  return <button data-variant={variant} />;
}"#,
    )?;
    let signature = details.signature.unwrap();
    let parameter = &signature.parameters[0];

    assert_eq!(parameter.type_text.as_deref(), Some("'' | 'primary'"));
    assert_eq!(parameter.type_kind, None);
    assert!(parameter.allowed_values.is_empty());
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
        .details
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
        .details
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
        .details
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
        .details
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
        r#"type Variant = 'primary' | 'secondary';
interface BaseProps {
  id: string;
}
export interface ExtendedProps extends BaseProps {
  label: string;
  variant?: Variant;
}
export function ExtendedButton({ label, variant }: ExtendedProps) {
  return <button data-variant={variant}>{label}</button>;
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
    let details = result.document.components[0].details.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(details).unwrap(),
        serde_json::json!({
            "status": "partial",
            "source_evidence": {
                "path": "src/ExtendedButton.tsx",
                "start_line": 9,
                "end_line": 11
            },
            "signature": {
                "type_text": "export function ExtendedButton({ label, variant }: ExtendedProps)",
                "parameters": [{
                    "name": "label",
                    "type_text": "string",
                    "required": true
                }, {
                    "name": "variant",
                    "type_text": "Variant",
                    "required": false
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
                "details": {
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
            "details": {
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
        serde_json::to_value(result.document.components[0].details.as_ref().unwrap()).unwrap(),
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
            "details": {
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
    let details = result.document.components[0].details.as_ref().unwrap();

    assert_eq!(
        details.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert_eq!(
        details.documentation.as_deref(),
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
    let details = result.document.components[0].details.as_ref().unwrap();

    assert_eq!(
        serde_json::to_value(details).unwrap(),
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
            .and_then(|component| component.details.as_ref())
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
            "Dialog".to_owned(),
            "MemoButton".to_owned(),
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
fn discover_uses_the_alias_for_a_named_export_without_a_barrel() -> io::Result<()> {
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

    assert_eq!(result.symbols(), ["Alias"]);
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
fn discover_only_recognizes_memo_imported_from_react() -> io::Result<()> {
    let result = discover_files(
        &["src"],
        &[
            (
                "src/memo.tsx",
                r#"
                import { memo } from "react";
                function Base() { return <button />; }
                export const MemoButton = memo(Base);
            "#,
            ),
            (
                "src/fake-memo.tsx",
                r#"
                function memo(component) { return component; }
                function Base() { return <button />; }
                export const FakeMemo = memo(Base);
            "#,
            ),
        ],
    )?;
    assert_eq!(result.symbols(), ["MemoButton"]);
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

#[test]
fn discover_prefers_the_root_index_and_keeps_loose_roots_independent() -> io::Result<()> {
    let result = discover_files(
        &["src", "src", "src/nested", "loose"],
        &[
            (
                "src/index.tsx",
                "export function Public() { return <p />; }",
            ),
            (
                "src/index.jsx",
                "export function LowerPriority() { return <p />; }",
            ),
            (
                "src/internal.tsx",
                "export function Internal() { return <p />; }",
            ),
            (
                "src/nested/index.tsx",
                "export function Nested() { return <p />; }",
            ),
            (
                "loose/button.tsx",
                "export function Loose() { return <p />; }",
            ),
        ],
    )?;

    assert_eq!(result.symbols(), ["Loose", "Nested", "Public"]);
    Ok(())
}

#[test]
fn discover_uses_the_in_scope_package_entry_across_roots() -> io::Result<()> {
    let result = discover_files(
        &["a/src", "a/other", "b/src", "b/other"],
        &[
            (
                "a/package.json",
                r#"{"name":"ui","exports":{".":{"import":"./dist/index.js","source":"./src/public.tsx"},"./other":"./other/internal.tsx"},"source":"./src/index.tsx"}"#,
            ),
            (
                "a/src/public.tsx",
                "export function Public() { return <p />; }",
            ),
            (
                "a/src/index.tsx",
                "export function InternalIndex() { return <p />; }",
            ),
            (
                "a/other/internal.tsx",
                "export function Internal() { return <p />; }",
            ),
            (
                "a/dist/index.js",
                "export function Built() { return <p />; }",
            ),
            (
                "b/package.json",
                r#"{"name":"ui","main":"./dist/index.js"}"#,
            ),
            (
                "b/src/index.tsx",
                "export function SeparateIndex() { return <p />; }",
            ),
            (
                "b/src/internal.tsx",
                "export function SeparateInternal() { return <p />; }",
            ),
            (
                "b/other/loose.tsx",
                "export function Loose() { return <p />; }",
            ),
        ],
    )?;

    assert_eq!(result.symbols(), ["Loose", "Public", "SeparateIndex"]);
    assert!(
        result
            .components
            .iter()
            .all(|entry| entry.package.as_deref() == Some("ui"))
    );
    Ok(())
}

#[test]
fn discover_follows_public_aliases_to_value_declarations() -> io::Result<()> {
    let result = discover_files(
        &["src"],
        &[
            (
                "src/index.ts",
                r#"
                export { LocalButton as Button, default as Icon } from "./controls";
                export { Fallback as default } from "./fallback";
                import { LocalButton as Imported } from "./controls";
                export { Imported as Action };
            "#,
            ),
            (
                "src/controls.tsx",
                r#"
                function implementation() { return <button />; }
                export { implementation as LocalButton };
                export default function InternalIcon() { return <svg />; }
            "#,
            ),
            (
                "src/fallback.tsx",
                r#"
                import { memo } from "react";
                export function Fallback() { return <button />; }
                export default memo(function UnrelatedDefault() { return <p />; });
                "#,
            ),
        ],
    )?;

    assert_eq!(result.symbols(), ["Action", "Button", "Fallback", "Icon"]);
    Ok(())
}

#[test]
fn discover_follows_nested_stars_and_namespaces_through_js_specifiers() -> io::Result<()> {
    let result = discover_files(
        &["src"],
        &[
            (
                "src/index.ts",
                r#"
                export * as Dialog from "./dialog.js";
                import * as Controls from "./controls.jsx";
                export { Controls };
            "#,
            ),
            ("src/dialog.ts", r#"export * from "./middle";"#),
            ("src/middle.ts", r#"export * from "./panel";"#),
            (
                "src/panel.tsx",
                r#"
                function DialogRoot() { return <dialog />; }
                export { DialogRoot as Root };
                export default function StarDefault() { return <p />; }
            "#,
            ),
            (
                "src/controls.tsx",
                "export function Button() { return <p />; }",
            ),
            (
                "src/internal.tsx",
                "export function BaseButton() { return <p />; }",
            ),
        ],
    )?;

    assert_eq!(result.symbols(), ["Controls.Button", "Dialog.Root"]);
    assert!(result.diagnostics.is_empty());
    Ok(())
}

#[test]
fn discover_omits_ambiguous_star_exports_and_resolves_cycles_by_origin() -> io::Result<()> {
    let result = discover_files(
        &["src"],
        &[
            (
                "src/index.ts",
                r#"
                export * from "./a";
                export * from "./b";
                export { Winner } from "./a";
                export { Cycle } from "./a";
                export { Conflict as NestedConflict } from "./nested";
            "#,
            ),
            (
                "src/a.tsx",
                r#"
                export * from "./index";
                export { Cycle } from "./index";
                export { Shared } from "./shared";
                export function Conflict() { return <a />; }
                export function Winner() { return <a />; }
            "#,
            ),
            (
                "src/b.tsx",
                r#"
                export { Shared } from "./shared";
                export function Conflict() { return <b />; }
                export function Winner() { return <b />; }
            "#,
            ),
            (
                "src/nested.ts",
                r#"export * from "./a"; export * from "./b";"#,
            ),
            (
                "src/shared.tsx",
                "export function Shared() { return <p />; }",
            ),
        ],
    )?;

    assert_eq!(result.symbols(), ["Shared", "Winner"]);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "discover_export_ambiguous" && diagnostic.message.contains("Conflict")
    }));
    Ok(())
}

#[test]
fn discover_reports_broken_local_edges_and_skips_external_and_type_exports() -> io::Result<()> {
    let result = discover_files(
        &["src"],
        &[
            (
                "src/index.ts",
                r#"
                export { Broken } from "./broken.js";
                export { Missing } from "./values";
                export * from "./absent";
                export { Outside } from "../outside";
                export { External } from "external";
                export type { TypeOnly } from "./type-only";
                export { type InlineTypeOnly, Visible } from "./values";
            "#,
            ),
            ("src/broken.tsx", "export const Broken = ("),
            (
                "src/values.tsx",
                r#"
                export function InlineTypeOnly() { return <p />; }
                export function Visible() { return <p />; }
            "#,
            ),
            ("outside.tsx", "export function Outside() { return <p />; }"),
        ],
    )?;

    assert_eq!(result.symbols(), ["Visible"]);
    let diagnostics: Vec<_> = result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.location.as_ref().unwrap().file.as_str(),
            )
        })
        .collect();
    assert_eq!(
        diagnostics,
        [
            ("parse_failed", "src/broken.tsx"),
            ("discover_export_unresolved", "src/index.ts"),
            ("discover_export_unresolved", "src/index.ts"),
            ("discover_export_unresolved", "src/index.ts"),
            ("discover_export_unresolved", "src/index.ts"),
        ]
    );
    assert!(result.diagnostics[1].message.contains("src/broken.tsx"));
    assert!(result.diagnostics[2].message.contains("Missing"));
    Ok(())
}

#[test]
fn discover_registry_details_uses_the_public_publisher_and_implementation_evidence()
-> io::Result<()> {
    let result = details_for_files(
        &["publisher/src", "implementation/src"],
        &[
            ("publisher/package.json", r#"{"name":"publisher"}"#),
            (
                "publisher/src/index.ts",
                r#"export * as Dialog from "../../implementation/src/dialog.js";"#,
            ),
            (
                "implementation/package.json",
                r#"{"name":"implementation"}"#,
            ),
            (
                "implementation/src/dialog.tsx",
                r#"/** Controls the dialog. */
function DialogRoot({ modal = true }: { modal?: boolean }) {
    return <dialog open={modal} />;
}
export { DialogRoot as Root };
"#,
            ),
        ],
        serde_json::json!([{"id":"dialog-root","symbol":"Dialog.Root","package":"publisher"}]),
    )?;

    let details = result.document.components[0].details.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(details).unwrap(),
        serde_json::json!({
            "status": "complete",
            "source_evidence": {
                "path": "implementation/src/dialog.tsx",
                "start_line": 2,
                "end_line": 4
            },
            "documentation": "Controls the dialog.",
            "signature": {
                "type_text": "function DialogRoot({ modal = true }: { modal?: boolean })",
                "parameters": [{
                    "name": "modal", "type_text": "boolean", "type_kind": "boolean",
                    "default": "true", "required": false
                }]
            }
        })
    );
    Ok(())
}

#[test]
fn discover_registry_details_falls_back_to_unique_direct_exports() -> io::Result<()> {
    let result = details_for_files(
        &["src"],
        &[
            ("src/index.ts", "export {};"),
            (
                "src/one.tsx",
                r#"
                export function Curated() { return <p />; }
                export function Duplicate() { return <p />; }
            "#,
            ),
            (
                "src/two.tsx",
                "export function Duplicate() { return <aside />; }",
            ),
        ],
        serde_json::json!([
            {"id":"curated","symbol":"Curated"},
            {"id":"duplicate","symbol":"Duplicate"}
        ]),
    )?;

    let evidence: Vec<_> = result
        .document
        .components
        .iter()
        .map(|component| {
            component
                .details
                .as_ref()
                .unwrap()
                .source_evidence
                .as_ref()
                .map(|source| source.path.as_str())
        })
        .collect();
    assert_eq!(evidence, [Some("src/one.tsx"), None]);
    Ok(())
}

#[test]
fn discover_registry_details_attaches_evidence_for_a_renamed_default_class() -> io::Result<()> {
    let result = details_for_files(
        &["src"],
        &[
            (
                "src/index.ts",
                r#"export { default as Card } from "./card";"#,
            ),
            (
                "src/card.tsx",
                "/** Shows card content. */\nexport default class { render() { return <article />; } }",
            ),
        ],
        serde_json::json!([{"id": "card", "symbol": "Card"}]),
    )?;

    let details = result.document.components[0].details.as_ref().unwrap();
    assert_eq!(
        details.documentation.as_deref(),
        Some("Shows card content.")
    );
    let evidence = details.source_evidence.as_ref().unwrap();
    assert_eq!(
        (evidence.path.as_str(), evidence.start_line),
        ("src/card.tsx", Some(2))
    );
    Ok(())
}

fn details_for_files(
    roots: &[&str],
    files: &[(&str, &str)],
    components: serde_json::Value,
) -> io::Result<wax_lang_api::DiscoverRegistryResult> {
    let repo = tempfile::tempdir()?;
    write_sources(repo.path(), files)?;
    Ok(discover_registry_details(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: repo.path().to_string_lossy().into_owned(),
        roots: roots.iter().map(|root| (*root).to_owned()).collect(),
        registry: serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "components": components
        }))
        .unwrap(),
    })
    .expect("rich discover should succeed"))
}

fn discover_files(
    roots: &[&str],
    files: &[(&str, &str)],
) -> io::Result<wax_lang_react::DiscoverSymbolsResult> {
    let repo = tempfile::tempdir()?;
    write_sources(repo.path(), files)?;
    Ok(ReactLanguage::new()
        .discover(&DiscoverRequest {
            request_type: DiscoverRequestType::Discover,
            api_version: WIRE_API_VERSION,
            language_id: "react".try_into().unwrap(),
            repo_root: repo.path().to_string_lossy().into_owned(),
            roots: roots.iter().map(|root| (*root).to_owned()).collect(),
        })
        .expect("discover should succeed"))
}

fn write_sources(root: &std::path::Path, files: &[(&str, &str)]) -> io::Result<()> {
    for (path, source) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, source)?;
    }
    Ok(())
}

#[test]
fn discover_registry_symbols_bounds_shared_star_export_traversal() -> io::Result<()> {
    let repo = tempfile::tempdir()?;
    let src = repo.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("button.tsx"),
        "export function Button() { return <button />; }\n",
    )?;

    let mut previous_a = "./button".to_owned();
    let mut previous_b = "./button".to_owned();
    for layer in 0..20 {
        let a = format!("layer-{layer}-a");
        let b = format!("layer-{layer}-b");
        fs::write(
            src.join(format!("{a}.ts")),
            format!("export * from '{previous_a}';\nexport * from '{previous_b}';\n"),
        )?;
        fs::write(
            src.join(format!("{b}.ts")),
            format!("export * from '{previous_a}';\nexport * from '{previous_b}';\n"),
        )?;
        previous_a = format!("./{a}");
        previous_b = format!("./{b}");
    }
    fs::write(
        src.join("index.ts"),
        format!("export * from '{previous_a}';\nexport * from '{previous_b}';\n"),
    )?;

    let started = std::time::Instant::now();
    let result = ReactLanguage::new()
        .discover(&DiscoverRequest {
            request_type: DiscoverRequestType::Discover,
            api_version: WIRE_API_VERSION,
            language_id: "react".try_into().unwrap(),
            repo_root: repo.path().to_string_lossy().into_owned(),
            roots: vec!["src".to_owned()],
        })
        .expect("discovery should complete");

    assert_eq!(result.symbols(), vec!["Button"]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "shared star-export graph took {:?}",
        started.elapsed()
    );
    Ok(())
}

#[test]
fn discovers_literal_palettes_through_public_exports_in_package_order() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    for (path, source) in [
        (
            "a/package.json",
            r#"{"name":"@example/colors","exports":"./src/index.ts"}"#,
        ),
        (
            "a/src/index.ts",
            r#"
export * from './palette.js';
export { blue as renamed } from './palette.js';
export * as theme from './palette.js';
export * from './left';
export * from './right';
"#,
        ),
        (
            "a/src/palette.ts",
            r#"const privatePalette = { hidden: '#aaa' };
export const blue = ({
  blue9: '#0090ff',
  'alpha': '#0090ff80',
  wide: 'color(display-p3 0 .5 1)',
  wideAlpha: 'color(display-p3 0.1 0.5 1 / .25)',
  invalidHex: '#abcde',
  invalidChannel: 'color(display-p3 0 1.01 1)',
  invalidAlpha: 'color(display-p3 0 1 1 / NaN)',
  dynamic: privatePalette.hidden
} as const) satisfies Record<string, string>;
export let mutable = { color: '#aaa' };
export const spread = { ...blue, color: '#aaa' };
export const computed = { ['color']: '#aaa' };
export const duplicate = { color: '#aaa', 'color': '#bbb' };
export const method = { color: '#aaa', value() {} };
const color = '#aaa';
export const shorthand = { color, other: '#bbb' };
export const cast = <Record<string, string>>{ color: '#abcd' };
"#,
        ),
        ("a/src/loose.ts", "export const hidden = { color: '#abc' };"),
        (
            "a/src/left.ts",
            "export const ambiguous = { color: '#abc' };",
        ),
        (
            "a/src/right.ts",
            "export const ambiguous = { color: '#def' };",
        ),
        ("b/package.json", r#"{"name":"other-colors"}"#),
        ("b/src/index.ts", "export const blue = { blue9: '#123' };"),
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(file, source)?;
    }
    let mut request = palette_request(root, &["a/src", "b/src"]);
    let result = discover_registry_details(&request).unwrap();
    let mut expected = Vec::new();
    for prefix in ["blue", "renamed", "theme.blue"] {
        for (key, value, line) in [
            ("alpha", "#0090ff80", 4),
            ("blue9", "#0090ff", 3),
            ("wide", "color(display-p3 0 .5 1)", 5),
            ("wideAlpha", "color(display-p3 0.1 0.5 1 / .25)", 6),
        ] {
            expected.push((
                format!("token:@example/colors#{prefix}.{key}"),
                value,
                "a/src/palette.ts",
                line,
            ));
        }
    }
    for key in ["cast.color", "theme.cast.color"] {
        expected.push((
            format!("token:@example/colors#{key}"),
            "#abcd",
            "a/src/palette.ts",
            19,
        ));
    }
    expected.push((
        "token:other-colors#blue.blue9".to_owned(),
        "#123",
        "b/src/index.ts",
        1,
    ));
    expected.sort_unstable();
    let observed = result
        .document
        .tokens
        .iter()
        .map(|token| {
            let details = token.details.as_ref().unwrap();
            let evidence = details.source_evidence.as_ref().unwrap();
            assert_eq!(
                details.extra["discovered_literal_value"],
                token.value.as_deref().unwrap()
            );
            assert_eq!(evidence.start_line, evidence.end_line);
            (
                token.id.clone(),
                token.value.as_deref().unwrap(),
                evidence.path.as_str(),
                evidence.start_line.unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(observed, expected);
    assert!(result.document.components.is_empty());
    request.roots.reverse();
    assert_eq!(
        discover_registry_details(&request).unwrap().document,
        result.document
    );
    Ok(())
}

#[test]
fn duplicate_palette_observations_from_root_entries_are_deduplicated() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::create_dir(root.join("src"))?;
    fs::write(
        root.join("src/palette.ts"),
        "export const blue = { blue9: '#0090ff' };",
    )?;
    fs::write(
        root.join("src/barrel.ts"),
        "export { blue } from './palette';",
    )?;
    let request = palette_request(root, &["src"]);

    let result = discover_registry_details(&request).expect("duplicate exports are valid");

    assert_eq!(result.document.tokens.len(), 1);
    assert_eq!(result.document.tokens[0].id, "token:blue.blue9");
    Ok(())
}

fn palette_request(root: &std::path::Path, roots: &[&str]) -> DiscoverRegistryRequest {
    DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: root.display().to_string(),
        roots: roots.iter().map(|root| (*root).to_owned()).collect(),
        registry: serde_json::from_value(serde_json::json!({"schema_version":1})).unwrap(),
    }
}

#[test]
fn rediscovery_prefers_scoped_seeds_and_preserves_creator_values() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    for (dir, palette) in [
        (
            "a",
            "export const blue = { shared: '#def', edited: '#def', cleared: '#def', legacy: '#def', conflict: '#def' };",
        ),
        (
            "b",
            "export const blue = { shared: '#456', multi: '#456' };",
        ),
        ("c", "export const blue = { multi: '#789' };"),
    ] {
        fs::create_dir(root.join(dir))?;
        fs::write(
            root.join(dir).join("package.json"),
            format!(r#"{{"name":"{dir}"}}"#),
        )?;
        fs::write(root.join(dir).join("index.ts"), palette)?;
    }
    let mut request = palette_request(root, &["a", "b", "c"]);
    request.registry = serde_json::from_str(r##"{
      "schema_version":1,
      "tokens":[
        {"id":"scoped","key":"blue.shared","package":"a","category":"color","value":"#abc","aliases":["brand.blue"],
         "metadata":{"description":"Owner","sources":[{"kind":"declaration","path":"a/index.ts"}]},"custom":"entry",
         "details":{"status":"complete","documentation":"Keep me","custom":"detail","discovered_literal_value":"#abc"}},
        {"id":"unscoped","key":"blue.shared","category":"color","value":"#fff"},
        {"id":"edited","key":"blue.edited","package":"a","category":"color","value":"#fff",
         "details":{"status":"complete","discovered_literal_value":"#abc"}},
        {"id":"cleared","key":"blue.cleared","package":"a","category":"color",
         "details":{"status":"complete","discovered_literal_value":"#abc"}},
        {"id":"legacy","key":"blue.legacy","category":"unknown","value":"#123"},
        {"id":"legacy-conflict","key":"blue.conflict","category":"color","value":"#fff"},
        {"id":"scoped-conflict","key":"blue.conflict","package":"c","category":"color","value":"#fff"},
        {"id":"legacy-multi","key":"blue.multi","category":"color","value":"#fff"}
      ]
    }"##).unwrap();
    let observed = discover_registry_details(&request).unwrap().document;
    let token = &observed.tokens[0];
    assert_eq!(
        (token.id.as_str(), token.value.as_deref()),
        ("scoped", Some("#def"))
    );
    assert_eq!(token.package.as_deref(), Some("a"));
    assert_eq!(token.aliases, ["brand.blue"]);
    assert_eq!(token.metadata, request.registry.tokens[0].metadata);
    assert_eq!(token.extra["custom"], "entry");
    let details = token.details.as_ref().unwrap();
    assert_eq!(details.documentation.as_deref(), Some("Keep me"));
    assert_eq!(details.extra["custom"], "detail");
    assert_eq!(details.extra["discovered_literal_value"], "#def");
    assert_eq!(observed.tokens[1], request.registry.tokens[1]);
    for (index, value) in [(2, Some("#fff")), (3, None), (4, Some("#123"))] {
        let token = &observed.tokens[index];
        assert_eq!(token.value.as_deref(), value);
        assert!(
            !token
                .details
                .as_ref()
                .unwrap()
                .extra
                .contains_key("discovered_literal_value")
        );
    }
    assert_eq!(observed.tokens[4].package, None);
    assert_eq!(
        observed.tokens[4].category,
        wax_contract::TokenCategory::Unknown
    );
    assert_eq!(&observed.tokens[5..8], &request.registry.tokens[5..]);
    assert_eq!(
        observed.tokens[8..]
            .iter()
            .map(|token| token.id.as_str())
            .collect::<Vec<_>>(),
        [
            "token:a#blue.conflict",
            "token:b#blue.multi",
            "token:b#blue.shared",
            "token:c#blue.multi"
        ]
    );
    Ok(())
}

#[test]
fn palette_rediscovery_preserves_evidence_on_damage_and_reports_clean_absence() -> io::Result<()> {
    use wax_contract::RegistryExtractionStatus::{Complete, NotObserved, Stale};

    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"colors","exports":"./index.ts"}"#,
    )?;
    let index = root.join("index.ts");
    let barrel = root.join("barrel.ts");
    let palette = root.join("palette.ts");
    let unrelated = root.join("unrelated.ts");
    fs::write(&index, "export { blue } from './barrel';")?;
    fs::write(&barrel, "export * from './palette';")?;
    fs::write(&palette, "export const blue = { blue9: '#abc' };")?;
    fs::write(&unrelated, "export const loose = { red: '#abc' };")?;
    let mut request = palette_request(root, &["."]);
    request.registry = discover_registry_details(&request).unwrap().document;
    let previous = request.registry.tokens[0].details.as_ref().unwrap();
    for (path, valid_source) in [
        (&palette, "export const blue = { blue9: '#abc' };"),
        (&barrel, "export * from './palette';"),
        (&index, "export { blue } from './barrel';"),
    ] {
        fs::write(path, "export const broken = @;")?;
        let result = discover_registry_details(&request).unwrap();
        assert_eq!(result.document.tokens.len(), 1);
        let token = &result.document.tokens[0];
        let details = token.details.as_ref().unwrap();
        assert_eq!(details.status, Stale, "{}", path.display());
        assert_eq!(details.source_evidence, previous.source_evidence);
        assert_eq!(details.extra["discovered_literal_value"], "#abc");
        assert_eq!(token.value.as_deref(), Some("#abc"));
        assert!(!result.diagnostics.is_empty());
        fs::write(path, valid_source)?;
    }
    fs::write(&index, "export {};")?;
    fs::write(&unrelated, "export const broken = @;")?;
    let uncertain = discover_registry_details(&request).unwrap().document;
    assert_eq!(
        uncertain.tokens[0].details.as_ref().unwrap().status,
        Complete
    );
    assert_eq!(uncertain.tokens, request.registry.tokens);

    fs::write(&unrelated, "export {};")?;
    let absent = discover_registry_details(&request).unwrap().document;
    let mut expected = request.registry.tokens.clone();
    expected[0].details.as_mut().unwrap().status = NotObserved;
    assert_eq!(absent.tokens, expected);
    Ok(())
}
#[test]
fn discovers_portal_returns_through_a_barrel() -> io::Result<()> {
    let files = [
        ("src/index.ts", "export * from './portal';"),
        (
            "src/portal.tsx",
            r#"import { forwardRef as wrap, memo, cloneElement as clone } from 'react';
import * as DOM from 'react-dom';
import { createPortal as mount } from 'react-dom';
import { createPortal as foreign } from 'other';
import type { createPortal as typeOnly } from 'react-dom';
export const Portal = wrap(({ label }: { label: string }) =>
    ready ? DOM.createPortal(<div>{label}</div>, node) : null
);
export const Aliased = wrap(() => {
    const content = <span />;
    if (ready) return mount(content, node);
    return null;
});
export const PortalLocal = wrap(() => {
    const result = DOM.createPortal(<div />, node);
    return result;
});
export const CloneLocal = wrap(() => {
    const result = clone(child, {});
    return result;
});
export const Wrapped = memo(wrap(() => <input />));
export const Shadowed = wrap((DOM) => DOM.createPortal(<div />, node));
export const ShadowedAlias = wrap(() => {
    const mount = foreign;
    return mount(<div />, node);
});
export const Foreign = wrap(() => foreign(<div />, node));
export const TypeOnly = wrap(() => typeOnly(<div />, node));
export const NoElement = wrap(() => mount(value, node));
export const Nested = wrap(() => {
    work(() => mount(<div />, node));
    return null;
});
"#,
        ),
    ];
    assert_eq!(
        discover_files(&["src"], &files)?.symbols(),
        ["Aliased", "CloneLocal", "Portal", "PortalLocal", "Wrapped"]
    );
    Ok(())
}

#[test]
fn discovers_clone_element_only_in_verified_render_callbacks() -> io::Result<()> {
    let files = [(
        "src/clone.tsx",
        r#"import React, { cloneElement as clone, memo } from 'react';
import { cloneElement as foreign } from 'other';
export const Cloned = memo(() => clone(child, {}));
export const Namespace = React.forwardRef(function (props, ref) {
    if (!props.ready) return null;
    return React.cloneElement(props.child, { ref });
});
export const Bare = () => clone(child, {});
export const Shadowed = React.forwardRef((React) => React.cloneElement(child));
export const Foreign = React.forwardRef(() => foreign(child));
export const Nested = React.forwardRef(() => {
    work(() => { return clone(child); });
    return null;
});
"#,
    )];
    assert_eq!(
        discover_files(&["src"], &files)?.symbols(),
        ["Cloned", "Namespace"]
    );
    Ok(())
}

#[test]
fn discovers_factory_exports_with_one_statically_known_return() -> io::Result<()> {
    let files = [
        ("src/index.ts", "export * from './slot';"),
        (
            "src/slot.tsx",
            r#"import React, { forwardRef } from 'react';
import { makeForeign } from 'other';
export const Slot = createSlot('Slot');
export function createSlot(ownerName: string) {
    const Inner = React.forwardRef((props) => React.cloneElement(props.child));
    Inner.displayName = ownerName;
    return Inner;
}
const Inner = () => <div />;
const makeArrow = () => { return Inner; };
export const FromOuter = makeArrow();
function makeDirect() { return forwardRef(() => <span />); }
export const Direct = makeDirect();

export const Foreign = makeForeign();
export const Cycle = cycle();
function cycle() { return other(); }
function other() { return cycle(); }
const alias = createSlot;
export const Indirect = alias();
function createDynamic(render) { return render; }
export const Dynamic = createDynamic(Inner);
function createConflict() { if (ready) return null; return Inner; }
export const Conflict = createConflict();
function createMutable() {
    let Target = Inner;
    Target = foreign;
    return Target;
}
export const Mutable = createMutable();
function reassigned() { return Inner; }
reassigned = makeForeign;
export const Reassigned = reassigned();
function createReassignedTarget() {
    function Target() { return <div />; }
    ({ Target } = foreign);
    return Target;
}
export const ReassignedTarget = createReassignedTarget();
function createShadowed(React) {
    const Inner = React.forwardRef(() => <div />);
    return Inner;
}
export const Shadowed = createShadowed(React);
function createShadowedTarget(Inner) { return Inner; }
export const ShadowedTarget = createShadowedTarget(Inner);
function createTooLate() {
    return Target;
    const Target = () => <div />;
}
export const TooLate = createTooLate();
"#,
        ),
    ];
    assert_eq!(
        discover_files(&["src"], &files)?.symbols(),
        ["Direct", "FromOuter", "Slot"]
    );
    Ok(())
}

#[test]
fn rich_discovery_preserves_callback_facts_and_factory_signature_gaps() -> io::Result<()> {
    let result = details_for_files(
        &["src"],
        &[
            (
                "src/index.ts",
                "export { Portal, Slot, UnknownSlot } from './components';",
            ),
            (
                "src/components.tsx",
                r#"import { forwardRef, cloneElement } from 'react';
import { createPortal } from 'react-dom';
const Portal = forwardRef(({ label }: { label: string }) =>
    createPortal(<div>{label}</div>, node)
);
const Slot = createSlot('Slot');
const UnknownSlot = createUnknownSlot();
function createSlot(ownerName: string) {
    const Inner = forwardRef(({ child }: { child: React.ReactElement }) => cloneElement(child));
    return Inner;
}
function createUnknownSlot() {
    return forwardRef((props) => cloneElement(props.child));
}
export { Portal, Slot, UnknownSlot };
"#,
            ),
        ],
        serde_json::json!([
            {"id": "portal", "symbol": "Portal"},
            {"id": "slot", "symbol": "Slot"},
            {"id": "unknown-slot", "symbol": "UnknownSlot"}
        ]),
    )?;

    for (entry, (line, status, expected_parameters)) in result.document.components.iter().zip([
        (
            3,
            wax_contract::RegistryExtractionStatus::Complete,
            &["label"][..],
        ),
        (
            6,
            wax_contract::RegistryExtractionStatus::Partial,
            &["child"][..],
        ),
        (7, wax_contract::RegistryExtractionStatus::Partial, &[][..]),
    ]) {
        let details = entry
            .details
            .as_ref()
            .unwrap_or_else(|| panic!("missing {}", entry.symbol));
        let evidence = details.source_evidence.as_ref().unwrap();
        assert_eq!(
            (evidence.path.as_str(), evidence.start_line),
            ("src/components.tsx", Some(line))
        );
        assert_eq!(details.status, status);
        let signature = details.signature.as_ref().unwrap();
        let names: Vec<_> = signature
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, expected_parameters);
        if entry.symbol.ends_with("Slot") {
            assert!(signature.type_text.is_none());
            assert!(
                details
                    .field_gaps
                    .iter()
                    .any(|gap| gap.field.starts_with("signature."))
            );
        }
    }
    Ok(())
}

#[test]
fn excludes_factories_that_yield_a_promise_or_iterator() -> io::Result<()> {
    let files = [(
        "src/factory.tsx",
        r#"const Component = () => <div />;
async function makeAsync() { return Component; }
function* makeIterator() { return Component; }
const makeAsyncArrow = async () => { return Component; };
export const Async = makeAsync();
export const Iterator = makeIterator();
export const AsyncArrow = makeAsyncArrow();
"#,
    )];
    assert!(discover_files(&["src"], &files)?.symbols().is_empty());
    Ok(())
}

#[test]
fn rich_factory_discovery_respects_callback_props_type_scope() -> io::Result<()> {
    let result = details_for_files(
        &["src"],
        &[(
            "src/slot.tsx",
            r#"import { cloneElement, forwardRef } from 'react';
type Props = { wrong: string };
function makeSlot<Props>() {
    return forwardRef((props: Props) => cloneElement(child));
}
export const Slot = makeSlot();
"#,
        )],
        serde_json::json!([{"id": "slot", "symbol": "Slot"}]),
    )?;
    let details = result.document.components[0].details.as_ref().unwrap();
    assert!(details.signature.as_ref().unwrap().parameters.is_empty());
    assert!(
        details
            .field_gaps
            .iter()
            .any(|gap| gap.field == "signature.parameters")
    );
    Ok(())
}

fn theme_request(root: &std::path::Path, roots: &[&str]) -> DiscoverRegistryRequest {
    palette_request(root, roots)
}

#[test]
fn discovers_top_level_tailwind_theme_tokens_with_evidence() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    fs::write(
        fixture.path().join("globals.css"),
        r#":root {
  --main: hsl(217, 100%, 66%);
  --shadow: var(--box-shadow-x) var(--box-shadow-y) 0px 0px var(--border);
}
@theme inline {
  --color-main: var(--main);
  --radius-base: 5px;
  --font-weight-base: 500;
  --shadow-shadow: var(--shadow);
  --text-shadow-nav: 1px 2px black;
  @keyframes spin { to { --color-nested: red; } }
}
"#,
    )?;
    let result = discover_registry_details(&theme_request(fixture.path(), &["."])).unwrap();

    use wax_contract::TokenCategory::{Color, Elevation, Radius, Typography};
    let expected = [
        ("color-main", Color, "hsl(217, 100%, 66%)", 6),
        ("radius-base", Radius, "5px", 7),
        ("font-weight-base", Typography, "500", 8),
        (
            "shadow-shadow",
            Elevation,
            "var(--box-shadow-x) var(--box-shadow-y) 0px 0px var(--border)",
            9,
        ),
        ("text-shadow-nav", Elevation, "1px 2px black", 10),
    ];
    assert_eq!(result.document.tokens.len(), expected.len());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    for (name, category, value, line) in expected {
        let token = result
            .document
            .tokens
            .iter()
            .find(|token| token.id == format!("css.{name}"))
            .unwrap();
        assert_eq!(
            (
                &*token.key,
                &token.package,
                token.category,
                token.value.as_deref()
            ),
            (
                format!("var(--{name})").as_str(),
                &None,
                category,
                Some(value)
            )
        );
        let details = token.details.as_ref().unwrap();
        assert_eq!(
            details.status,
            wax_contract::RegistryExtractionStatus::Complete
        );
        let evidence = details.source_evidence.as_ref().unwrap();
        assert_eq!(
            (evidence.path.as_str(), evidence.start_line),
            ("globals.css", Some(line))
        );
    }
    Ok(())
}

#[test]
fn resolves_whole_value_theme_references_with_bounded_root_first_lookup() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    fs::write(
        fixture.path().join("a.css"),
        r#":root {
  --first: var(--color-literal);
  --color-literal: purple;
  --loop-a: var(--loop-b);
  --loop-b: var(--loop-a);
  --hop-1: var(--hop-2); --hop-2: var(--hop-3); --hop-3: var(--hop-4);
  --hop-4: var(--hop-5); --hop-5: var(--hop-6); --hop-6: var(--hop-7);
  --hop-7: var(--hop-8); --hop-8: var(--hop-9); --hop-9: blue;
}
@theme inline {
  --color-main: var(--first);
  --color-literal: red;
  --color-cycle: var(--loop-a);
  --color-fallback: var(--missing, rgb(1, 2, 3)) !important;
  --color-missing: var(--missing);
  --color-theme-ref: var(--color-fallback);
  --shadow-multiple: var(--color-literal) var(--missing);
  --color-bound: var(--hop-1);
  --color-commented: /* keep */ blue /* source */ !important;
}
"#,
    )?;
    fs::write(
        fixture.path().join("z.css"),
        ":root { --color-literal: orange; --color-literal: green; }",
    )?;
    let result = discover_registry_details(&theme_request(fixture.path(), &["."])).unwrap();
    let values: std::collections::BTreeMap<_, _> = result
        .document
        .tokens
        .iter()
        .map(|token| (token.id.as_str(), token.value.as_deref().unwrap()))
        .collect();
    assert_eq!(
        values,
        [
            ("css.color-main", "green"),
            ("css.color-literal", "red"),
            ("css.color-cycle", "var(--loop-a)"),
            ("css.color-fallback", "rgb(1, 2, 3)"),
            ("css.color-missing", "var(--missing)"),
            ("css.color-theme-ref", "rgb(1, 2, 3)"),
            ("css.shadow-multiple", "var(--color-literal) var(--missing)"),
            ("css.color-bound", "var(--hop-9)"),
            ("css.color-commented", "/* keep */ blue /* source */"),
        ]
        .into()
    );
    Ok(())
}

#[test]
fn discovers_only_supported_top_level_namespaces_under_css_roots() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::create_dir(root.join("styles"))?;
    fs::write(root.join("outside.css"), "@theme { --color-outside: red; }")?;
    for dir in ["node_modules", "generated", "__generated__"] {
        fs::create_dir(root.join("styles").join(dir))?;
        fs::write(
            root.join("styles").join(dir).join("theme.css"),
            "@theme { --color-ignored: red; }",
        )?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("outside.css"), root.join("styles/link.css"))?;

    fs::write(
        root.join("styles/theme.css"),
        r#"div { --main: red; }
@media screen { @theme inline { --color-media: red; } }
@theme {
    --color-*: initial;
    --color-: red;
    --font-weight-: 500;
    --text-shadow-: 1px 2px black;
    --unknown-name: red;
    --text-xl--line-height: 2;
    --color-main: var(--main);
}
@theme /* mode */ static { --spacing: 4px; }
@theme default { --container-s: 10rem; }
@theme reference { --animate-spin: spin 1s linear; }
@theme invalid { --color-invalid: red; }
"#,
    )?;
    let result = discover_registry_details(&theme_request(
        root,
        &["styles", "styles", "styles/generated"],
    ))
    .unwrap();
    let values: std::collections::BTreeMap<_, _> = result
        .document
        .tokens
        .iter()
        .map(|token| (token.id.as_str(), token.value.as_deref().unwrap()))
        .collect();
    assert_eq!(
        values,
        [
            ("css.color-main", "var(--main)"),
            ("css.spacing", "4px"),
            ("css.container-s", "10rem"),
            ("css.animate-spin", "spin 1s linear"),
        ]
        .into()
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    Ok(())
}

#[test]
fn keeps_first_tailwind_declaration_and_warns_on_conflicting_values() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    for (file, css) in [
        ("b.css", "@theme { --color-main: blue; }"),
        ("a.css", "@theme { --color-main: red; --color-main: red; }"),
    ] {
        fs::write(fixture.path().join(file), css)?;
    }
    let result = discover_registry_details(&theme_request(fixture.path(), &["."])).unwrap();

    assert_eq!(result.document.tokens.len(), 1);
    assert_eq!(result.document.tokens[0].value.as_deref(), Some("red"));
    let [warning] = result.diagnostics.as_slice() else {
        panic!(
            "expected one duplicate warning, got {:?}",
            result.diagnostics
        );
    };
    assert_eq!(warning.code, "tailwind_theme_duplicate");
    assert_eq!(warning.severity, wax_contract::DiagnosticSeverity::Warning);
    assert_eq!(warning.location.as_ref().unwrap().file, "b.css");
    Ok(())
}

#[test]
fn rediscovery_preserves_authored_theme_fields_and_logical_css_variants() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    fs::write(
        fixture.path().join("theme.css"),
        "@theme inline { --color-main: blue; --color-logical: green; --color-authored: red; }",
    )?;
    let mut request = theme_request(fixture.path(), &["."]);
    request.registry = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "tokens": [
            {
                "id": "brand.primary", "key": "var(--color-main)", "category": "color",
                "aliases": ["brand"], "metadata": {"description": "Primary brand color"},
                "value": "red",
                "details": {"status": "complete", "discovered_literal_value": "red"}
            },
            {
                "id": "brand.logical", "key": "var(--color-logical)", "category": "color",
                "css_variants": ["brand.primary"],
                "details": {"status": "complete", "discovered_literal_value": "red"}
            },
            {
                "id": "brand.custom", "key": "var(--color-authored)", "category": "unknown",
                "value": "black"
            }
        ]
    }))
    .unwrap();
    let result = discover_registry_details(&request).unwrap();
    result.document.validate().unwrap();

    let [main, logical, authored] = result.document.tokens.as_slice() else {
        panic!("expected exactly the three seed tokens");
    };
    assert_eq!(main.id, "brand.primary");
    assert_eq!(main.aliases, ["brand"]);
    assert_eq!(main.metadata, request.registry.tokens[0].metadata);
    assert_eq!(main.value.as_deref(), Some("blue"));
    let details = main.details.as_ref().unwrap();
    assert_eq!(details.extra["discovered_literal_value"], "blue");
    assert_eq!(details.source_evidence.as_ref().unwrap().path, "theme.css");

    assert_eq!(logical.id, "brand.logical");
    assert_eq!(logical.css_variants, ["brand.primary"]);
    assert_eq!(logical.value, None);
    let details = logical.details.as_ref().unwrap();
    assert!(!details.extra.contains_key("discovered_literal_value"));
    assert_eq!(details.source_evidence.as_ref().unwrap().path, "theme.css");

    assert_eq!(authored.value.as_deref(), Some("black"));
    assert_eq!(authored.category, wax_contract::TokenCategory::Unknown);
    Ok(())
}

#[test]
fn rediscovery_tracks_removed_and_damaged_tailwind_declarations() -> io::Result<()> {
    use wax_contract::RegistryExtractionStatus::{Complete, NotObserved, Stale};

    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::write(root.join("removed.css"), "@theme { --color-removed: red; }")?;
    fs::write(
        root.join("damaged.css"),
        "@theme { --color-damaged: blue; }",
    )?;
    let mut request = theme_request(root, &["."]);
    request.registry = discover_registry_details(&request).unwrap().document;
    fs::remove_file(root.join("removed.css"))?;
    fs::write(root.join("damaged.css"), "@theme { --color-damaged: ; }")?;
    fs::write(root.join("clean.css"), "@theme { --color-clean: green; }")?;
    let result = discover_registry_details(&request).unwrap();

    let tokens: std::collections::BTreeMap<_, _> = result
        .document
        .tokens
        .iter()
        .map(|token| {
            (
                token.id.as_str(),
                (
                    token.details.as_ref().unwrap().status,
                    token.value.as_deref(),
                ),
            )
        })
        .collect();
    assert_eq!(
        tokens,
        [
            ("css.color-clean", (Complete, Some("green"))),
            ("css.color-damaged", (Stale, Some("blue"))),
            ("css.color-removed", (NotObserved, Some("red"))),
        ]
        .into()
    );
    let [error] = result.diagnostics.as_slice() else {
        panic!(
            "expected one CSS parse failure, got {:?}",
            result.diagnostics
        );
    };
    assert_eq!(error.code, "parse_failed");
    let location = error.location.as_ref().unwrap();
    assert_eq!((location.file.as_str(), location.line), ("damaged.css", 1));

    request.registry = result.document;
    fs::remove_file(root.join("clean.css"))?;
    fs::remove_file(root.join("damaged.css"))?;
    let result = discover_registry_details(&request).unwrap();
    assert!(
        result
            .document
            .tokens
            .iter()
            .all(|token| token.details.as_ref().unwrap().status == NotObserved)
    );
    assert!(result.diagnostics.is_empty());
    Ok(())
}

#[test]
fn rediscovery_tracks_tailwind_tokens_after_generated_value_marker_is_cleared() -> io::Result<()> {
    use wax_contract::RegistryExtractionStatus::{NotObserved, Stale};

    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    fs::write(root.join("removed.css"), "@theme { --color-removed: red; }")?;
    fs::write(
        root.join("damaged.css"),
        "@theme { --color-damaged: blue; }",
    )?;
    let mut request = theme_request(root, &["."]);
    request.registry = discover_registry_details(&request).unwrap().document;
    for token in &mut request.registry.tokens {
        token
            .details
            .as_mut()
            .unwrap()
            .extra
            .remove("discovered_literal_value");
    }

    fs::remove_file(root.join("removed.css"))?;
    fs::write(root.join("damaged.css"), "@theme { --color-damaged: ; }")?;
    let result = discover_registry_details(&request).unwrap();
    let status_by_id: std::collections::BTreeMap<_, _> = result
        .document
        .tokens
        .iter()
        .map(|token| (token.id.as_str(), token.details.as_ref().unwrap().status))
        .collect();
    assert_eq!(
        status_by_id,
        [
            ("css.color-damaged", Stale),
            ("css.color-removed", NotObserved)
        ]
        .into()
    );
    Ok(())
}

#[test]
fn css_without_tailwind_theme_leaves_discovery_output_unchanged() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let request = theme_request(fixture.path(), &["."]);
    let before = discover_registry_details(&request).unwrap();
    fs::write(
        fixture.path().join("ordinary.css"),
        ":root { --color-main: blue; } .button { color: ; }",
    )?;
    let after = discover_registry_details(&request).unwrap();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap()
    );
    Ok(())
}

#[test]
fn discover_intersection_preserves_inline_props_and_destructured_defaults() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import * as React from "react";
export function Button({ size = "default", ...props }:
    React.ComponentProps<"div"> & ({ size?: "default" | "sm" })) {
    return <div {...props} />;
}"#,
    )?;
    let parameters = details.signature.unwrap().parameters;
    assert_eq!(parameters.len(), 1);
    let size = &parameters[0];
    assert_eq!(size.name, "size");
    assert_eq!(size.type_kind, Some(RegistryParameterTypeKind::Enum));
    assert_eq!(size.allowed_values, ["default", "sm"]);
    assert_eq!(size.default.as_deref(), Some(r#""default""#));
    assert_eq!(details.field_gaps.len(), 1);
    assert_eq!(details.field_gaps[0].field, "signature.parameters");
    assert_eq!(
        details.field_gaps[0].reason,
        r#"props intersection member `React.ComponentProps<"div">` requires type resolution"#
    );
    Ok(())
}

#[test]
fn discover_button_intersection_extracts_cva_variants() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
const buttonVariants = cva("button", {
    variants: {
        /** Visual emphasis. */
        variant: { default: "", noShadow: "", neutral: "", reverse: "" },
        size: { default: "", xs: "", sm: "", lg: "", icon: "",
                "icon-xs": "", "icon-sm": "", "icon-lg": "" },
    },
    defaultVariants: { variant: "default", size: "default" },
});
export function Button({ variant, size, ...props }:
    React.ComponentProps<typeof ButtonPrimitive> & VariantProps<typeof buttonVariants>) {
    return <ButtonPrimitive {...props} />;
}"#,
    )?;
    assert_eq!(
        serde_json::to_value(details.signature.unwrap().parameters).unwrap(),
        serde_json::json!([
            {
                "name": "variant", "type_kind": "enum",
                "allowed_values": ["default", "noShadow", "neutral", "reverse"],
                "description": "Visual emphasis.", "default": "\"default\"", "required": false
            },
            {
                "name": "size", "type_kind": "enum",
                "allowed_values": ["default", "xs", "sm", "lg", "icon", "icon-xs", "icon-sm", "icon-lg"],
                "default": "\"default\"", "required": false
            }
        ])
    );
    assert_eq!(
        details.status,
        wax_contract::RegistryExtractionStatus::Partial
    );
    assert_eq!(details.field_gaps.len(), 1);
    assert_eq!(
        details.field_gaps[0].reason,
        "props intersection member `React.ComponentProps<typeof ButtonPrimitive>` requires type resolution"
    );
    Ok(())
}

#[test]
fn discover_intersection_alias_resolves_local_members_and_keeps_gap_evidence() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva, type VariantProps } from "class-variance-authority";
const v = cva("", { variants: { size: { sm: "", lg: "" } }, defaultVariants: { size: "lg" } });
type ButtonProps = A & (VariantProps<typeof v> & MoreProps) & { loading?: boolean };
type MoreProps = React.ComponentProps<"button"> & { label: string };
interface A extends NativeProps {
    size: number;
    /** An inherited interface's own prop. */
    disabled?: boolean;
}
export function Button({ size = "sm" }: ButtonProps) { return <button />; }"#,
    )?;
    assert_eq!(
        serde_json::to_value(details.signature.unwrap().parameters).unwrap(),
        serde_json::json!([
            {"name": "size", "type_kind": "enum", "allowed_values": ["sm", "lg"],
             "default": "\"sm\"", "required": false},
            {"name": "disabled", "type_text": "boolean", "required": false,
             "description": "An inherited interface's own prop."},
            {"name": "label", "type_text": "string", "type_kind": "string", "required": true},
            {"name": "loading", "type_text": "boolean", "type_kind": "boolean", "required": false}
        ])
    );
    assert_eq!(
        details
            .field_gaps
            .into_iter()
            .map(|gap| gap.reason)
            .collect::<Vec<_>>(),
        [
            "props type `A` inherits fields that require type resolution",
            r#"props intersection member `React.ComponentProps<"button">` requires type resolution"#
        ]
    );
    Ok(())
}

#[test]
fn discover_badge_intersection_extracts_boolean_variant_from_renamed_cva() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva as variants } from "class-variance-authority";
import type { VariantProps as StyleProps } from "class-variance-authority";
export const badgeVariants = variants("", {
    variants: { highlighted: { true: "", false: "" } },
    defaultVariants: { highlighted: false },
});
function Badge(props: useRender.ComponentProps<"span"> & StyleProps<typeof badgeVariants>) {
    return <span {...props} />;
}
export { Badge as Button };"#,
    )?;
    let parameters = details.signature.unwrap().parameters;
    assert_eq!(
        serde_json::to_value(parameters).unwrap(),
        serde_json::json!([
            {"name": "highlighted", "type_kind": "boolean", "default": "false", "required": false}
        ])
    );
    assert_eq!(details.field_gaps.len(), 1);
    assert_eq!(
        details.field_gaps[0].reason,
        r#"props intersection member `useRender.ComponentProps<"span">` requires type resolution"#
    );
    Ok(())
}

#[test]
fn discover_intersection_extracts_tv_values_in_first_seen_order() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { tv as define, type VariantProps } from "tailwind-variants";
const v = define({
    variants: { "density": { 2: "", compact: "", "2": "", 1: "" } },
    defaultVariants: { density: 2 },
});
export function Button(props: (VariantProps<typeof v> & {})) { return <button />; }"#,
    )?;
    assert_eq!(
        serde_json::to_value(details.signature.unwrap().parameters).unwrap(),
        serde_json::json!([
            {"name": "density", "type_kind": "enum", "allowed_values": ["2", "compact", "1"],
             "default": "2", "required": false}
        ])
    );
    assert_eq!(
        details.status,
        wax_contract::RegistryExtractionStatus::Complete
    );
    assert!(details.field_gaps.is_empty());
    Ok(())
}

#[test]
fn discover_intersection_does_not_claim_variant_facts_before_trailing_spreads() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva, type VariantProps } from "class-variance-authority";
const config = cva("", {
    variants: { size: { sm: "" } },
    defaultVariants: { size: "sm" },
    ...overrides,
});
export function Button(props: {} & VariantProps<typeof config>) { return <button />; }"#,
    )?;
    assert!(details.signature.unwrap().parameters.is_empty());
    Ok(())
}

#[test]
fn discover_intersection_does_not_claim_defaults_before_trailing_spreads() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva, type VariantProps } from "class-variance-authority";
const config = cva("", {
    variants: { size: { sm: "", lg: "" } },
    defaultVariants: { size: "sm", ...defaults },
});
export function Button(props: {} & VariantProps<typeof config>) { return <button />; }"#,
    )?;
    let parameters = details.signature.unwrap().parameters;
    assert_eq!(parameters[0].allowed_values, ["sm", "lg"]);
    assert_eq!(parameters[0].default, None);
    Ok(())
}

#[test]
fn discover_intersection_reports_nonliteral_variant_entries() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva, type VariantProps } from "class-variance-authority";
const v = cva("", {
    variants: {
        ...extra,
        [dynamic]: { one: "" },
        external: values,
        empty: {},
        indirect: { known: "", ...more },
        computed: { [dynamic]: "" },
        known: { one: "", two: "" }
    },
    defaultVariants: { known: theme.defaultVariant },
});
export function Button(props: {} & VariantProps<typeof v>) { return <button />; }"#,
    )?;
    assert_eq!(
        serde_json::to_value(details.signature.unwrap().parameters).unwrap(),
        serde_json::json!([
            {"name": "known", "type_kind": "enum", "allowed_values": ["one", "two"], "required": false}
        ])
    );
    assert_eq!(
        details.status,
        wax_contract::RegistryExtractionStatus::Partial
    );
    assert_eq!(
        details
            .field_gaps
            .into_iter()
            .map(|gap| gap.reason)
            .collect::<Vec<_>>(),
        [
            "variant `...extra` is not a literal object",
            "variant `[dynamic]` is not a literal object",
            "variant `external` is not a literal object",
            "variant `empty` is not a literal object",
            "variant `indirect` is not a literal object",
            "variant `computed` is not a literal object"
        ]
    );
    Ok(())
}

#[test]
fn discover_intersection_keeps_unproven_variant_bindings_opaque() -> io::Result<()> {
    let details = discover_documented_button(
        r#"import { cva, type VariantProps } from "class-variance-authority";
import type { VariantProps as OtherProps } from "other";
import { cva as otherCva } from "other";
import { unrelated as notCva } from "class-variance-authority";
const proven = cva("", { variants: { size: { sm: "" } } });
const wrongPackage = otherCva("", { variants: { size: { sm: "" } } });
const wrongExport = notCva("", { variants: { size: { sm: "" } } });
const indirect = cva("", config);
const spreadBase = cva(...base, { variants: { size: { sm: "" } } });
function localCva() {}
const local = localCva("", { variants: { size: { sm: "" } } });
export function Button(props:
    React.ComponentProps<"button"> & OtherProps<typeof proven> &
    VariantProps<typeof wrongPackage> & VariantProps<typeof wrongExport> &
    VariantProps<typeof indirect> & VariantProps<typeof spreadBase> &
    VariantProps<typeof local>
) { return <button />; }"#,
    )?;
    assert!(details.signature.unwrap().parameters.is_empty());
    assert_eq!(
        details.status,
        wax_contract::RegistryExtractionStatus::Partial
    );
    assert_eq!(
        details
            .field_gaps
            .into_iter()
            .map(|gap| gap.reason)
            .collect::<Vec<_>>(),
        [
            r#"props intersection member `React.ComponentProps<"button">` requires type resolution"#,
            "props intersection member `OtherProps<typeof proven>` requires type resolution",
            "props intersection member `VariantProps<typeof wrongPackage>` requires type resolution",
            "props intersection member `VariantProps<typeof wrongExport>` requires type resolution",
            "props intersection member `VariantProps<typeof indirect>` requires type resolution",
            "props intersection member `VariantProps<typeof spreadBase>` requires type resolution",
            "props intersection member `VariantProps<typeof local>` requires type resolution",
        ]
    );
    Ok(())
}
