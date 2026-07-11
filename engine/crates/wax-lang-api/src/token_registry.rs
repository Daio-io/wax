//! Shared token registry parsing and exact token reference matching.

use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use wax_contract::{DesignSystemToken, SourceLocation, TokenCategory, TokenSite};

/// Errors returned while parsing registry token definitions.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TokenRegistryError {
    /// `tokens` exists but is not an array.
    #[error("tokens must be an array")]
    TokensNotArray,
    /// A token field is missing or invalid.
    #[error("tokens[{index}].{field} {reason}")]
    InvalidTokenField {
        /// Token array index.
        index: usize,
        /// Invalid field name.
        field: &'static str,
        /// Human-readable reason.
        reason: String,
    },
    /// Two token entries declare the same id.
    #[error("duplicate token id {id}")]
    DuplicateTokenId {
        /// Duplicate token id.
        id: String,
    },
    /// The same key or alias points at two different token ids.
    #[error("duplicate token match key {key}")]
    DuplicateMatchKey {
        /// Duplicate key or alias.
        key: String,
    },
    /// A token field is empty.
    #[error("token {field} must not be empty")]
    EmptyTokenField {
        /// Invalid field name.
        field: &'static str,
    },
}

/// Exact token reference match found in source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMatch {
    /// Matched token id.
    pub token_id: String,
    /// Exact matched key or alias.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
}

/// Lookup table for exact token key and alias matching.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryTokenIndex {
    /// Tokens in deterministic registry order.
    pub tokens: Vec<DesignSystemToken>,
    /// Key or alias to token match.
    pub matches: BTreeMap<String, TokenMatch>,
}

/// Parses optional `tokens` from a registry JSON value.
pub fn parse_registry_tokens(
    value: &serde_json::Value,
) -> Result<Vec<DesignSystemToken>, TokenRegistryError> {
    let Some(tokens_value) = value.get("tokens") else {
        return Ok(Vec::new());
    };
    let tokens_array = tokens_value
        .as_array()
        .ok_or(TokenRegistryError::TokensNotArray)?;
    let mut tokens = Vec::with_capacity(tokens_array.len());
    let mut seen_ids = BTreeSet::new();
    for (index, token) in tokens_array.iter().enumerate() {
        let id = required_non_empty_string(token, index, "id")?;
        if !seen_ids.insert(id.to_owned()) {
            return Err(TokenRegistryError::DuplicateTokenId { id: id.to_owned() });
        }
        let key = required_non_empty_string(token, index, "key")?;
        let category = parse_category(required_non_empty_string(token, index, "category")?)
            .map_err(|reason| TokenRegistryError::InvalidTokenField {
                index,
                field: "category",
                reason,
            })?;
        let aliases = parse_aliases(token, index)?;
        tokens.push(DesignSystemToken {
            id: id.to_owned(),
            key: key.to_owned(),
            category,
            aliases,
        });
    }
    tokens.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(tokens)
}

/// Builds an exact key and alias lookup index.
pub fn token_index(tokens: &[DesignSystemToken]) -> Result<RegistryTokenIndex, TokenRegistryError> {
    let mut seen_ids = BTreeSet::new();
    let mut matches = BTreeMap::new();
    for token in tokens {
        if token.id.is_empty() {
            return Err(TokenRegistryError::EmptyTokenField { field: "id" });
        }
        if !seen_ids.insert(token.id.clone()) {
            return Err(TokenRegistryError::DuplicateTokenId {
                id: token.id.clone(),
            });
        }
        if token.key.is_empty() {
            return Err(TokenRegistryError::EmptyTokenField { field: "key" });
        }
        for alias in &token.aliases {
            if alias.is_empty() {
                return Err(TokenRegistryError::EmptyTokenField { field: "aliases" });
            }
        }
        insert_match(&mut matches, token, &token.key)?;
        for alias in &token.aliases {
            insert_match(&mut matches, token, alias)?;
        }
    }
    Ok(RegistryTokenIndex {
        tokens: tokens.to_vec(),
        matches,
    })
}

/// Finds exact token key and alias references in source text.
///
/// Used by the basic pack only. Matching is naive substring search per line;
/// false positives inside longer identifiers are a known v1 limitation.
pub fn find_token_matches(
    source: &str,
    file: &str,
    index: &RegistryTokenIndex,
    id_prefix: &str,
) -> Vec<TokenSite> {
    let mut keys: Vec<&String> = index.matches.keys().collect();
    keys.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

    let mut sites = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let line_number = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
        let mut search_from = 0;
        while search_from < line.len() {
            let Some((key, token_match)) = longest_match_at(line, search_from, &keys, index) else {
                search_from += 1;
                continue;
            };
            let column = u32::try_from(search_from + 1).unwrap_or(u32::MAX);
            sites.push(TokenSite {
                id: format!(
                    "{id_prefix}:{file}:{line_number}:{column}:{}",
                    token_match.token_id
                ),
                location: SourceLocation {
                    file: file.to_owned(),
                    line: line_number,
                    column: Some(column),
                },
                token_id: token_match.token_id.clone(),
                key: key.clone(),
                category: token_match.category,
                parent: None,
            });
            search_from += key.len();
        }
    }
    sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
            .then(left.token_id.cmp(&right.token_id))
    });
    sites
}

fn longest_match_at<'a>(
    line: &str,
    start: usize,
    keys: &[&'a String],
    index: &'a RegistryTokenIndex,
) -> Option<(&'a String, &'a TokenMatch)> {
    let suffix = &line[start..];
    for key in keys {
        if suffix.starts_with(key.as_str()) {
            return index
                .matches
                .get(key.as_str())
                .map(|token_match| (*key, token_match));
        }
    }
    None
}

fn required_non_empty_string<'a>(
    token: &'a serde_json::Value,
    index: usize,
    field: &'static str,
) -> Result<&'a str, TokenRegistryError> {
    let value = token
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must be a string".to_owned(),
        })?;
    if value.is_empty() {
        return Err(TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(value)
}

fn parse_aliases(
    token: &serde_json::Value,
    index: usize,
) -> Result<Vec<String>, TokenRegistryError> {
    let Some(aliases_value) = token.get("aliases") else {
        return Ok(Vec::new());
    };
    let aliases =
        aliases_value
            .as_array()
            .ok_or_else(|| TokenRegistryError::InvalidTokenField {
                index,
                field: "aliases",
                reason: "must be an array".to_owned(),
            })?;
    let mut parsed = Vec::with_capacity(aliases.len());
    for (alias_index, alias) in aliases.iter().enumerate() {
        let alias = alias
            .as_str()
            .ok_or_else(|| TokenRegistryError::InvalidTokenField {
                index,
                field: "aliases",
                reason: format!("aliases[{alias_index}] must be a string"),
            })?;
        if alias.is_empty() {
            return Err(TokenRegistryError::InvalidTokenField {
                index,
                field: "aliases",
                reason: format!("aliases[{alias_index}] must not be empty"),
            });
        }
        parsed.push(alias.to_owned());
    }
    Ok(parsed)
}

fn parse_category(value: &str) -> Result<TokenCategory, String> {
    match value {
        "color" => Ok(TokenCategory::Color),
        "spacing" => Ok(TokenCategory::Spacing),
        "typography" => Ok(TokenCategory::Typography),
        "radius" => Ok(TokenCategory::Radius),
        "elevation" => Ok(TokenCategory::Elevation),
        "unknown" => Ok(TokenCategory::Unknown),
        other => Err(format!("unsupported category {other:?}")),
    }
}

fn insert_match(
    matches: &mut BTreeMap<String, TokenMatch>,
    token: &DesignSystemToken,
    key: &str,
) -> Result<(), TokenRegistryError> {
    if key.is_empty() {
        return Err(TokenRegistryError::EmptyTokenField { field: "key" });
    }
    if matches.contains_key(key) {
        return Err(TokenRegistryError::DuplicateMatchKey {
            key: key.to_owned(),
        });
    }
    matches.insert(
        key.to_owned(),
        TokenMatch {
            token_id: token.id.clone(),
            key: key.to_owned(),
            category: token.category,
        },
    );
    Ok(())
}
