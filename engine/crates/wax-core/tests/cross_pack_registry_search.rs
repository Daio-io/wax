//! Cross-pack merge → search integration at the wax-core seam.
//!
//! React, Compose, and Swift shaped registries (with shared enrichment facts)
//! merge safely, then fuzzy search/detail resolve across the combined snapshots.

use serde_json::Map;
use wax_contract::{
    LanguageId, REGISTRY_SCHEMA_VERSION, RegistryEntryKind, RegistryExtractionStatus,
    RegistrySnapshotRef, registry_from_json,
};
use wax_core::registry_cache::RegistrySnapshot;
use wax_core::registry_merge::merge_registry;
use wax_core::registry_search::{
    RegistrySearchOptions, lookup_registry_entry, search_registry_entries,
};

fn snapshot(source: &str, language: &str, registry: serde_json::Value) -> RegistrySnapshot {
    RegistrySnapshot {
        reference: RegistrySnapshotRef {
            source: source.to_owned(),
            language: LanguageId::try_from(language).unwrap(),
            digest: format!("{language}{}", "a".repeat(58)),
            system_id: Some("acme".to_owned()),
            git_commit: None,
            extra: Map::new(),
        },
        document: registry_from_json(&registry.to_string()).unwrap(),
    }
}

fn enriched_pack_registry(language: &str, component_id: &str, package: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": REGISTRY_SCHEMA_VERSION,
        "language": language,
        "components": [{
            "id": component_id,
            "symbol": "RichButton",
            "package": package,
            "metadata": {
                "description": "Publisher seed description",
                "tags": ["seed"]
            },
            "extracted": {
                "status": "complete",
                "documentation": "A button used for important actions.",
                "signature": {
                    "parameters": [
                        {"name": "label", "type": "string"},
                        {"name": "onClick", "type": "function"}
                    ]
                },
                "deprecation": {"message": "Prefer PrimaryButton"},
                "attachments": [{
                    "kind": "example",
                    "path": "examples/rich-button",
                    "content": "example"
                }]
            }
        }],
        "tokens": [{
            "id": format!("{language}.color.primary"),
            "key": "color.primary",
            "category": "color",
            "value": "#ff00aa"
        }]
    })
}

#[test]
fn cross_pack_merge_then_search_and_detail_share_enrichment_semantics() {
    let packs = [
        ("react", "react.rich-button", "@acme/design-system"),
        ("compose", "compose.rich-button", "com.example.ds"),
        ("swift", "swift.rich-button", "AcmeDesignSystem"),
    ];

    let mut snapshots = Vec::new();
    for (language, component_id, package) in packs {
        let seed = registry_from_json(
            &serde_json::json!({
                "schema_version": REGISTRY_SCHEMA_VERSION,
                "components": [{
                    "id": component_id,
                    "symbol": "RichButton",
                    "package": package,
                    "metadata": {
                        "description": "Publisher seed description",
                        "tags": ["seed"]
                    }
                }]
            })
            .to_string(),
        )
        .unwrap();
        let discovered = registry_from_json(
            &enriched_pack_registry(language, component_id, package).to_string(),
        )
        .unwrap();
        let merged = merge_registry(Some(&seed), &discovered, true).expect("merge");
        assert_eq!(
            merged.document.components[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.description.as_deref()),
            Some("Publisher seed description"),
            "{language} merge retains publisher metadata"
        );
        assert_eq!(
            merged.document.components[0]
                .extracted
                .as_ref()
                .map(|extracted| extracted.status),
            Some(RegistryExtractionStatus::Complete),
            "{language} merge keeps complete enrichment"
        );
        let mut document = merged.document;
        document.extra.insert(
            "language".to_owned(),
            serde_json::Value::String(language.to_owned()),
        );
        snapshots.push(snapshot(
            &format!("publisher:fixture#{language}"),
            language,
            serde_json::to_value(&document).unwrap(),
        ));
    }

    let search = search_registry_entries(
        &snapshots,
        "richbutton",
        &RegistrySearchOptions {
            kind: Some(RegistryEntryKind::Component),
            language: None,
            limit: Some(10),
        },
    )
    .expect("cross-pack search");
    assert_eq!(search.results.len(), 3);
    let mut languages: Vec<_> = search
        .results
        .iter()
        .map(|result| result.snapshot.language.as_str().to_owned())
        .collect();
    languages.sort();
    assert_eq!(languages, vec!["compose", "react", "swift"]);
    for result in &search.results {
        assert_eq!(
            result.extraction_status,
            Some(RegistryExtractionStatus::Complete)
        );
    }

    let react_only = search_registry_entries(
        &snapshots,
        "button",
        &RegistrySearchOptions {
            kind: Some(RegistryEntryKind::Component),
            language: Some(LanguageId::try_from("react").unwrap()),
            limit: Some(5),
        },
    )
    .expect("language-filtered search");
    assert!(
        react_only
            .results
            .iter()
            .all(|result| result.snapshot.language.as_str() == "react")
    );

    let detail = lookup_registry_entry(
        &snapshots,
        RegistryEntryKind::Component,
        "compose.rich-button",
    )
    .expect("compose detail");
    assert_eq!(detail.snapshot.language.as_str(), "compose");
    assert_eq!(detail.entry["symbol"], "RichButton");
    assert_eq!(
        detail.entry["extracted"]["documentation"],
        "A button used for important actions."
    );
    assert_eq!(
        detail.entry["metadata"]["description"],
        "Publisher seed description"
    );

    let token = lookup_registry_entry(&snapshots, RegistryEntryKind::Token, "swift.color.primary")
        .expect("swift token detail");
    assert_eq!(token.snapshot.language.as_str(), "swift");
    assert_eq!(token.entry["value"], "#ff00aa");
}
