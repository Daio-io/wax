use serde_json::Map;
use wax_contract::{
    LanguageId, REGISTRY_SCHEMA_VERSION, RegistryEntryKind, RegistryLookupErrorCode,
    RegistrySnapshotRef, registry_from_json,
};
use wax_core::registry_cache::RegistrySnapshot;
use wax_core::registry_search::{
    RegistrySearchOptions, lookup_registry_entry, search_registry_entries,
};

fn snapshot(source: &str, language: &str, registry: serde_json::Value) -> RegistrySnapshot {
    RegistrySnapshot {
        reference: RegistrySnapshotRef {
            source: source.to_owned(),
            language: LanguageId::try_from(language).unwrap(),
            digest: "a".repeat(64),
            system_id: None,
            git_commit: None,
            extra: Map::new(),
        },
        document: registry_from_json(&registry.to_string()).unwrap(),
    }
}

fn registry(components: serde_json::Value, tokens: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schema_version": REGISTRY_SCHEMA_VERSION,
        "components": components,
        "tokens": tokens,
    })
}

#[test]
fn fuzzy_search_covers_components_tokens_and_aliases() {
    let snapshots = [snapshot(
        "file:./.wax/react.registry.json",
        "react",
        registry(
            serde_json::json!([{
                "id": "primary-button",
                "symbol": "Button",
                "package": "@acme/ui",
                "aliases": ["ActionButton"],
                "summary": "Call to action",
                "metadata": {
                    "description": "Primary action control",
                    "tags": ["controls"]
                },
                "details": {
                    "status": "complete",
                    "documentation": "Use in confirmation dialogs",
                    "attachments": [{
                        "kind": "example",
                        "path": "examples/button.tsx",
                        "content": "Checkout submit flow"
                    }]
                }
            }]),
            serde_json::json!([{
                "id": "color-primary",
                "key": "color.primary",
                "category": "color",
                "aliases": ["brand-color"],
                "value": "#FF00AA"
            }]),
        ),
    )];
    let options = RegistrySearchOptions::default();

    let typo = search_registry_entries(&snapshots, "buton", &options).unwrap();
    assert_eq!(typo.results[0].id, "primary-button");

    let alias = search_registry_entries(&snapshots, "action_button", &options).unwrap();
    assert_eq!(alias.results[0].id, "primary-button");
    assert_eq!(alias.results[0].aliases, ["ActionButton"]);

    let tag = search_registry_entries(&snapshots, "controls", &options).unwrap();
    assert_eq!(tag.results[0].tags, ["controls"]);

    let value = search_registry_entries(&snapshots, "#ff00aa", &options).unwrap();
    assert_eq!(value.results[0].id, "color-primary");

    let separated = search_registry_entries(&snapshots, "COLOR PRIMARY", &options).unwrap();
    assert_eq!(separated.results[0].kind, RegistryEntryKind::Token);
    assert_eq!(separated.results[0].id, "color-primary");

    for query in [
        "@ACME/UI",
        "controls",
        "call-to-action",
        "primary action",
        "confirmation dialogs",
        "checkout submit",
    ] {
        let response = search_registry_entries(&snapshots, query, &options).unwrap();
        assert_eq!(response.results[0].id, "primary-button", "query: {query}");
    }

    let category = search_registry_entries(&snapshots, "color", &options).unwrap();
    assert_eq!(category.results[0].id, "color-primary");
}

#[test]
fn tag_match_outranks_description_only_match() {
    let snapshots = [snapshot(
        "tag-ranking",
        "react",
        registry(
            serde_json::json!([
                {
                    "id": "tagged-navigation",
                    "symbol": "Menu",
                    "metadata": {"tags": ["navigation"]}
                },
                {
                    "id": "described-navigation",
                    "symbol": "Breadcrumbs",
                    "metadata": {"description": "navigation"}
                }
            ]),
            serde_json::json!([]),
        ),
    )];

    let response = search_registry_entries(&snapshots, "navigation", &Default::default()).unwrap();

    assert_eq!(
        response
            .results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        ["tagged-navigation", "described-navigation"]
    );
    assert_eq!(response.results[0].tags, ["navigation"]);
}

#[test]
fn token_tag_and_empty_metadata_edges() {
    let snapshots = [snapshot(
        "metadata-edges",
        "react",
        registry(
            serde_json::json!([{
                "id": "plain-component",
                "symbol": "PlainComponent"
            }]),
            serde_json::json!([{
                "id": "color-brand",
                "key": "color.brand",
                "category": "color",
                "metadata": {"tags": ["brand"]}
            }]),
        ),
    )];

    let tagged = search_registry_entries(&snapshots, "brand", &Default::default()).unwrap();
    assert_eq!(tagged.results[0].tags, ["brand"]);

    let plain =
        search_registry_entries(&snapshots, "plain component", &Default::default()).unwrap();
    let value = serde_json::to_value(&plain.results[0]).unwrap();
    assert!(value.get("aliases").is_none());
    assert!(value.get("tags").is_none());
}

#[test]
fn ranking_filters_and_ties_are_stable() {
    let long_summary = "🦀".repeat(241);
    let snapshots = [
        snapshot(
            "z-source",
            "react",
            registry(
                serde_json::json!([
                    {"id": "shared", "symbol": "Button"},
                    {
                        "id": "description-only",
                        "symbol": "Card",
                        "metadata": {"description": "Button"}
                    }
                ]),
                serde_json::json!([{
                    "id": "shared",
                    "key": "shared",
                    "category": "color"
                }]),
            ),
        ),
        snapshot(
            "a-source",
            "react",
            registry(
                serde_json::json!([{
                    "id": "shared",
                    "symbol": "Button",
                    "summary": long_summary
                }]),
                serde_json::json!([]),
            ),
        ),
        snapshot(
            "compose-source",
            "compose",
            registry(
                serde_json::json!([{"id": "shared", "symbol": "Button"}]),
                serde_json::json!([]),
            ),
        ),
    ];

    let ranked = search_registry_entries(&snapshots, "button", &Default::default()).unwrap();
    let name_position = ranked
        .results
        .iter()
        .position(|result| result.id == "shared")
        .unwrap();
    let description_position = ranked
        .results
        .iter()
        .position(|result| result.id == "description-only")
        .unwrap();
    assert!(name_position < description_position);

    let filtered = search_registry_entries(
        &snapshots,
        "shared",
        &RegistrySearchOptions {
            kind: Some(RegistryEntryKind::Component),
            language: Some(LanguageId::try_from("react").unwrap()),
            limit: Some(2),
        },
    )
    .unwrap();
    assert_eq!(filtered.results.len(), 2);
    assert_eq!(filtered.results[0].snapshot.source, "a-source");
    assert_eq!(filtered.results[1].snapshot.source, "z-source");
    assert_eq!(filtered.results[0].summary.chars().count(), 240);
    assert!(
        filtered.results[0]
            .summary
            .is_char_boundary(filtered.results[0].summary.len())
    );

    let fuzzy_ranked = search_registry_entries(
        &[snapshot(
            "fuzzy-source",
            "react",
            registry(
                serde_json::json!([
                    {"id": "a-less-similar", "symbol": "Buton"},
                    {"id": "z-more-similar", "symbol": "Button"}
                ]),
                serde_json::json!([]),
            ),
        )],
        "buttn",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(fuzzy_ranked.results[0].id, "z-more-similar");

    let many_components = (0..21)
        .map(|index| {
            serde_json::json!({
                "id": format!("item-{index:02}"),
                "symbol": format!("Item{index:02}")
            })
        })
        .collect::<Vec<_>>();
    let default_limited = search_registry_entries(
        &[snapshot(
            "many-source",
            "react",
            registry(
                serde_json::Value::Array(many_components),
                serde_json::json!([]),
            ),
        )],
        "item",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(default_limited.filters.limit, 20);
    assert_eq!(default_limited.results.len(), 20);
}

#[test]
fn exact_details_retain_metadata_extensions_and_report_stable_errors() {
    let entry = serde_json::json!({
        "id": "ds.button",
        "symbol": "Button",
        "package": "@acme/ui",
        "aliases": ["ActionButton"],
        "metadata": {
            "description": "Primary action control",
            "tags": ["control"],
            "owner": "design-systems"
        },
        "details": {
            "status": "complete",
            "documentation": "Use for primary actions.",
            "signature": {
                "parameters": [{
                    "name": "size",
                    "type_kind": "enum",
                    "allowed_values": ["small", "large"],
                    "description": "Visual size"
                }]
            },
            "best_practices": [{"description": "Use one primary action"}],
            "attachments": [{
                "kind": "example",
                "path": "examples/button.tsx",
                "content": "<Button />",
                "framework": "react"
            }]
        },
        "legacy_flag": true
    });
    let first = snapshot(
        "a-source",
        "react",
        registry(serde_json::json!([entry]), serde_json::json!([])),
    );

    let detail = lookup_registry_entry(
        std::slice::from_ref(&first),
        RegistryEntryKind::Component,
        "ds.button",
    )
    .unwrap();
    assert_eq!(detail.snapshot, first.reference);
    assert_eq!(detail.entry["legacy_flag"], true);
    assert_eq!(detail.entry["metadata"]["owner"], "design-systems");
    assert_eq!(
        detail.entry["details"]["attachments"][0]["framework"],
        "react"
    );
    assert_eq!(
        detail.entry["details"]["signature"]["parameters"][0]["type_kind"],
        "enum"
    );
    assert_eq!(
        detail.entry["details"]["best_practices"][0]["description"],
        "Use one primary action"
    );

    let absent = lookup_registry_entry(
        std::slice::from_ref(&first),
        RegistryEntryKind::Component,
        "ds.buton",
    )
    .unwrap_err();
    assert_eq!(absent.code, RegistryLookupErrorCode::EntryNotFound);

    let duplicate = snapshot(
        "b-source",
        "react",
        registry(
            serde_json::json!([{"id": "ds.button", "symbol": "Button"}]),
            serde_json::json!([]),
        ),
    );
    let ambiguous = lookup_registry_entry(
        &[first.clone(), duplicate],
        RegistryEntryKind::Component,
        "ds.button",
    )
    .unwrap_err();
    assert_eq!(ambiguous.code, RegistryLookupErrorCode::AmbiguousEntry);

    let empty_query =
        search_registry_entries(std::slice::from_ref(&first), "  ", &Default::default())
            .unwrap_err();
    assert_eq!(empty_query.code, RegistryLookupErrorCode::InvalidArgument);

    let zero_limit = search_registry_entries(
        std::slice::from_ref(&first),
        "button",
        &RegistrySearchOptions {
            limit: Some(0),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(zero_limit.code, RegistryLookupErrorCode::InvalidArgument);

    let punctuation_only =
        search_registry_entries(std::slice::from_ref(&first), "!!!", &Default::default()).unwrap();
    assert!(punctuation_only.results.is_empty());

    let no_matches = search_registry_entries(
        &[first],
        "unrelated-query-with-no-match",
        &Default::default(),
    )
    .unwrap();
    assert!(no_matches.results.is_empty());
}

#[test]
fn search_scores_typed_detail_fields_not_nested_extra_keys() {
    let snapshots = [snapshot(
        "typed-source",
        "react",
        registry(
            serde_json::json!([
                {
                    "id": "with-details-docs",
                    "symbol": "Panel",
                    "details": {
                        "status": "complete",
                        "documentation": "Use in confirmation dialogs",
                        "signature": {
                            "parameters": [
                                {
                                    "name": "emphasisLevel",
                                    "type_text": "InteractionMode",
                                    "type_kind": "enum",
                                    "allowed_values": ["prominent", "quiet"],
                                    "description": "Visual priority treatment",
                                    "default": "quiet"
                                },
                                {
                                    "name": "items",
                                    "type_kind": "collection"
                                }
                            ]
                        },
                        "best_practices": [{
                            "description": "Avoid competing hero actions"
                        }],
                        "attachments": [{
                            "kind": "example",
                            "path": "examples/panel.tsx",
                            "content": "Checkout submit flow"
                        }]
                    }
                },
                {
                    "id": "nested-extra-noise",
                    "symbol": "Card",
                    "nested_docs": {
                        "documentation": "Use in confirmation dialogs",
                        "content": "Checkout submit flow",
                        "name": "emphasisLevel",
                        "type_text": "InteractionMode",
                        "type_kind": "collection",
                        "allowed_values": ["prominent"],
                        "description": "Visual priority treatment",
                        "default": "quiet",
                        "best_practice": "Avoid competing hero actions"
                    }
                },
                {
                    "id": "all-summary-sources",
                    "symbol": "SummaryPrimary",
                    "summary": "Published summary",
                    "metadata": {"description": "Creator description"},
                    "details": {
                        "status": "complete",
                        "documentation": "Generated documentation"
                    }
                },
                {
                    "id": "metadata-summary",
                    "symbol": "MetadataSummary",
                    "metadata": {"description": "Creator fallback"},
                    "details": {
                        "status": "complete",
                        "documentation": "Generated fallback"
                    }
                },
                {
                    "id": "empty-summary",
                    "symbol": "EmptySummary"
                }
            ]),
            serde_json::json!([]),
        ),
    )];

    for query in ["confirmation dialogs", "checkout submit"] {
        let response = search_registry_entries(&snapshots, query, &Default::default()).unwrap();
        assert_eq!(
            response.results.len(),
            1,
            "query `{query}` should ignore nested extra documentation/content keys"
        );
        assert_eq!(response.results[0].id, "with-details-docs");
    }

    for query in [
        "emphasis level",
        "interaction mode",
        "collection",
        "prominent",
        "quiet",
        "visual priority treatment",
        "competing hero actions",
    ] {
        let response = search_registry_entries(&snapshots, query, &Default::default()).unwrap();
        assert_eq!(
            response.results.len(),
            1,
            "query `{query}` should index only typed details fields"
        );
        assert_eq!(response.results[0].id, "with-details-docs");
    }

    let response =
        search_registry_entries(&snapshots, "confirmation dialogs", &Default::default()).unwrap();
    assert_eq!(response.results[0].summary, "Use in confirmation dialogs");

    for (query, expected_summary) in [
        ("summary primary", "Published summary"),
        ("metadata summary", "Creator fallback"),
        ("empty summary", ""),
    ] {
        let response = search_registry_entries(&snapshots, query, &Default::default()).unwrap();
        assert_eq!(
            response.results[0].summary, expected_summary,
            "query: {query}"
        );
    }
}
