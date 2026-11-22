//! Shared token registry parsing and exact token reference matching.

use std::borrow::Cow;
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

/// Lookup table for package-scoped and legacy exact token references.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryTokenIndex {
    /// Tokens in deterministic registry order.
    pub tokens: Vec<DesignSystemToken>,
    /// Legacy key or alias to token match.
    pub matches: BTreeMap<String, TokenMatch>,
    scoped: BTreeMap<String, BTreeMap<String, TokenMatch>>,
    packages: BTreeSet<String>,
}

impl RegistryTokenIndex {
    /// Returns whether any tokens require import evidence.
    pub fn has_scoped_tokens(&self) -> bool {
        !self.scoped.is_empty()
    }

    /// Returns whether a name is registered in any package.
    pub fn has_scoped_name(&self, key: &str) -> bool {
        self.scoped.contains_key(key)
    }

    /// Splits a static path at its longest registered package prefix.
    pub fn package_qualified_path<'a>(&self, path: &'a str) -> Option<(&'a str, &'a str)> {
        path.rmatch_indices('.').find_map(|(index, _)| {
            let package = &path[..index];
            self.packages
                .contains(package)
                .then_some((package, &path[index + 1..]))
        })
    }

    /// Matches a registered name using its observed package and exported path.
    ///
    /// The source is the original expression. The path is its static spelling,
    /// if available. A scoped name suppresses legacy matching when its origin
    /// cannot be verified. Legacy entries always match the original expression.
    pub fn match_reference(
        &self,
        source: &str,
        path: Option<&str>,
        origin: Option<(&str, &str)>,
    ) -> Option<&TokenMatch> {
        if let Some((package, key)) = origin
            && let Some(packages) = self.scoped.get(key)
        {
            return packages.get(package);
        }
        if self.scoped.contains_key(source)
            || path.is_some_and(|path| self.scoped.contains_key(path))
        {
            return None;
        }
        self.matches.get(source)
    }
}

/// Normalizes whitespace around dots in a static identifier path.
pub fn static_dotted_token_path(text: &str) -> Option<Cow<'_, str>> {
    let segments = text.split('.').map(str::trim);
    if !segments.clone().all(|segment| {
        let mut chars = segment.chars();
        chars
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
            && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
    }) {
        return None;
    }
    if text.chars().any(char::is_whitespace) {
        Some(Cow::Owned(segments.collect::<Vec<_>>().join(".")))
    } else {
        Some(Cow::Borrowed(text))
    }
}

/// Parses and indexes registry tokens, preserving package scopes and all scan facts.
///
/// # Errors
///
/// Returns a token registry error for invalid fields, duplicate ids, or
/// conflicting keys or aliases within a package or the legacy partition.
pub fn registry_token_index(
    value: &serde_json::Value,
) -> Result<RegistryTokenIndex, TokenRegistryError> {
    let entries = parse_token_entries(value)?;
    build_index(
        entries
            .iter()
            .map(|(token, package)| (token, package.as_deref())),
    )
}

/// Parses optional `tokens` from a registry JSON value.
///
/// # Errors
///
/// Returns [`TokenRegistryError::TokensNotArray`] when `tokens` is not an array,
/// [`TokenRegistryError::InvalidTokenField`] for missing or malformed fields,
/// or [`TokenRegistryError::DuplicateTokenId`] for repeated token ids.
pub fn parse_registry_tokens(
    value: &serde_json::Value,
) -> Result<Vec<DesignSystemToken>, TokenRegistryError> {
    Ok(parse_token_entries(value)?
        .into_iter()
        .map(|(token, _)| token)
        .collect())
}

fn parse_token_entries(
    value: &serde_json::Value,
) -> Result<Vec<(DesignSystemToken, Option<String>)>, TokenRegistryError> {
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
        let category = parse_category(index, required_non_empty_string(token, index, "category")?)?;
        let aliases = parse_aliases(token, index)?;
        let value = optional_non_empty_string(token, index, "value")?;
        let package = optional_non_empty_string(token, index, "package")?;
        tokens.push((
            DesignSystemToken {
                id: id.to_owned(),
                key: key.to_owned(),
                category,
                aliases,
                value,
            },
            package,
        ));
    }
    tokens.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
    Ok(tokens)
}

/// Builds an exact key and alias lookup index.
///
/// # Errors
///
/// Returns [`TokenRegistryError::EmptyTokenField`] for an empty id, key, or
/// alias, [`TokenRegistryError::DuplicateTokenId`] for repeated ids, or
/// [`TokenRegistryError::DuplicateMatchKey`] when one key or alias maps to
/// different token ids.
pub fn token_index(tokens: &[DesignSystemToken]) -> Result<RegistryTokenIndex, TokenRegistryError> {
    build_index(tokens.iter().map(|token| (token, None)))
}

fn build_index<'a>(
    entries: impl IntoIterator<Item = (&'a DesignSystemToken, Option<&'a str>)>,
) -> Result<RegistryTokenIndex, TokenRegistryError> {
    let mut seen_ids = BTreeSet::new();
    let mut index = RegistryTokenIndex::default();
    for (token, package) in entries {
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
        if let Some(package) = package {
            index.packages.insert(package.to_owned());
        }
        for key in std::iter::once(&token.key).chain(&token.aliases) {
            if let Some(package) = package {
                insert_match(
                    index.scoped.entry(key.clone()).or_default(),
                    token,
                    package,
                    key,
                )?;
            } else {
                insert_match(&mut index.matches, token, key, key)?;
            }
        }
        index.tokens.push(token.clone());
    }
    Ok(index)
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
    let mut keys: Vec<&String> = index
        .matches
        .keys()
        .filter(|key| !index.scoped.contains_key(key.as_str()))
        .collect();
    keys.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

    let mut sites = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let line_number = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
        let mut search_from = 0;
        while search_from < line.len() {
            let Some((key, token_match)) = longest_match_at(line, search_from, &keys, index) else {
                // Advance by one Unicode scalar so slicing stays on a char boundary.
                search_from += line[search_from..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
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
                    root_group: None,
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

fn optional_non_empty_string(
    token: &serde_json::Value,
    index: usize,
    field: &'static str,
) -> Result<Option<String>, TokenRegistryError> {
    let Some(value) = token.get(field) else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must be a string".to_owned(),
        });
    };
    if text.is_empty() {
        return Err(TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(Some(text.to_owned()))
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

fn parse_category(index: usize, value: &str) -> Result<TokenCategory, TokenRegistryError> {
    match value {
        "color" => Ok(TokenCategory::Color),
        "spacing" => Ok(TokenCategory::Spacing),
        "typography" => Ok(TokenCategory::Typography),
        "radius" => Ok(TokenCategory::Radius),
        "elevation" => Ok(TokenCategory::Elevation),
        "unknown" => Ok(TokenCategory::Unknown),
        other => Err(TokenRegistryError::InvalidTokenField {
            index,
            field: "category",
            reason: format!("unsupported category {other:?}"),
        }),
    }
}

fn insert_match(
    matches: &mut BTreeMap<String, TokenMatch>,
    token: &DesignSystemToken,
    lookup: &str,
    key: &str,
) -> Result<(), TokenRegistryError> {
    if key.is_empty() {
        return Err(TokenRegistryError::EmptyTokenField { field: "key" });
    }
    if matches
        .get(lookup)
        .is_some_and(|matched| matched.token_id != token.id)
    {
        return Err(TokenRegistryError::DuplicateMatchKey {
            key: key.to_owned(),
        });
    }
    matches.insert(
        lookup.to_owned(),
        TokenMatch {
            token_id: token.id.clone(),
            key: key.to_owned(),
            category: token.category,
        },
    );
    Ok(())
}
