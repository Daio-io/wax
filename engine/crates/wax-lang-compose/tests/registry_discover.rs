use std::path::PathBuf;
use std::{fs, io};

use wax_lang_compose::discover::discover_registry_symbols;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/src/main/kotlin")
}

#[test]
fn discovers_public_top_level_composables() {
    let symbols = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

    assert!(!symbols.iter().any(|symbol| symbol == "NestedCard"));
    assert_eq!(
        symbols,
        vec!["PrimaryButton", "QualifiedButton", "SecondaryButton"]
    );
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
    let broken_file = tempdir.path().join("Broken.kt");
    fs::write(&broken_file, "@Composable\nfun Broken(")?;

    let err =
        discover_registry_symbols(&[tempdir.path().to_path_buf()]).expect_err("parse should fail");

    assert!(err.to_string().contains("failed to parse"));
    assert!(err.to_string().contains("Broken.kt"));
    Ok(())
}
