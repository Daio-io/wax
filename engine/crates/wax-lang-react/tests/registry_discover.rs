use std::path::PathBuf;
use std::{fs, io};

use wax_lang_api::{DiscoverRequest, DiscoverRequestType};
use wax_lang_react::{ReactLanguage, discover_registry_symbols};

#[test]
fn discover_registry_symbols_emits_exported_public_components() {
    let root = fixture_root().join("design-system/src");

    let result = discover_registry_symbols(&[root]).expect("discover should succeed");

    assert_eq!(
        result.symbols,
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
    assert!(result.symbols.contains(&"Button".to_owned()));
    assert!(!result.symbols.contains(&"PrivateBadge".to_owned()));
    assert!(!result.symbols.contains(&"lowerBadge".to_owned()));
    assert!(!result.symbols.contains(&"notComponent".to_owned()));
    assert!(!result.symbols.contains(&"FactoryMemo".to_owned()));
    assert!(!result.symbols.contains(&"FactoryRef".to_owned()));
}

#[test]
fn missing_root_fails() {
    let missing = fixture_root().join("missing");

    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_skipped_with_diagnostics() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let broken_file = tempdir.path().join("Broken.tsx");
    fs::write(&broken_file, "export const Broken = (")?;

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert!(result.symbols.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert!(
        result.diagnostics[0]
            .location
            .as_ref()
            .is_some_and(|location| location.file.contains("Broken.tsx"))
            || result.diagnostics[0].message.contains("Broken.tsx")
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

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols, vec!["Button".to_owned()]);
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

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols, vec!["ChainedInput".to_owned()]);
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

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols, vec!["DefaultChainedInput".to_owned()]);
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

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert_eq!(result.symbols, vec!["DefaultInput".to_owned()]);
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

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should succeed");

    assert!(result.symbols.is_empty());
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
fn discover_merges_symbols_from_multiple_roots() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root_a = tempdir.path().join("pkg-a");
    let root_b = tempdir.path().join("pkg-b");
    fs::create_dir_all(&root_a)?;
    fs::create_dir_all(&root_b)?;
    fs::write(
        root_a.join("Alpha.tsx"),
        "export function Alpha() { return <a />; }",
    )?;
    fs::write(
        root_b.join("Beta.tsx"),
        "export function Beta() { return <b />; }",
    )?;

    let result = discover_registry_symbols(&[root_a, root_b]).expect("discover should succeed");

    assert_eq!(result.symbols, vec!["Alpha".to_owned(), "Beta".to_owned()]);
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

    let result = discover_registry_symbols(&[root]).expect("discover should succeed");

    assert_eq!(result.symbols, vec!["Button".to_owned()]);
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

    let result = discover_registry_symbols(&[root]).expect("discover should succeed");

    assert!(result.symbols.is_empty());
    Ok(())
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}
