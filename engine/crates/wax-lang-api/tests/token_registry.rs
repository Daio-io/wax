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
