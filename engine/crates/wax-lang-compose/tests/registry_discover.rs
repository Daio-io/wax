use std::path::PathBuf;
use std::{fs, io};

use wax_contract::DiagnosticSeverity;
use wax_lang_compose::discover::discover_registry_symbols;

fn fixture_parse_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}

fn fixture_root() -> PathBuf {
    fixture_parse_root().join("design-system/src/main/kotlin")
}

#[test]
fn discovers_public_top_level_composables() {
    let result = discover_registry_symbols(&fixture_parse_root(), &[fixture_root()])
        .expect("discover symbols");

    assert!(!result.symbols().iter().any(|symbol| symbol == "NestedCard"));
    assert_eq!(
        result.symbols(),
        vec!["PrimaryButton", "QualifiedButton", "SecondaryButton"]
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
    assert_eq!(package_for("PrimaryButton"), None);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "discover_package_conflict");
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
