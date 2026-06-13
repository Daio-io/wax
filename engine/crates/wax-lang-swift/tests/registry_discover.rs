use std::path::PathBuf;

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
    let symbols = discover_registry_symbols(&[fixture_sources_root()]).expect("discover symbols");
    assert_eq!(symbols, vec!["Badge", "PackageCard", "PrimaryButton"]);
}

#[test]
fn missing_discovery_root_fails() {
    let missing = fixture_sources_root().join("missing");
    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");
    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_reported() {
    let root = fixture_root().join("broken/Sources");
    let err = discover_registry_symbols(&[root]).expect_err("parse should fail");
    assert!(err.to_string().contains("failed to parse"));
    assert!(err.to_string().contains("Broken.swift"));
}

#[test]
fn duplicate_symbols_are_deduped_and_nested_symbols_are_excluded() {
    let symbols = discover_registry_symbols(&[fixture_sources_root()]).expect("discover symbols");
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| *symbol == "PrimaryButton")
            .count(),
        1
    );
    assert!(!symbols.iter().any(|symbol| symbol == "NestedCard"));
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
