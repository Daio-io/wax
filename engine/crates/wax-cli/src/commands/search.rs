//! Registry search and exact entry detail commands.

use super::state_path::resolve_state_path;
use serde::Serialize;
use serde_json::Map;
use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use wax_contract::{
    LanguageId, RegistryDetailResponse, RegistryEntryKind, RegistryExtractionStatus,
    RegistryLookupError, RegistryLookupErrorCode, RegistrySearchResponse, RegistrySnapshotRef,
};
use wax_core::paths::PathsError;
use wax_core::registry_lookup::{RegistryLookupOptions, resolve_lookup_snapshots};
use wax_core::registry_search::{
    RegistrySearchOptions, lookup_registry_entry, search_registry_entries,
};

use crate::cli::LookupStdoutFormat;

/// Shared options for search and detail lookup commands.
#[derive(Debug, Clone)]
pub struct LookupCommandOptions {
    /// Repository or standalone directory establishing lookup context.
    pub repo_root: PathBuf,
    /// Remembered id, app/publisher id, or explicit path/URL.
    pub design_system: Option<String>,
    /// Optional language filter.
    pub language: Option<LanguageId>,
    /// Optional Git tag or commit for an explicit Git selector.
    pub tag: Option<String>,
    /// Refuse source I/O and resolve only already cached bytes.
    pub offline: bool,
    /// Stdout format.
    pub format: LookupStdoutFormat,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Options for `wax search`.
#[derive(Debug, Clone)]
pub struct SearchCommandOptions {
    /// Shared lookup selection options.
    pub lookup: LookupCommandOptions,
    /// Fuzzy search query.
    pub query: String,
    /// Optional entry-kind filter.
    pub kind: Option<RegistryEntryKind>,
    /// Maximum results across every selected snapshot.
    pub limit: u32,
}

/// Options for `wax component` and `wax token`.
#[derive(Debug, Clone)]
pub struct DetailCommandOptions {
    /// Shared lookup selection options.
    pub lookup: LookupCommandOptions,
    /// Exact entry kind.
    pub kind: RegistryEntryKind,
    /// Exact entry id.
    pub id: String,
    /// Optional immutable snapshot digest from a prior search result.
    pub snapshot_digest: Option<String>,
}

/// Errors returned by search and detail CLI commands.
#[derive(Debug, Error)]
pub enum SearchCommandError {
    /// Registry lookup or search failed with a stable machine-readable code.
    #[error(transparent)]
    Lookup(#[from] RegistryLookupError),
    /// Global path resolution failed.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Output writing failed.
    #[error("failed to write search command output: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

impl SearchCommandError {
    /// Returns a stable lookup error envelope when one can be emitted as JSON.
    #[must_use]
    pub fn to_lookup_error(&self) -> Option<RegistryLookupError> {
        match self {
            Self::Lookup(error) => Some(error.clone()),
            Self::Paths(error) => Some(paths_to_lookup_error(error)),
            Self::Io { .. } => None,
        }
    }
}

/// Runs `wax search`.
///
/// # Errors
///
/// Returns a typed lookup error for invalid arguments or selected-source failure.
/// An empty result set is success.
pub fn run_search(
    options: SearchCommandOptions,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    let snapshots = resolve_snapshots(&options.lookup, None)?;
    let response = search_registry_entries(
        &snapshots,
        &options.query,
        &RegistrySearchOptions {
            kind: options.kind,
            language: options.lookup.language.clone(),
            limit: Some(options.limit),
        },
    )?;
    write_search_output(options.lookup.format, &response, writer)
}

/// Runs `wax component` or `wax token`.
///
/// # Errors
///
/// Returns a typed lookup error for absence, ambiguity, snapshot mismatch, or
/// selected-source failure.
pub fn run_detail(
    options: DetailCommandOptions,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    let snapshots = resolve_snapshots(&options.lookup, options.snapshot_digest.as_deref())?;
    let response = lookup_registry_entry(&snapshots, options.kind, &options.id)?;
    write_detail_output(options.lookup.format, &response, writer)
}

/// Writes a typed lookup failure for `--format json`.
///
/// # Errors
///
/// Returns [`SearchCommandError::Io`] when stdout cannot be written.
pub fn write_lookup_error_json(
    error: &RegistryLookupError,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    write_json(error, writer)
}

fn resolve_snapshots(
    options: &LookupCommandOptions,
    snapshot_digest: Option<&str>,
) -> Result<Vec<wax_core::registry_cache::RegistrySnapshot>, SearchCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let base = RegistryLookupOptions {
        repo_root: options.repo_root.clone(),
        selector: options.design_system.clone(),
        language: options.language.clone(),
        tag: options.tag.clone(),
        snapshot: None,
        offline: options.offline,
        state_path: Some(state_path),
    };

    let Some(digest) = snapshot_digest else {
        return Ok(resolve_lookup_snapshots(&base)?);
    };

    let current = resolve_lookup_snapshots(&base)?;
    let matching = current
        .iter()
        .filter(|snapshot| snapshot.reference.digest == digest)
        .cloned()
        .collect::<Vec<_>>();
    if !matching.is_empty() {
        return Ok(matching);
    }

    if current.is_empty() {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::SnapshotMismatch,
            format!("requested snapshot digest `{digest}` was not found in the selected sources"),
        )
        .into());
    }

    // Digest is absent from the refreshed selection. Probe each selected source
    // identity so multi-language / multi-source selections do not pin `current[0]`.
    // Prefer offline cache hits first (search-to-detail after refresh), then allow
    // online reproduction when the caller did not request `--offline`.
    let offline_modes: &[bool] = if base.offline {
        &[true]
    } else {
        &[true, false]
    };
    let mut last_error = None;
    for &offline in offline_modes {
        let mut recovered = Vec::new();
        for template in &current {
            if options
                .language
                .as_ref()
                .is_some_and(|language| language != &template.reference.language)
            {
                continue;
            }
            let expected = RegistrySnapshotRef {
                source: template.reference.source.clone(),
                language: template.reference.language.clone(),
                digest: digest.to_owned(),
                system_id: template.reference.system_id.clone(),
                // Historical blob reads do not require the current selection's commit.
                git_commit: None,
                extra: Map::new(),
            };
            match resolve_lookup_snapshots(&RegistryLookupOptions {
                snapshot: Some(expected),
                offline,
                ..base.clone()
            }) {
                Ok(snapshots) if snapshots.iter().all(snapshot_language_matches_document) => {
                    recovered.push(snapshots);
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.code,
                        RegistryLookupErrorCode::SnapshotMismatch
                            | RegistryLookupErrorCode::LockedSnapshotUnavailable
                            | RegistryLookupErrorCode::OfflineCacheMiss
                    ) =>
                {
                    last_error = Some(error);
                }
                Err(error) => return Err(error.into()),
            }
        }
        match recovered.len() {
            1 => return Ok(recovered.remove(0)),
            0 => {}
            _ => {
                return Err(RegistryLookupError::new(
                    RegistryLookupErrorCode::SnapshotMismatch,
                    format!(
                        "requested snapshot digest `{digest}` matched multiple selected sources"
                    ),
                )
                .into());
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| {
            RegistryLookupError::new(
                RegistryLookupErrorCode::SnapshotMismatch,
                format!(
                    "requested snapshot digest `{digest}` was not found in the selected sources"
                ),
            )
        })
        .into())
}

fn snapshot_language_matches_document(
    snapshot: &wax_core::registry_cache::RegistrySnapshot,
) -> bool {
    match snapshot
        .document
        .extra
        .get("language")
        .and_then(|value| value.as_str())
    {
        Some(language) => language == snapshot.reference.language.as_str(),
        None => true,
    }
}

fn write_search_output(
    format: LookupStdoutFormat,
    response: &RegistrySearchResponse,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    match format {
        LookupStdoutFormat::Json => write_json(response, writer),
        LookupStdoutFormat::Text => write_search_text(response, writer),
    }
}

fn write_detail_output(
    format: LookupStdoutFormat,
    response: &RegistryDetailResponse,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    match format {
        LookupStdoutFormat::Json => write_json(response, writer),
        LookupStdoutFormat::Text => write_detail_text(response, writer),
    }
}

fn write_json(value: &impl Serialize, writer: &mut impl Write) -> Result<(), SearchCommandError> {
    serde_json::to_writer(&mut *writer, value).map_err(|source| SearchCommandError::Io {
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })?;
    writeln!(writer).map_err(|source| SearchCommandError::Io { source })?;
    Ok(())
}

fn write_search_text(
    response: &RegistrySearchResponse,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    writeln!(writer, "query: {}", response.query).map_err(write_error)?;
    writeln!(writer, "results: {}", response.results.len()).map_err(write_error)?;
    if let Some(kind) = response.filters.kind {
        writeln!(writer, "kind: {}", entry_kind_label(kind)).map_err(write_error)?;
    }
    if let Some(language) = &response.filters.language {
        writeln!(writer, "language: {}", language.as_str()).map_err(write_error)?;
    }
    writeln!(writer, "limit: {}", response.filters.limit).map_err(write_error)?;
    writeln!(writer).map_err(write_error)?;

    if response.results.is_empty() {
        writeln!(writer, "No matching registry entries.").map_err(write_error)?;
        return Ok(());
    }

    for result in &response.results {
        writeln!(
            writer,
            "{:<9}  {}  {}",
            entry_kind_label(result.kind),
            result.id,
            result.label
        )
        .map_err(write_error)?;
        if !result.summary.is_empty() {
            writeln!(writer, "  summary: {}", result.summary).map_err(write_error)?;
        }
        if !result.aliases.is_empty() {
            writeln!(writer, "  aliases: {}", result.aliases.join(", ")).map_err(write_error)?;
        }
        if !result.tags.is_empty() {
            writeln!(writer, "  tags: {}", result.tags.join(", ")).map_err(write_error)?;
        }
        if let Some(status) = result.extraction_status {
            writeln!(writer, "  extraction: {}", extraction_status_label(status))
                .map_err(write_error)?;
        }
        write_snapshot_lines(writer, &result.snapshot, "  ")?;
        writeln!(writer).map_err(write_error)?;
    }
    Ok(())
}

fn write_detail_text(
    response: &RegistryDetailResponse,
    writer: &mut impl Write,
) -> Result<(), SearchCommandError> {
    // Detail responses keep a lossless JSON entry object so unknown fields survive
    // round-trips; text rendering therefore reads presentation keys from that map.
    writeln!(writer, "kind: {}", entry_kind_label(response.kind)).map_err(write_error)?;
    write_optional_entry_str(writer, &response.entry, "id")?;
    write_snapshot_lines(writer, &response.snapshot, "")?;
    writeln!(writer).map_err(write_error)?;

    for key in ["symbol", "key", "package", "value", "category"] {
        write_optional_entry_str(writer, &response.entry, key)?;
    }
    if let Some(aliases) = response
        .entry
        .get("aliases")
        .and_then(|value| value.as_array())
        && !aliases.is_empty()
    {
        let rendered = aliases
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(writer, "aliases: {rendered}").map_err(write_error)?;
    }

    write_pretty_entry_section(writer, &response.entry, "metadata")?;
    write_pretty_entry_section(writer, &response.entry, "details")?;

    writeln!(writer, "\nentry:").map_err(write_error)?;
    writeln!(
        writer,
        "{}",
        serde_json::to_string_pretty(&response.entry).map_err(|source| SearchCommandError::Io {
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?
    )
    .map_err(write_error)?;
    Ok(())
}

fn write_optional_entry_str(
    writer: &mut impl Write,
    entry: &serde_json::Value,
    key: &str,
) -> Result<(), SearchCommandError> {
    if let Some(value) = entry.get(key).and_then(serde_json::Value::as_str) {
        writeln!(writer, "{key}: {value}").map_err(write_error)?;
    }
    Ok(())
}

fn write_pretty_entry_section(
    writer: &mut impl Write,
    entry: &serde_json::Value,
    key: &str,
) -> Result<(), SearchCommandError> {
    writeln!(writer, "\n{key}:").map_err(write_error)?;
    match entry.get(key) {
        Some(value) if !value.is_null() => {
            writeln!(
                writer,
                "{}",
                serde_json::to_string_pretty(value).map_err(|source| SearchCommandError::Io {
                    source: io::Error::new(io::ErrorKind::InvalidData, source),
                })?
            )
            .map_err(write_error)?;
        }
        _ => {
            writeln!(writer, "  <missing>").map_err(write_error)?;
        }
    }
    Ok(())
}

fn write_snapshot_lines(
    writer: &mut impl Write,
    snapshot: &RegistrySnapshotRef,
    indent: &str,
) -> Result<(), SearchCommandError> {
    writeln!(
        writer,
        "{indent}source: {}  language: {}  digest: {}",
        snapshot.source,
        snapshot.language.as_str(),
        snapshot.digest
    )
    .map_err(write_error)?;
    if let Some(system_id) = &snapshot.system_id {
        writeln!(writer, "{indent}system: {system_id}").map_err(write_error)?;
    }
    if let Some(commit) = &snapshot.git_commit {
        writeln!(writer, "{indent}git_commit: {commit}").map_err(write_error)?;
    }
    Ok(())
}

fn entry_kind_label(kind: RegistryEntryKind) -> &'static str {
    match kind {
        RegistryEntryKind::Component => "component",
        RegistryEntryKind::Token => "token",
    }
}

fn extraction_status_label(status: RegistryExtractionStatus) -> &'static str {
    match status {
        RegistryExtractionStatus::Complete => "complete",
        RegistryExtractionStatus::Partial => "partial",
        RegistryExtractionStatus::Unavailable => "unavailable",
        RegistryExtractionStatus::Stale => "stale",
        RegistryExtractionStatus::NotObserved => "not_observed",
    }
}

fn write_error(source: io::Error) -> SearchCommandError {
    SearchCommandError::Io { source }
}

fn paths_to_lookup_error(error: &PathsError) -> RegistryLookupError {
    RegistryLookupError::new(RegistryLookupErrorCode::InvalidArgument, error.to_string())
}
