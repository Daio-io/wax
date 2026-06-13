use std::path::PathBuf;
use std::{fs, io};

use wax_contract::DiagnosticSeverity;
use wax_lang_compose::discover::discover_registry_symbols;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/src/main/kotlin")
}

#[test]
fn discovers_public_top_level_composables() {
    let result = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

    assert!(!result.symbols.iter().any(|symbol| symbol == "NestedCard"));
    assert_eq!(
        result.symbols,
        vec!["PrimaryButton", "QualifiedButton", "SecondaryButton"]
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn missing_root_fails() {
    let missing = fixture_root().join("missing");

    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_do_not_block_symbols_from_other_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let good_file = tempdir.path().join("Good.kt");
    let broken_file = tempdir.path().join("Broken.kt");
    fs::write(&good_file, "@Composable\nfun GoodButton() {}\n")?;
    fs::write(&broken_file, "@Composable\nfun Broken(")?;

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols, vec!["GoodButton"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}

#[test]
fn parse_failures_are_skipped_with_diagnostics() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let broken_file = tempdir.path().join("Broken.kt");
    fs::write(&broken_file, "@Composable\nfun Broken(")?;

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert!(result.symbols.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].severity, DiagnosticSeverity::Warning);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert!(result.diagnostics[0].message.contains("Broken.kt"));
    Ok(())
}
