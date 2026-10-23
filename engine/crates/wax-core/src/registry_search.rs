//! In-memory search and exact lookup over validated registry snapshots.

use serde::Serialize;
use serde_json::{Map, Value};
use wax_contract::{
    LanguageId, REGISTRY_LOOKUP_SCHEMA_VERSION, RegistryComponent, RegistryDetailResponse,
    RegistryEntryKind, RegistryExtracted, RegistryLookupError, RegistryLookupErrorCode,
    RegistryMetadata, RegistrySearchFilters, RegistrySearchResponse, RegistrySearchResult,
    RegistryToken,
};

use crate::registry_cache::RegistrySnapshot;

const DEFAULT_RESULT_LIMIT: u32 = 20;
const MAX_SUMMARY_CHARS: usize = 240;
const FUZZY_MATCH_THRESHOLD: f64 = 0.72;
const SCORE_EXACT: u16 = 0;
const SCORE_PREFIX: u16 = 20;
const SCORE_CONTAINS: u16 = 40;
const SCORE_FUZZY_BASE: u16 = 60;
const SCORE_DESCRIPTIVE: u16 = 100;
const SCORE_DOCS: u16 = 120;

/// Filters and bounds applied to an in-memory registry search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistrySearchOptions {
    /// Restricts results to components or tokens.
    pub kind: Option<RegistryEntryKind>,
    /// Restricts results to snapshots for one language.
    pub language: Option<LanguageId>,
    /// Maximum results across every selected snapshot. Defaults to 20.
    pub limit: Option<u32>,
}

/// Searches validated registry snapshots without performing I/O or pack execution.
///
/// Results are ranked by match quality and then by stable snapshot and entry identity.
/// Separator-only or punctuation-only queries normalize to an empty identifier and succeed
/// with an empty result set.
///
/// # Errors
///
/// Returns [`RegistryLookupErrorCode::InvalidArgument`] when `query` is empty (after trim)
/// or the requested limit is zero.
pub fn search_registry_entries(
    snapshots: &[RegistrySnapshot],
    query: &str,
    options: &RegistrySearchOptions,
) -> Result<RegistrySearchResponse, RegistryLookupError> {
    if query.trim().is_empty() {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::InvalidArgument,
            "registry search query must not be empty",
        ));
    }

    let limit = options.limit.unwrap_or(DEFAULT_RESULT_LIMIT);
    if limit == 0 {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::InvalidArgument,
            "registry search limit must be positive",
        ));
    }

    let normalized_query = Normalized::new(query);
    let mut selected_snapshots = snapshots
        .iter()
        .filter(|snapshot| {
            options
                .language
                .as_ref()
                .is_none_or(|language| snapshot.reference.language == *language)
        })
        .collect::<Vec<_>>();
    selected_snapshots.sort_by(|left, right| snapshot_key(left).cmp(&snapshot_key(right)));

    let results = if normalized_query.compact.is_empty() {
        Vec::new()
    } else {
        let mut matches = Vec::new();
        for snapshot in &selected_snapshots {
            if options.kind != Some(RegistryEntryKind::Token) {
                for component in &snapshot.document.components {
                    if let Some(score) = component_score(component, &normalized_query) {
                        matches.push(RankedResult {
                            score,
                            result: component_result(snapshot, component),
                        });
                    }
                }
            }
            if options.kind != Some(RegistryEntryKind::Component) {
                for token in &snapshot.document.tokens {
                    if let Some(score) = token_score(token, &normalized_query) {
                        matches.push(RankedResult {
                            score,
                            result: token_result(snapshot, token),
                        });
                    }
                }
            }
        }

        matches.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| result_key(&left.result).cmp(&result_key(&right.result)))
        });
        matches
            .into_iter()
            .take(limit as usize)
            .map(|ranked| ranked.result)
            .collect()
    };

    Ok(RegistrySearchResponse {
        schema_version: REGISTRY_LOOKUP_SCHEMA_VERSION,
        query: query.to_owned(),
        filters: RegistrySearchFilters {
            kind: options.kind,
            language: options.language.clone(),
            limit,
            extra: Map::new(),
        },
        snapshots: selected_snapshots
            .into_iter()
            .map(|snapshot| snapshot.reference.clone())
            .collect(),
        results,
        diagnostics: Vec::new(),
        extra: Map::new(),
    })
}

/// Retrieves one complete registry entry by its exact kind and id.
///
/// # Errors
///
/// Returns [`RegistryLookupErrorCode::EntryNotFound`] when no entry has the exact
/// id, [`RegistryLookupErrorCode::AmbiguousEntry`] when multiple snapshots do,
/// or [`RegistryLookupErrorCode::InvalidRegistry`] if serialization fails.
pub fn lookup_registry_entry(
    snapshots: &[RegistrySnapshot],
    kind: RegistryEntryKind,
    id: &str,
) -> Result<RegistryDetailResponse, RegistryLookupError> {
    let mut matches = snapshots
        .iter()
        .flat_map(|snapshot| exact_matches(snapshot, kind, id))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| snapshot_key(left.0).cmp(&snapshot_key(right.0)));

    match matches.as_slice() {
        [] => Err(RegistryLookupError::new(
            RegistryLookupErrorCode::EntryNotFound,
            format!("no {kind:?} registry entry has exact id `{id}`"),
        )),
        [(snapshot, entry)] => Ok(RegistryDetailResponse {
            schema_version: REGISTRY_LOOKUP_SCHEMA_VERSION,
            snapshot: snapshot.reference.clone(),
            kind,
            entry: entry_to_value(entry)?,
            extra: Map::new(),
        }),
        _ => Err(RegistryLookupError::new(
            RegistryLookupErrorCode::AmbiguousEntry,
            format!("multiple {kind:?} registry entries have exact id `{id}`"),
        )),
    }
}

#[derive(Debug)]
struct RankedResult {
    score: u16,
    result: RegistrySearchResult,
}

#[derive(Debug)]
enum EntryRef<'a> {
    Component(&'a RegistryComponent),
    Token(&'a RegistryToken),
}

fn exact_matches<'a>(
    snapshot: &'a RegistrySnapshot,
    kind: RegistryEntryKind,
    id: &str,
) -> Vec<(&'a RegistrySnapshot, EntryRef<'a>)> {
    match kind {
        RegistryEntryKind::Component => snapshot
            .document
            .components
            .iter()
            .filter(|entry| entry.id == id)
            .map(|entry| (snapshot, EntryRef::Component(entry)))
            .collect(),
        RegistryEntryKind::Token => snapshot
            .document
            .tokens
            .iter()
            .filter(|entry| entry.id == id)
            .map(|entry| (snapshot, EntryRef::Token(entry)))
            .collect(),
    }
}

fn entry_to_value(entry: &EntryRef<'_>) -> Result<Value, RegistryLookupError> {
    match entry {
        EntryRef::Component(component) => serialize_entry(component),
        EntryRef::Token(token) => serialize_entry(token),
    }
}

fn serialize_entry(entry: &impl Serialize) -> Result<Value, RegistryLookupError> {
    serde_json::to_value(entry).map_err(|error| {
        RegistryLookupError::new(
            RegistryLookupErrorCode::InvalidRegistry,
            format!("failed to serialize validated registry entry: {error}"),
        )
    })
}

fn component_score(component: &RegistryComponent, query: &Normalized) -> Option<u16> {
    entry_score(
        [&component.id, &component.symbol]
            .into_iter()
            .map(String::as_str),
        component.aliases.iter().map(String::as_str),
        component.package.iter().map(String::as_str),
        component.metadata.as_ref(),
        component.extracted.as_ref(),
        &component.extra,
        query,
    )
}

fn token_score(token: &RegistryToken, query: &Normalized) -> Option<u16> {
    let category = token_category_name(token);
    entry_score(
        [&token.id, &token.key].into_iter().map(String::as_str),
        token.aliases.iter().map(String::as_str),
        token
            .value
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(category)),
        token.metadata.as_ref(),
        token.extracted.as_ref(),
        &token.extra,
        query,
    )
}

fn entry_score<'a>(
    primary: impl Iterator<Item = &'a str>,
    aliases: impl Iterator<Item = &'a str>,
    secondary: impl Iterator<Item = &'a str>,
    metadata: Option<&RegistryMetadata>,
    extracted: Option<&RegistryExtracted>,
    extra: &Map<String, Value>,
    query: &Normalized,
) -> Option<u16> {
    let mut score = best_structured_score(primary, query, 0);
    score = min_score(score, best_structured_score(aliases, query, 1));
    score = min_score(score, best_structured_score(secondary, query, 2));
    min_score(score, descriptive_score(metadata, extracted, extra, query))
}

fn best_structured_score<'a>(
    fields: impl Iterator<Item = &'a str>,
    query: &Normalized,
    field_priority: u16,
) -> Option<u16> {
    fields
        .filter_map(|field| match_score(field, query).map(|score| score + field_priority))
        .min()
}

fn descriptive_score(
    metadata: Option<&RegistryMetadata>,
    extracted: Option<&RegistryExtracted>,
    extra: &Map<String, Value>,
    query: &Normalized,
) -> Option<u16> {
    let metadata_score = metadata
        .into_iter()
        .flat_map(|metadata| {
            metadata
                .tags
                .iter()
                .map(String::as_str)
                .chain(metadata.description.iter().map(String::as_str))
        })
        .filter_map(|field| match_score(field, query))
        .min()
        .map(|score| SCORE_DESCRIPTIVE + score);
    let summary_score = extra
        .get("summary")
        .and_then(Value::as_str)
        .and_then(|summary| match_score(summary, query))
        .map(|score| SCORE_DESCRIPTIVE + score);
    let documentation_score = extracted
        .and_then(|extracted| extracted.documentation.as_deref())
        .and_then(|documentation| match_score(documentation, query))
        .map(|score| SCORE_DOCS + score);
    let attachment_score = extracted
        .into_iter()
        .flat_map(|extracted| extracted.attachments.iter())
        .filter_map(|attachment| match_score(&attachment.content, query))
        .min()
        .map(|score| SCORE_DOCS + score);
    min_score(
        min_score(
            min_score(metadata_score, summary_score),
            documentation_score,
        ),
        attachment_score,
    )
}

fn match_score(field: &str, query: &Normalized) -> Option<u16> {
    let field = Normalized::new(field);
    if field.compact.is_empty() {
        return None;
    }
    if field.spaced == query.spaced || field.compact == query.compact {
        return Some(SCORE_EXACT);
    }
    if field.spaced.starts_with(&query.spaced) || field.compact.starts_with(&query.compact) {
        return Some(SCORE_PREFIX);
    }
    if field.spaced.contains(&query.spaced) || field.compact.contains(&query.compact) {
        return Some(SCORE_CONTAINS);
    }
    let similarity = normalized_levenshtein(&field.compact, &query.compact);
    (similarity >= FUZZY_MATCH_THRESHOLD)
        .then(|| SCORE_FUZZY_BASE + ((1.0 - similarity) * 100.0).round() as u16)
}

fn component_result(
    snapshot: &RegistrySnapshot,
    component: &RegistryComponent,
) -> RegistrySearchResult {
    search_result(
        snapshot,
        RegistryEntryKind::Component,
        component.id.clone(),
        component.symbol.clone(),
        component.metadata.as_ref(),
        component.extracted.as_ref(),
        &component.extra,
    )
}

fn token_result(snapshot: &RegistrySnapshot, token: &RegistryToken) -> RegistrySearchResult {
    search_result(
        snapshot,
        RegistryEntryKind::Token,
        token.id.clone(),
        token.key.clone(),
        token.metadata.as_ref(),
        token.extracted.as_ref(),
        &token.extra,
    )
}

fn search_result(
    snapshot: &RegistrySnapshot,
    kind: RegistryEntryKind,
    id: String,
    label: String,
    metadata: Option<&RegistryMetadata>,
    extracted: Option<&RegistryExtracted>,
    extra: &Map<String, Value>,
) -> RegistrySearchResult {
    RegistrySearchResult {
        snapshot: snapshot.reference.clone(),
        kind,
        id,
        label,
        summary: entry_summary(metadata, extra),
        extraction_status: extracted.map(|extracted| extracted.status),
        extra: Map::new(),
    }
}

fn entry_summary(metadata: Option<&RegistryMetadata>, extra: &Map<String, Value>) -> String {
    let text = extra
        .get("summary")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| metadata.and_then(|metadata| metadata.description.clone()))
        .unwrap_or_default();
    text.chars().take(MAX_SUMMARY_CHARS).collect()
}

fn token_category_name(token: &RegistryToken) -> &'static str {
    use wax_contract::TokenCategory;

    match token.category {
        TokenCategory::Color => "color",
        TokenCategory::Spacing => "spacing",
        TokenCategory::Typography => "typography",
        TokenCategory::Radius => "radius",
        TokenCategory::Elevation => "elevation",
        TokenCategory::Unknown => "unknown",
    }
}

fn min_score(left: Option<u16>, right: Option<u16>) -> Option<u16> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

fn snapshot_key(snapshot: &RegistrySnapshot) -> (&str, &str, &str) {
    (
        &snapshot.reference.source,
        snapshot.reference.language.as_str(),
        &snapshot.reference.digest,
    )
}

fn result_key(result: &RegistrySearchResult) -> (&str, &str, RegistryEntryKind, &str) {
    (
        &result.snapshot.source,
        result.snapshot.language.as_str(),
        result.kind,
        &result.id,
    )
}

fn normalized_levenshtein(left: &str, right: &str) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let max_len = left_len.max(right_len);
    if max_len == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(left, right) as f64 / max_len as f64)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let left_len = left_chars.len();
    let right_len = right_chars.len();
    if left_len == 0 {
        return right_len;
    }
    if right_len == 0 {
        return left_len;
    }

    let mut previous = (0..=right_len).collect::<Vec<_>>();
    let mut current = vec![0; right_len + 1];
    for (i, left_char) in left_chars.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = usize::from(left_char != right_char);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_len]
}

#[derive(Debug)]
struct Normalized {
    spaced: String,
    compact: String,
}

impl Normalized {
    fn new(input: &str) -> Self {
        let characters = input.chars().collect::<Vec<_>>();
        let mut spaced = String::new();
        let mut previous_was_separator = true;

        for (index, character) in characters.iter().copied().enumerate() {
            if !character.is_alphanumeric() {
                if !spaced.is_empty() {
                    previous_was_separator = true;
                }
                continue;
            }

            let previous = index.checked_sub(1).and_then(|index| characters.get(index));
            let next = characters.get(index + 1);
            let identifier_boundary = character.is_uppercase()
                && (previous
                    .is_some_and(|previous| previous.is_lowercase() || previous.is_numeric())
                    || (previous.is_some_and(|previous| previous.is_uppercase())
                        && next.is_some_and(|next| next.is_lowercase())));
            if !spaced.is_empty() && (previous_was_separator || identifier_boundary) {
                spaced.push(' ');
            }
            spaced.extend(character.to_lowercase());
            previous_was_separator = false;
        }

        let compact = spaced
            .chars()
            .filter(|character| *character != ' ')
            .collect();
        Self { spaced, compact }
    }
}
