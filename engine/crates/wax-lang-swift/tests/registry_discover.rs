use std::path::PathBuf;
use std::{fs, io};

use wax_lang_api::{DiscoverRequest, DiscoverRequestType};
use wax_lang_swift::{SwiftLanguage, discover::discover_registry_symbols};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover")
}

fn fixture_sources_root() -> PathBuf {
    fixture_root().join("design-system/Sources")
}

#[test]
fn discovers_public_and_package_swiftui_symbols() {
    let result = discover_registry_symbols(&[fixture_sources_root()]).expect("discover symbols");
    assert_eq!(
        result.symbols,
        vec!["Badge", "PackageCard", "PrimaryButton"]
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn missing_discovery_root_fails() {
    let missing = fixture_sources_root().join("missing");
    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");
    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_skipped_with_diagnostics() {
    let root = fixture_root().join("broken/Sources");
    let result = discover_registry_symbols(&[root]).expect("discover should continue");
    assert!(result.symbols.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    assert!(result.diagnostics[0].message.contains("Broken.swift"));
}

#[test]
fn duplicate_symbols_are_deduped_and_nested_symbols_are_excluded() {
    let result = discover_registry_symbols(&[fixture_sources_root()]).expect("discover symbols");
    assert_eq!(
        result
            .symbols
            .iter()
            .filter(|symbol| *symbol == "PrimaryButton")
            .count(),
        1
    );
    assert!(!result.symbols.iter().any(|symbol| symbol == "NestedCard"));
}

#[test]
fn swift_language_discover_joins_repo_relative_roots() {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: 1,
        language_id: "swift".try_into().expect("swift id"),
        repo_root: fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
    };

    let result = SwiftLanguage::new()
        .discover(&request)
        .expect("discover via language wrapper");
    assert_eq!(
        result.symbols,
        vec!["Badge", "PackageCard", "PrimaryButton"]
    );
}

#[test]
fn parse_failures_do_not_block_symbols_from_other_files() -> io::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let good_file = tempdir.path().join("Good.swift");
    let broken_file = tempdir.path().join("Broken.swift");
    fs::write(
        &good_file,
        "@MainActor\nstruct GoodButton: View { var body: some View { Text(\"ok\") } }\n",
    )?;
    fs::write(&broken_file, "struct Broken(")?;

    let result = discover_registry_symbols(&[tempdir.path().to_path_buf()])
        .expect("discover should continue after parse failure");

    assert_eq!(result.symbols, vec!["GoodButton"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "parse_failed");
    Ok(())
}
