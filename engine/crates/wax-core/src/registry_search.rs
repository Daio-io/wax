//! In-memory search and exact lookup over validated registry snapshots.

use serde::Serialize;
use serde_json::{Map, Value};
use wax_contract::{
    LanguageId, REGISTRY_LOOKUP_SCHEMA_VERSION, RegistryComponent, RegistryDetailResponse,
    RegistryEntryKind, RegistryLookupError, RegistryLookupErrorCode, RegistryMetadata,
    RegistrySearchFilters, RegistrySearchResponse, RegistrySearchResult, RegistryToken,
};

use crate::registry_cache::RegistrySnapshot;

const DEFAULT_RESULT_LIMIT: u32 = 20;
const MAX_SUMMARY_CHARS: usize = 240;
const FUZZY_MATCH_THRESHOLD: f64 = 0.72;

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
///
/// # Errors
///
/// Returns [`RegistryLookupErrorCode::InvalidArgument`] when `query` is empty or
/// the requested limit is zero.
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
    if normalized_query.compact.is_empty() {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::InvalidArgument,
            "registry search query must contain an identifier or value",
        ));
    }

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
    let results = matches
        .into_iter()
        .take(limit as usize)
        .map(|ranked| ranked.result)
        .collect();

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
    let mut score = best_structured_score(
        [&component.id, &component.symbol]
            .into_iter()
            .map(String::as_str),
        query,
        0,
    );
    score = min_score(
        score,
        best_structured_score(component.aliases.iter().map(String::as_str), query, 1),
    );
    score = min_score(
        score,
        best_structured_score(component.package.iter().map(String::as_str), query, 2),
    );
    min_score(
        score,
        descriptive_score(component.metadata.as_ref(), component, query),
    )
}

fn token_score(token: &RegistryToken, query: &Normalized) -> Option<u16> {
    let mut score = best_structured_score(
        [&token.id, &token.key].into_iter().map(String::as_str),
        query,
        0,
    );
    score = min_score(
        score,
        best_structured_score(token.aliases.iter().map(String::as_str), query, 1),
    );
    score = min_score(
        score,
        best_structured_score(token.value.iter().map(String::as_str), query, 2),
    );
    let category = token_category_name(token);
    score = min_score(
        score,
        best_structured_score([category].into_iter(), query, 2),
    );
    min_score(
        score,
        descriptive_score(token.metadata.as_ref(), token, query),
    )
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

fn descriptive_score<T: Serialize>(
    metadata: Option<&RegistryMetadata>,
    entry: &T,
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
        .map(|score| 100 + score);
    let Ok(value) = serde_json::to_value(entry) else {
        return metadata_score;
    };
    let summary_score = named_json_score(&value, &["summary"], query, 100);
    let attachment_score = named_json_score(&value, &["documentation", "content"], query, 120);
    min_score(min_score(metadata_score, summary_score), attachment_score)
}

fn named_json_score(value: &Value, names: &[&str], query: &Normalized, weight: u16) -> Option<u16> {
    let mut fields = Vec::new();
    collect_named_strings(value, names, &mut fields);
    fields
        .into_iter()
        .filter_map(|field| match_score(field, query))
        .min()
        .map(|score| weight + score)
}

fn collect_named_strings<'a>(value: &'a Value, names: &[&str], output: &mut Vec<&'a str>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if names.contains(&key.as_str()) {
                    match value {
                        Value::String(text) => output.push(text),
                        Value::Array(values) => {
                            output.extend(values.iter().filter_map(Value::as_str))
                        }
                        _ => {}
                    }
                }
                collect_named_strings(value, names, output);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_named_strings(value, names, output);
            }
        }
        _ => {}
    }
}

fn match_score(field: &str, query: &Normalized) -> Option<u16> {
    let field = Normalized::new(field);
    if field.compact.is_empty() {
        return None;
    }
    if field.spaced == query.spaced || field.compact == query.compact {
        return Some(0);
    }
    if field.spaced.starts_with(&query.spaced) || field.compact.starts_with(&query.compact) {
        return Some(20);
    }
    if field.spaced.contains(&query.spaced) || field.compact.contains(&query.compact) {
        return Some(40);
    }
    let similarity = strsim::normalized_levenshtein(&field.compact, &query.compact);
    (similarity >= FUZZY_MATCH_THRESHOLD).then(|| 60 + ((1.0 - similarity) * 100.0).round() as u16)
}

fn component_result(
    snapshot: &RegistrySnapshot,
    component: &RegistryComponent,
) -> RegistrySearchResult {
    RegistrySearchResult {
        snapshot: snapshot.reference.clone(),
        kind: RegistryEntryKind::Component,
        id: component.id.clone(),
        label: component.symbol.clone(),
        summary: entry_summary(component.metadata.as_ref(), component),
        extraction_status: component
            .extracted
            .as_ref()
            .map(|extracted| extracted.status),
        extra: Map::new(),
    }
}

fn token_result(snapshot: &RegistrySnapshot, token: &RegistryToken) -> RegistrySearchResult {
    RegistrySearchResult {
        snapshot: snapshot.reference.clone(),
        kind: RegistryEntryKind::Token,
        id: token.id.clone(),
        label: token.key.clone(),
        summary: entry_summary(token.metadata.as_ref(), token),
        extraction_status: token.extracted.as_ref().map(|extracted| extracted.status),
        extra: Map::new(),
    }
}

fn entry_summary<T: Serialize>(metadata: Option<&RegistryMetadata>, entry: &T) -> String {
    let explicit_summary = serde_json::to_value(entry).ok().and_then(|value| {
        value
            .as_object()
            .and_then(|object| object.get("summary"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let text = explicit_summary
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
