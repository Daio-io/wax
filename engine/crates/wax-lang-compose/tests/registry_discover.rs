use std::path::PathBuf;

use wax_lang_compose::discover::discover_registry_symbols;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/src/main/kotlin")
}

#[test]
fn discovers_public_top_level_composables() {
    let symbols = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

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
