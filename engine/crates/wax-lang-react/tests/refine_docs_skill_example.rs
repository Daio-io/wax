//! Keeps the `wax-refine-docs` skill's golden react example aligned with extraction.

use std::path::PathBuf;
use std::{fs, io};

use wax_contract::{RegistryDetails, RegistryDocument};
use wax_lang_api::{DiscoverRegistryRequest, WIRE_API_VERSION};
use wax_lang_react::discover_registry_details;

fn skill_examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../skills/wax-refine-docs/examples/react")
}

fn discovered_details() -> io::Result<RegistryDetails> {
    let tempdir = tempfile::tempdir()?;
    let root = tempdir.path().join("src");
    fs::create_dir_all(&root)?;
    fs::copy(
        skill_examples().join("AcmeButton.tsx"),
        root.join("AcmeButton.tsx"),
    )?;
    let registry: RegistryDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "components": [{"id": "acme.button", "symbol": "AcmeButton"}]
    }))
    .expect("registry fixture");
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().expect("language id"),
        repo_root: tempdir.path().to_string_lossy().into_owned(),
        roots: vec!["src".to_owned()],
        registry,
    };
    let result = discover_registry_details(&request).expect("discover details");
    Ok(result.document.components[0]
        .details
        .clone()
        .expect("example component should be enriched"))
}

#[test]
fn refine_docs_skill_example_extracts_every_documented_fact() -> io::Result<()> {
    let details = serde_json::to_value(discovered_details()?).expect("details serialize");
    let expected_path = skill_examples().join("AcmeButton.expected.json");
    if std::env::var_os("WAX_BLESS").is_some() {
        let mut json = serde_json::to_string_pretty(&details).expect("details serialize");
        json.push('\n');
        fs::write(&expected_path, json)?;
    }
    let expected: serde_json::Value = serde_json::from_str(&fs::read_to_string(&expected_path)?)
        .expect("expected details should be JSON");

    assert_eq!(
        details, expected,
        "the skill example and its expected details drifted; rerun with WAX_BLESS=1 after review"
    );
    Ok(())
}
