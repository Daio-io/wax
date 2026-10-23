use wax_contract::{RegistryDocument, RegistryExtractionStatus, registry_from_json};
use wax_core::registry_merge::{
    RegistryMergeChangeKind, RegistryMergeDiagnosticCode, RegistryMergeEntryKind, merge_registry,
};

fn registry(json: &str) -> RegistryDocument {
    registry_from_json(json).expect("valid registry fixture")
}

#[test]
fn rediscovery_preserves_creator_content_and_tokens() {
    let existing = registry(
        r##"{
            "schema_version": 1,
            "publisher": "design-systems",
            "components": [{
                "id": "authored-button-id",
                "symbol": "Button",
                "package": "@acme/ui",
                "aliases": ["PrimaryButton"],
                "metadata": {
                    "description": "Use for important actions",
                    "owner": "design-systems"
                },
                "extracted": {
                    "status": "complete",
                    "documentation": "Old generated docs"
                },
                "review_state": "approved"
            }],
            "tokens": [{
                "id": "authored-color-id",
                "key": "color.brand",
                "category": "color",
                "aliases": ["brand.color"],
                "value": "#663399",
                "metadata": {"description": "Canonical brand color"},
                "extracted": {
                    "status": "complete",
                    "documentation": "Old token docs"
                },
                "visibility": "public"
            }]
        }"##,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "generated-button-id",
                "symbol": "Button",
                "package": "@acme/ui",
                "extracted": {
                    "status": "complete",
                    "documentation": "Fresh generated docs"
                }
            }],
            "tokens": [{
                "id": "generated-color-id",
                "key": "color.brand",
                "category": "color",
                "value": "rgb(102 51 153)",
                "extracted": {
                    "status": "complete",
                    "documentation": "Fresh token docs"
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    let component = &merged.document.components[0];
    assert_eq!(component.id, "authored-button-id");
    assert_eq!(component.aliases, ["PrimaryButton"]);
    assert_eq!(
        component
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Use for important actions")
    );
    assert_eq!(component.extra["review_state"], "approved");
    assert_eq!(
        component
            .extracted
            .as_ref()
            .and_then(|extracted| extracted.documentation.as_deref()),
        Some("Fresh generated docs")
    );

    let token = &merged.document.tokens[0];
    assert_eq!(token.value.as_deref(), Some("#663399"));
    assert_eq!(token.aliases, ["brand.color"]);
    assert_eq!(token.extra["visibility"], "public");
    assert_eq!(
        token
            .extracted
            .as_ref()
            .and_then(|extracted| extracted.documentation.as_deref()),
        Some("Fresh token docs")
    );

    assert_eq!(merged.changes.len(), 2);
    assert_eq!(
        merged
            .changes
            .iter()
            .map(|change| (change.entry_kind, change.id.as_str(), change.kind))
            .collect::<Vec<_>>(),
        [
            (
                RegistryMergeEntryKind::Component,
                "authored-button-id",
                RegistryMergeChangeKind::Updated,
            ),
            (
                RegistryMergeEntryKind::Token,
                "authored-color-id",
                RegistryMergeChangeKind::Updated,
            ),
        ]
    );
    assert!(merged.diagnostics.is_empty());
}

#[test]
fn partial_or_narrow_discovery_does_not_prune() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "button",
                    "symbol": "Button",
                    "package": "ui",
                    "extracted": {
                        "status": "complete",
                        "documentation": "Old button docs",
                        "signature": {"type_text": "fun Button(old: String)"}
                    }
                },
                {
                    "id": "card",
                    "symbol": "Card",
                    "package": "ui",
                    "extracted": {
                        "status": "complete",
                        "documentation": "Card docs"
                    }
                },
                {
                    "id": "badge",
                    "symbol": "Badge",
                    "package": "ui",
                    "extracted": {
                        "status": "complete",
                        "documentation": "Badge docs"
                    }
                }
            ],
            "tokens": [{
                "id": "spacing-small",
                "key": "spacing.small",
                "category": "spacing",
                "extracted": {
                    "status": "complete",
                    "documentation": "Old spacing docs",
                    "deprecation": {"message": "Use spacing.medium"}
                }
            }]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "button",
                    "symbol": "Button",
                    "package": "ui",
                    "extracted": {
                        "status": "complete",
                        "documentation": "Fresh button docs"
                    }
                },
                {
                    "id": "badge",
                    "symbol": "Badge",
                    "package": "ui"
                }
            ],
            "tokens": [{
                "id": "spacing-small",
                "key": "spacing.small",
                "category": "spacing",
                "extracted": {
                    "status": "partial",
                    "documentation": "Fresh spacing docs",
                    "field_gaps": [{
                        "field": "deprecation",
                        "reason": "root was not scanned"
                    }]
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, false).expect("registry merges");

    let button = &merged.document.components[0];
    assert_eq!(
        button.extracted.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Complete)
    );
    assert!(
        button
            .extracted
            .as_ref()
            .and_then(|facts| facts.signature.as_ref())
            .is_none()
    );

    let card = &merged.document.components[1];
    assert_eq!(
        card.extracted.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::NotObserved)
    );
    assert_eq!(
        card.extracted
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Card docs")
    );

    let badge = &merged.document.components[2];
    assert_eq!(
        badge.extracted.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Stale)
    );
    assert_eq!(
        badge
            .extracted
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Badge docs")
    );

    let token = &merged.document.tokens[0];
    assert_eq!(
        token.extracted.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Partial)
    );
    assert_eq!(
        token
            .extracted
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Fresh spacing docs")
    );
    assert_eq!(
        token
            .extracted
            .as_ref()
            .and_then(|facts| facts.deprecation.as_ref())
            .and_then(|deprecation| deprecation.message.as_deref()),
        Some("Use spacing.medium")
    );
}

#[test]
fn clean_extraction_removes_obsolete_generated_fields() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "extracted": {
                    "status": "complete",
                    "documentation": "Old docs",
                    "signature": {"type_text": "fun Button(old: String)"},
                    "old_extractor_field": true
                }
            }]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "extracted": {
                    "status": "complete",
                    "documentation": "Fresh docs"
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");
    let extracted = merged.document.components[0]
        .extracted
        .as_ref()
        .expect("extracted facts");

    assert!(extracted.signature.is_none());
    assert!(!extracted.extra.contains_key("old_extractor_field"));
}

#[test]
fn identity_conflict_keeps_creator_content() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "primary-button",
                    "symbol": "Button",
                    "metadata": {"description": "Primary authored content"},
                    "extracted": {"status": "complete", "documentation": "Primary docs"}
                },
                {
                    "id": "secondary-button",
                    "symbol": "Button",
                    "metadata": {"description": "Secondary authored content"},
                    "extracted": {"status": "complete", "documentation": "Secondary docs"}
                }
            ]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "generated-button",
                "symbol": "Button",
                "extracted": {"status": "complete", "documentation": "Fresh docs"}
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert_eq!(merged.document.components.len(), 3);
    assert_eq!(
        merged.document.components[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Primary authored content")
    );
    assert_eq!(
        merged.document.components[1]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Secondary authored content")
    );
    for candidate in &merged.document.components[..2] {
        let extracted = candidate.extracted.as_ref().expect("extracted facts");
        assert_eq!(extracted.status, RegistryExtractionStatus::Complete);
        assert!(
            matches!(
                extracted.documentation.as_deref(),
                Some("Primary docs") | Some("Secondary docs")
            ),
            "conflict must not rewrite authored extraction docs"
        );
    }
    assert!(!merged.changes.iter().any(|change| {
        matches!(
            (change.entry_kind, change.id.as_str(), change.kind),
            (
                RegistryMergeEntryKind::Component,
                "primary-button" | "secondary-button",
                RegistryMergeChangeKind::Updated
            )
        )
    }));
    assert_eq!(merged.diagnostics.len(), 1);
    assert_eq!(
        merged.diagnostics[0].code,
        RegistryMergeDiagnosticCode::IdentityConflict
    );
}

#[test]
fn incomplete_discovery_preserves_prior_field_gaps_when_omitted() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "extracted": {
                    "status": "partial",
                    "documentation": "Prior docs",
                    "field_gaps": [{
                        "field": "signature",
                        "reason": "parser unavailable"
                    }]
                }
            }]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "extracted": {
                    "status": "partial",
                    "documentation": "Fresh docs"
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");
    let extracted = merged.document.components[0]
        .extracted
        .as_ref()
        .expect("extracted facts");

    assert_eq!(extracted.status, RegistryExtractionStatus::Partial);
    assert_eq!(extracted.documentation.as_deref(), Some("Fresh docs"));
    assert_eq!(extracted.field_gaps.len(), 1);
    assert_eq!(extracted.field_gaps[0].field, "signature");
    assert_eq!(extracted.field_gaps[0].reason, "parser unavailable");
}

#[test]
fn complete_flag_does_not_change_retention_of_unobserved_entries() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "button",
                    "symbol": "Button",
                    "extracted": {"status": "complete", "documentation": "Button docs"}
                },
                {
                    "id": "card",
                    "symbol": "Card",
                    "extracted": {"status": "complete", "documentation": "Card docs"}
                }
            ]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "extracted": {"status": "complete", "documentation": "Fresh button docs"}
            }]
        }"#,
    );

    let when_complete = merge_registry(Some(&existing), &discovered, true).expect("complete merge");
    let when_incomplete =
        merge_registry(Some(&existing), &discovered, false).expect("incomplete merge");

    assert_eq!(when_complete.document, when_incomplete.document);
    assert_eq!(when_complete.changes, when_incomplete.changes);
    assert_eq!(when_complete.diagnostics, when_incomplete.diagnostics);
    assert_eq!(
        when_complete.document.components[1]
            .extracted
            .as_ref()
            .map(|facts| facts.status),
        Some(RegistryExtractionStatus::NotObserved)
    );
}

#[test]
fn distinct_discoveries_do_not_match_entries_added_during_merge() {
    let existing = registry(r#"{"schema_version": 1}"#);
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {"id": "button-a", "symbol": "Button", "extracted": {"status": "complete"}},
                {"id": "button-b", "symbol": "Button", "extracted": {"status": "complete"}}
            ],
            "tokens": [
                {"id": "color-a", "key": "color.brand", "category": "color", "extracted": {"status": "complete"}},
                {"id": "color-b", "key": "color.brand", "category": "color", "extracted": {"status": "complete"}}
            ]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert_eq!(merged.document.components.len(), 2);
    assert_eq!(merged.document.tokens.len(), 2);
    assert!(merged.diagnostics.is_empty());
}

#[test]
fn exact_ids_disambiguate_ambiguous_component_and_token_fallbacks() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {"id": "button-a", "symbol": "Button", "extracted": {"status": "complete", "documentation": "old a"}},
                {"id": "button-b", "symbol": "Button", "extracted": {"status": "complete", "documentation": "old b"}}
            ],
            "tokens": [
                {"id": "color-a", "key": "color.brand", "category": "color", "extracted": {"status": "complete", "documentation": "old a"}},
                {"id": "color-b", "key": "color.brand", "category": "color", "extracted": {"status": "complete", "documentation": "old b"}}
            ]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{"id": "button-b", "symbol": "Button", "extracted": {"status": "complete", "documentation": "fresh"}}],
            "tokens": [{"id": "color-b", "key": "color.brand", "category": "color", "extracted": {"status": "complete", "documentation": "fresh"}}]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert_eq!(merged.diagnostics.len(), 0);
    assert_eq!(
        merged.document.components[1]
            .extracted
            .as_ref()
            .unwrap()
            .documentation
            .as_deref(),
        Some("fresh")
    );
    assert_eq!(
        merged.document.tokens[1]
            .extracted
            .as_ref()
            .unwrap()
            .documentation
            .as_deref(),
        Some("fresh")
    );
}

#[test]
fn unchanged_rediscovery_is_idempotent() {
    let input = registry(
        r#"{
            "schema_version": 1,
            "registry_extension": {"format": "custom"},
            "components": [{
                "id": "button",
                "symbol": "Button",
                "package": "ui",
                "aliases": ["PrimaryButton"],
                "metadata": {"tags": ["action"]},
                "extracted": {
                    "status": "complete",
                    "documentation": "Button docs",
                    "extractor_extension": "stable"
                }
            }],
            "tokens": [{
                "id": "color-brand",
                "key": "color.brand",
                "category": "color",
                "value": "rebeccapurple",
                "extracted": {"status": "complete"}
            }]
        }"#,
    );

    let first = merge_registry(Some(&input), &input, true).expect("registry merges");
    let second = merge_registry(Some(&first.document), &input, true).expect("registry merges");

    assert_eq!(first.document, input);
    assert!(first.changes.is_empty());
    assert!(first.diagnostics.is_empty());
    assert_eq!(second, first);
}
