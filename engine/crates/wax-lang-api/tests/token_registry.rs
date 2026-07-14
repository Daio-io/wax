use wax_contract::TokenCategory;
use wax_lang_api::{find_token_matches, parse_registry_tokens, token_index};

#[test]
fn parses_tokens_with_aliases() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["AppColors.Primary"]
            }
        ]
    });

    let tokens = parse_registry_tokens(&value).expect("tokens should parse");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].id, "color.primary");
    assert_eq!(tokens[0].key, "Theme.colors.primary");
    assert_eq!(tokens[0].category, TokenCategory::Color);
    assert_eq!(tokens[0].aliases, vec!["AppColors.Primary"]);
}

#[test]
fn missing_tokens_key_is_empty() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}]
    });

    let tokens = parse_registry_tokens(&value).expect("missing tokens should be valid");
    assert!(tokens.is_empty());
}

#[test]
fn token_index_finds_key_and_alias_matches() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["AppColors.Primary"]
            }
        ]
    });
    let tokens = parse_registry_tokens(&value).unwrap();
    let index = token_index(&tokens).unwrap();

    let sites = find_token_matches(
        "val a = Theme.colors.primary\nval b = AppColors.Primary\n",
        "src/Screen.kt",
        &index,
        "token.compose",
    );

    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].token_id, "color.primary");
    assert_eq!(sites[0].key, "Theme.colors.primary");
    assert_eq!(sites[0].category, TokenCategory::Color);
    assert_eq!(sites[0].location.line, 1);
    assert_eq!(sites[1].key, "AppColors.Primary");
    assert_eq!(sites[1].location.line, 2);
}

#[test]
fn find_token_matches_handles_multibyte_utf8_without_panic() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color"
            }
        ]
    });
    let tokens = parse_registry_tokens(&value).unwrap();
    let index = token_index(&tokens).unwrap();

    let sites = find_token_matches(
        "let label = \"Café\"; Theme.colors.primary\n",
        "src/Screen.kt",
        &index,
        "token.basic",
    );

    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].token_id, "color.primary");
    assert_eq!(sites[0].key, "Theme.colors.primary");
}

#[test]
fn find_token_matches_empty_index_walks_utf8_safely() {
    let index = token_index(&[]).unwrap();
    let sites = find_token_matches(
        "Café crème brûlée\n",
        "src/Screen.kt",
        &index,
        "token.basic",
    );
    assert!(sites.is_empty());
}

#[test]
fn longest_match_suppresses_overlapping_alias() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "theme.colors.primary",
                "category": "color",
                "aliases": ["colors.primary"]
            }
        ]
    });
    let tokens = parse_registry_tokens(&value).unwrap();
    let index = token_index(&tokens).unwrap();

    let sites = find_token_matches(
        "val color = theme.colors.primary\n",
        "src/App.tsx",
        &index,
        "token.react",
    );

    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].key, "theme.colors.primary");
    assert_eq!(sites[0].location.column, Some(13));
}

#[test]
fn parse_registry_rejects_duplicate_token_ids() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "theme.colors.primary",
                "category": "color"
            },
            {
                "id": "color.primary",
                "key": "theme.colors.secondary",
                "category": "color"
            }
        ]
    });

    let err = parse_registry_tokens(&value).expect_err("duplicate ids must fail");
    assert!(err.to_string().contains("duplicate token id"));
}

#[test]
fn parse_registry_rejects_empty_alias() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "theme.colors.primary",
                "category": "color",
                "aliases": [""]
            }
        ]
    });

    let err = parse_registry_tokens(&value).expect_err("empty alias must fail");
    assert!(err.to_string().contains("aliases"));
}

#[test]
fn token_index_rejects_duplicate_token_ids() {
    use wax_contract::DesignSystemToken;

    let tokens = vec![
        DesignSystemToken {
            id: "color.primary".into(),
            key: "theme.colors.primary".into(),
            category: TokenCategory::Color,
            aliases: vec![],
        },
        DesignSystemToken {
            id: "color.primary".into(),
            key: "theme.colors.secondary".into(),
            category: TokenCategory::Color,
            aliases: vec![],
        },
    ];

    let err = token_index(&tokens).expect_err("duplicate ids must fail");
    assert!(matches!(
        err,
        wax_lang_api::TokenRegistryError::DuplicateTokenId { .. }
    ));
}

#[test]
fn token_index_rejects_empty_match_key() {
    use wax_contract::DesignSystemToken;

    let tokens = vec![DesignSystemToken {
        id: "color.primary".into(),
        key: "".into(),
        category: TokenCategory::Color,
        aliases: vec![],
    }];

    let err = token_index(&tokens).expect_err("empty key must fail");
    assert!(matches!(
        err,
        wax_lang_api::TokenRegistryError::EmptyTokenField { field: "key" }
    ));
}

#[test]
fn token_index_rejects_duplicate_match_keys() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "theme.colors.primary",
                "category": "color"
            },
            {
                "id": "color.secondary",
                "key": "theme.colors.primary",
                "category": "color"
            }
        ]
    });
    let tokens = parse_registry_tokens(&value).unwrap();

    let err = token_index(&tokens).expect_err("duplicate match keys must fail");
    assert!(matches!(
        err,
        wax_lang_api::TokenRegistryError::DuplicateMatchKey { .. }
    ));
}
