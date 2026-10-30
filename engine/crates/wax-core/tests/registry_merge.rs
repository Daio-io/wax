use wax_contract::{RegistryDocument, RegistryExtractionStatus, registry_from_json};
use wax_core::registry_merge::{
    RegistryMergeChangeKind, RegistryMergeDiagnosticCode, RegistryMergeEntryKind, merge_registry,
    replace_registry,
};

fn registry(json: &str) -> RegistryDocument {
    registry_from_json(json).expect("valid registry fixture")
}

#[test]
fn replacement_reports_added_updated_and_removed_entries() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {"id": "button", "symbol": "Button", "details": {"status": "complete"}},
                {"id": "legacy", "symbol": "Legacy"}
            ],
            "tokens": [
                {"id": "color.brand", "key": "color.brand", "category": "color"},
                {"id": "color.legacy", "key": "color.legacy", "category": "color"}
            ]
        }"#,
    );
    let replacement = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {"id": "button", "symbol": "Button", "details": {"status": "partial"}},
                {"id": "card", "symbol": "Card"}
            ],
            "tokens": [
                {"id": "color.brand", "key": "color.brand", "category": "color"},
                {"id": "spacing.small", "key": "spacing.small", "category": "spacing"}
            ]
        }"#,
    );

    let result = replace_registry(Some(&existing), &replacement).expect("registry replaces");

    assert_eq!(result.document, replacement);
    assert_eq!(
        result
            .changes
            .iter()
            .map(|change| (change.entry_kind, change.id.as_str(), change.kind))
            .collect::<Vec<_>>(),
        [
            (
                RegistryMergeEntryKind::Component,
                "legacy",
                RegistryMergeChangeKind::Removed,
            ),
            (
                RegistryMergeEntryKind::Token,
                "color.legacy",
                RegistryMergeChangeKind::Removed,
            ),
            (
                RegistryMergeEntryKind::Component,
                "button",
                RegistryMergeChangeKind::Updated,
            ),
            (
                RegistryMergeEntryKind::Component,
                "card",
                RegistryMergeChangeKind::Added,
            ),
            (
                RegistryMergeEntryKind::Token,
                "spacing.small",
                RegistryMergeChangeKind::Added,
            ),
        ]
    );
}

#[test]
fn replacement_with_no_existing_registry_reports_additions() {
    let replacement = registry(
        r#"{
            "schema_version": 1,
            "components": [{"id": "button", "symbol": "Button"}],
            "tokens": [{"id": "color.brand", "key": "color.brand", "category": "color"}]
        }"#,
    );

    let result = replace_registry(None, &replacement).expect("registry replaces");

    assert_eq!(
        result
            .changes
            .iter()
            .map(|change| (change.entry_kind, change.id.as_str(), change.kind))
            .collect::<Vec<_>>(),
        [
            (
                RegistryMergeEntryKind::Component,
                "button",
                RegistryMergeChangeKind::Added,
            ),
            (
                RegistryMergeEntryKind::Token,
                "color.brand",
                RegistryMergeChangeKind::Added,
            ),
        ]
    );
}

#[test]
fn partial_discovery_with_guidance_replaces_best_practices() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "details": {
                    "status": "complete",
                    "best_practices": [{"description": "Old guidance"}]
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
                "details": {
                    "status": "partial",
                    "best_practices": [{"description": "Fresh guidance"}]
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, false).expect("registry merges");
    let best_practices = &merged.document.components[0]
        .details
        .as_ref()
        .expect("details")
        .best_practices;

    assert_eq!(best_practices.len(), 1);
    assert_eq!(best_practices[0].description, "Fresh guidance");
}

#[test]
fn complete_discovery_clears_obsolete_best_practices() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "details": {
                    "status": "complete",
                    "best_practices": [{"description": "Obsolete guidance"}]
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
                "details": {"status": "complete"}
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert!(
        merged.document.components[0]
            .details
            .as_ref()
            .expect("details")
            .best_practices
            .is_empty()
    );
}

#[test]
fn partial_discovery_without_guidance_retains_prior_best_practices() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "details": {
                    "status": "complete",
                    "best_practices": [{"description": "Retained guidance"}]
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
                "details": {"status": "partial"}
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, false).expect("registry merges");
    let best_practices = &merged.document.components[0]
        .details
        .as_ref()
        .expect("details")
        .best_practices;

    assert_eq!(best_practices.len(), 1);
    assert_eq!(best_practices[0].description, "Retained guidance");
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
                "details": {
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
                "details": {
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
                "details": {
                    "status": "complete",
                    "documentation": "Fresh generated docs"
                }
            }],
            "tokens": [{
                "id": "generated-color-id",
                "key": "color.brand",
                "category": "color",
                "value": "rgb(102 51 153)",
                "details": {
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
            .details
            .as_ref()
            .and_then(|details| details.documentation.as_deref()),
        Some("Fresh generated docs")
    );

    let token = &merged.document.tokens[0];
    assert_eq!(token.value.as_deref(), Some("#663399"));
    assert_eq!(token.aliases, ["brand.color"]);
    assert_eq!(token.extra["visibility"], "public");
    assert_eq!(
        token
            .details
            .as_ref()
            .and_then(|details| details.documentation.as_deref()),
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
                    "details": {
                        "status": "complete",
                        "documentation": "Old button docs",
                        "signature": {"type_text": "fun Button(old: String)"}
                    }
                },
                {
                    "id": "card",
                    "symbol": "Card",
                    "package": "ui",
                    "details": {
                        "status": "complete",
                        "documentation": "Card docs"
                    }
                },
                {
                    "id": "badge",
                    "symbol": "Badge",
                    "package": "ui",
                    "details": {
                        "status": "complete",
                        "documentation": "Badge docs"
                    }
                }
            ],
            "tokens": [{
                "id": "spacing-small",
                "key": "spacing.small",
                "category": "spacing",
                "details": {
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
                    "details": {
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
                "details": {
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
        button.details.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Complete)
    );
    assert!(
        button
            .details
            .as_ref()
            .and_then(|facts| facts.signature.as_ref())
            .is_none()
    );

    let card = &merged.document.components[1];
    assert_eq!(
        card.details.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::NotObserved)
    );
    assert_eq!(
        card.details
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Card docs")
    );

    let badge = &merged.document.components[2];
    assert_eq!(
        badge.details.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Stale)
    );
    assert_eq!(
        badge
            .details
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Badge docs")
    );

    let token = &merged.document.tokens[0];
    assert_eq!(
        token.details.as_ref().map(|facts| facts.status),
        Some(RegistryExtractionStatus::Partial)
    );
    assert_eq!(
        token
            .details
            .as_ref()
            .and_then(|facts| facts.documentation.as_deref()),
        Some("Fresh spacing docs")
    );
    assert_eq!(
        token
            .details
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
                "details": {
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
                "details": {
                    "status": "complete",
                    "documentation": "Fresh docs"
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");
    let details = merged.document.components[0]
        .details
        .as_ref()
        .expect("details");

    assert!(details.signature.is_none());
    assert!(!details.extra.contains_key("old_extractor_field"));
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
                    "details": {"status": "complete", "documentation": "Primary docs"}
                },
                {
                    "id": "secondary-button",
                    "symbol": "Button",
                    "metadata": {"description": "Secondary authored content"},
                    "details": {"status": "complete", "documentation": "Secondary docs"}
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
                "details": {"status": "complete", "documentation": "Fresh docs"}
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
        let details = candidate.details.as_ref().expect("details");
        assert_eq!(details.status, RegistryExtractionStatus::Complete);
        assert!(
            matches!(
                details.documentation.as_deref(),
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
                "details": {
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
                "details": {
                    "status": "partial",
                    "documentation": "Fresh docs"
                }
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");
    let details = merged.document.components[0]
        .details
        .as_ref()
        .expect("details");

    assert_eq!(details.status, RegistryExtractionStatus::Partial);
    assert_eq!(details.documentation.as_deref(), Some("Fresh docs"));
    assert_eq!(details.field_gaps.len(), 1);
    assert_eq!(details.field_gaps[0].field, "signature");
    assert_eq!(details.field_gaps[0].reason, "parser unavailable");
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
                    "details": {"status": "complete", "documentation": "Button docs"}
                },
                {
                    "id": "card",
                    "symbol": "Card",
                    "details": {"status": "complete", "documentation": "Card docs"}
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
                "details": {"status": "complete", "documentation": "Fresh button docs"}
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
            .details
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
                {"id": "button-a", "symbol": "Button", "details": {"status": "complete"}},
                {"id": "button-b", "symbol": "Button", "details": {"status": "complete"}}
            ],
            "tokens": [
                {"id": "color-a", "key": "color.brand", "category": "color", "details": {"status": "complete"}},
                {"id": "color-b", "key": "color.brand", "category": "color", "details": {"status": "complete"}}
            ]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert_eq!(merged.document.components.len(), 2);
    assert_eq!(merged.document.tokens.len(), 2);
    assert!(merged.diagnostics.is_empty());
}

#[test]
fn id_fallback_mismatch_retains_extraction_on_both_candidates() {
    // Exact id hits entry A while a unique package/symbol (or token key) hits B.
    // Conflict must be diagnostic-only for both indices — A must not become not_observed.
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "button-a",
                    "symbol": "Card",
                    "details": {"status": "complete", "documentation": "A docs"}
                },
                {
                    "id": "button-b",
                    "symbol": "Button",
                    "package": "@acme/ui",
                    "details": {"status": "complete", "documentation": "B docs"}
                }
            ],
            "tokens": [
                {
                    "id": "color-a",
                    "key": "color.other",
                    "category": "color",
                    "details": {"status": "complete", "documentation": "A docs"}
                },
                {
                    "id": "color-b",
                    "key": "color.brand",
                    "category": "color",
                    "details": {"status": "complete", "documentation": "B docs"}
                }
            ]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{
                "id": "button-a",
                "symbol": "Button",
                "package": "@acme/ui",
                "details": {"status": "complete", "documentation": "Fresh component docs"}
            }],
            "tokens": [{
                "id": "color-a",
                "key": "color.brand",
                "category": "color",
                "details": {"status": "complete", "documentation": "Fresh token docs"}
            }]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    for (kind, index, docs) in [
        ("component", 0usize, "A docs"),
        ("component", 1, "B docs"),
        ("token", 0, "A docs"),
        ("token", 1, "B docs"),
    ] {
        let details = match kind {
            "component" => merged.document.components[index]
                .details
                .as_ref()
                .expect("component details"),
            "token" => merged.document.tokens[index]
                .details
                .as_ref()
                .expect("token details"),
            _ => unreachable!(),
        };
        assert_eq!(
            details.status,
            RegistryExtractionStatus::Complete,
            "{kind}[{index}] must retain complete extraction on id/fallback conflict"
        );
        assert_eq!(
            details.documentation.as_deref(),
            Some(docs),
            "{kind}[{index}] docs must stay untouched"
        );
    }

    assert!(!merged.changes.iter().any(|change| {
        matches!(
            (change.entry_kind, change.id.as_str(), change.kind),
            (
                RegistryMergeEntryKind::Component,
                "button-a" | "button-b",
                RegistryMergeChangeKind::Updated
            ) | (
                RegistryMergeEntryKind::Token,
                "color-a" | "color-b",
                RegistryMergeChangeKind::Updated
            )
        )
    }));
    assert_eq!(merged.diagnostics.len(), 2);
    assert!(
        merged
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code == RegistryMergeDiagnosticCode::IdentityConflict })
    );
    assert!(
        merged.diagnostics[0].message.contains("button-a")
            && merged.diagnostics[0].message.contains("button-b"),
        "component conflict should name both candidates: {}",
        merged.diagnostics[0].message
    );
    assert!(
        merged.diagnostics[1].message.contains("color-a")
            && merged.diagnostics[1].message.contains("color-b"),
        "token conflict should name both candidates: {}",
        merged.diagnostics[1].message
    );
}

#[test]
fn exact_ids_disambiguate_ambiguous_component_and_token_fallbacks() {
    let existing = registry(
        r#"{
            "schema_version": 1,
            "components": [
                {"id": "button-a", "symbol": "Button", "details": {"status": "complete", "documentation": "old a"}},
                {"id": "button-b", "symbol": "Button", "details": {"status": "complete", "documentation": "old b"}}
            ],
            "tokens": [
                {"id": "color-a", "key": "color.brand", "category": "color", "details": {"status": "complete", "documentation": "old a"}},
                {"id": "color-b", "key": "color.brand", "category": "color", "details": {"status": "complete", "documentation": "old b"}}
            ]
        }"#,
    );
    let discovered = registry(
        r#"{
            "schema_version": 1,
            "components": [{"id": "button-b", "symbol": "Button", "details": {"status": "complete", "documentation": "fresh"}}],
            "tokens": [{"id": "color-b", "key": "color.brand", "category": "color", "details": {"status": "complete", "documentation": "fresh"}}]
        }"#,
    );

    let merged = merge_registry(Some(&existing), &discovered, true).expect("registry merges");

    assert_eq!(merged.diagnostics.len(), 0);
    assert_eq!(
        merged.document.components[1]
            .details
            .as_ref()
            .unwrap()
            .documentation
            .as_deref(),
        Some("fresh")
    );
    assert_eq!(
        merged.document.tokens[1]
            .details
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
                "details": {
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
                "details": {"status": "complete"}
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
