use std::path::PathBuf;
use std::{fs, io};

use wax_lang_api::{DiscoverRequest, DiscoverRequestType};
use wax_lang_react::{ReactLanguage, discover_registry_symbols};

#[test]
fn discover_registry_symbols_emits_exported_public_components() {
    let root = fixture_root().join("design-system/src");

    let symbols = discover_registry_symbols(&[root]).expect("discover should succeed");

    assert_eq!(
        symbols,
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
fn parse_failures_are_reported() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let broken_file = tempdir.path().join("Broken.tsx");
    fs::write(&broken_file, "export const Broken = (")?;

    let err =
        discover_registry_symbols(&[tempdir.path().to_path_buf()]).expect_err("parse should fail");

    assert!(err.to_string().contains("failed to parse"));
    assert!(err.to_string().contains("Broken.tsx"));
    Ok(())
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}
