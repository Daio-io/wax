//! Lossless merging of discovered registry facts into authored registries.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wax_contract::{
    RegistryComponent, RegistryDocument, RegistryError, RegistryExtracted,
    RegistryExtractionStatus, RegistryToken,
};

/// Result of merging discovered facts into an optional authored registry.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryMerge {
    /// Losslessly merged registry document.
    pub document: RegistryDocument,
    /// Entry-level changes suitable for a preview UI.
    pub changes: Vec<RegistryMergeChange>,
    /// Non-fatal identity conflicts encountered during the merge.
    pub diagnostics: Vec<RegistryMergeDiagnostic>,
}

/// Kind of registry entry affected by a merge.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryMergeEntryKind {
    /// Component registry entry.
    Component,
    /// Token registry entry.
    Token,
}

/// Kind of observable entry-level merge change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryMergeChangeKind {
    /// A newly discovered entry was added.
    Added,
    /// Generated facts or observation status changed on an existing entry.
    Updated,
}

/// Structured entry-level change returned for merge previews.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryMergeChange {
    /// Registry entry kind.
    pub entry_kind: RegistryMergeEntryKind,
    /// Stable id in the merged document.
    pub id: String,
    /// Change applied to the entry.
    pub kind: RegistryMergeChangeKind,
}

/// Stable diagnostic code emitted by registry merging.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryMergeDiagnosticCode {
    /// Available identity evidence points at multiple authored entries.
    IdentityConflict,
}

/// Non-fatal merge diagnostic that preserves ambiguous authored entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryMergeDiagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: RegistryMergeDiagnosticCode,
    /// Registry entry kind involved in the conflict.
    pub entry_kind: RegistryMergeEntryKind,
    /// Id supplied by the discovered entry.
    pub discovered_id: String,
    /// Human-readable conflict explanation.
    pub message: String,
}

/// Typed failures returned by [`merge_registry`].
#[derive(Debug, Error)]
pub enum RegistryMergeError {
    /// The existing registry violates the public registry contract.
    #[error("existing registry is invalid: {0}")]
    InvalidExisting(#[source] RegistryError),
    /// The discovered registry violates the public registry contract.
    #[error("discovered registry is invalid: {0}")]
    InvalidDiscovered(#[source] RegistryError),
    /// The merged registry violates the public registry contract.
    #[error("merged registry is invalid: {0}")]
    InvalidMerged(#[source] RegistryError),
}

/// Merges generated discovery facts without replacing creator-owned content.
///
/// `complete` describes whether discovery enumerated the full source scope. It
/// is accepted for API/spec compatibility and never authorizes pruning; entries
/// absent from discovery are retained with a `not_observed` extraction status
/// whether `complete` is `true` or `false`.
///
/// # Errors
///
/// Returns [`RegistryMergeError`] when an input or the merged output violates
/// the registry contract.
pub fn merge_registry(
    existing: Option<&RegistryDocument>,
    discovered: &RegistryDocument,
    complete: bool,
) -> Result<RegistryMerge, RegistryMergeError> {
    // Enumeration completeness is intentionally unused today: limited roots
    // never authorize deletion, and retention/`not_observed` is identical for
    // both values. Keep the parameter so callers can pass the discovery signal
    // without a future API break.
    let _ = complete;
    if let Some(existing) = existing {
        existing
            .validate()
            .map_err(RegistryMergeError::InvalidExisting)?;
    }
    discovered
        .validate()
        .map_err(RegistryMergeError::InvalidDiscovered)?;

    let Some(existing) = existing else {
        return Ok(RegistryMerge {
            document: discovered.clone(),
            changes: discovered_changes(discovered),
            diagnostics: Vec::new(),
        });
    };

    let mut document = existing.clone();
    let mut changes = Vec::new();
    let mut diagnostics = Vec::new();
    let existing_component_count = document.components.len();
    let existing_token_count = document.tokens.len();

    merge_entries(
        &mut document.components,
        existing_component_count,
        &discovered.components,
        RegistryMergeEntryKind::Component,
        resolve_component_identity,
        &mut changes,
        &mut diagnostics,
    );
    merge_entries(
        &mut document.tokens,
        existing_token_count,
        &discovered.tokens,
        RegistryMergeEntryKind::Token,
        resolve_token_identity,
        &mut changes,
        &mut diagnostics,
    );

    document
        .validate()
        .map_err(RegistryMergeError::InvalidMerged)?;
    Ok(RegistryMerge {
        document,
        changes,
        diagnostics,
    })
}

trait MergeEntry: Clone {
    fn id(&self) -> &str;
    fn extracted(&self) -> Option<&RegistryExtracted>;
    fn set_extracted(&mut self, extracted: Option<RegistryExtracted>);
}

impl MergeEntry for RegistryComponent {
    fn id(&self) -> &str {
        &self.id
    }

    fn extracted(&self) -> Option<&RegistryExtracted> {
        self.extracted.as_ref()
    }

    fn set_extracted(&mut self, extracted: Option<RegistryExtracted>) {
        self.extracted = extracted;
    }
}

impl MergeEntry for RegistryToken {
    fn id(&self) -> &str {
        &self.id
    }

    fn extracted(&self) -> Option<&RegistryExtracted> {
        self.extracted.as_ref()
    }

    fn set_extracted(&mut self, extracted: Option<RegistryExtracted>) {
        self.extracted = extracted;
    }
}

fn merge_entries<T: MergeEntry>(
    entries: &mut Vec<T>,
    existing_count: usize,
    discovered_entries: &[T],
    entry_kind: RegistryMergeEntryKind,
    resolve: impl Fn(&[T], &T) -> IdentityResolution,
    changes: &mut Vec<RegistryMergeChange>,
    diagnostics: &mut Vec<RegistryMergeDiagnostic>,
) {
    let mut retain_extracted = vec![false; existing_count];
    let kind_label = entry_kind_label(entry_kind);

    for discovered in discovered_entries {
        match resolve(&entries[..existing_count], discovered) {
            IdentityResolution::Match(index) => {
                retain_extracted[index] = true;
                let entry = &mut entries[index];
                let extracted = merge_extracted(entry.extracted(), discovered.extracted());
                if entry.extracted() != extracted.as_ref() {
                    let id = entry.id().to_owned();
                    entry.set_extracted(extracted);
                    changes.push(updated_change(entry_kind, &id));
                }
            }
            IdentityResolution::NoMatch => {
                changes.push(RegistryMergeChange {
                    entry_kind,
                    id: discovered.id().to_owned(),
                    kind: RegistryMergeChangeKind::Added,
                });
                entries.push(discovered.clone());
            }
            IdentityResolution::Conflict(candidate_indices) => {
                // Diagnostics only: leave candidate extraction untouched.
                for &index in &candidate_indices {
                    retain_extracted[index] = true;
                }
                let candidate_ids = candidate_indices
                    .iter()
                    .map(|&index| entries[index].id().to_owned())
                    .collect::<Vec<_>>();
                diagnostics.push(RegistryMergeDiagnostic {
                    code: RegistryMergeDiagnosticCode::IdentityConflict,
                    entry_kind,
                    discovered_id: discovered.id().to_owned(),
                    message: format!(
                        "discovered {kind_label} `{}` has ambiguous identity candidates: {}",
                        discovered.id(),
                        candidate_ids.join(", ")
                    ),
                });
                if !entries.iter().any(|entry| entry.id() == discovered.id()) {
                    changes.push(RegistryMergeChange {
                        entry_kind,
                        id: discovered.id().to_owned(),
                        kind: RegistryMergeChangeKind::Added,
                    });
                    entries.push(discovered.clone());
                }
            }
        }
    }

    for (index, retain) in retain_extracted.into_iter().enumerate() {
        if retain {
            continue;
        }
        let entry = &mut entries[index];
        let extracted = Some(not_observed(entry.extracted()));
        if entry.extracted() != extracted.as_ref() {
            let id = entry.id().to_owned();
            entry.set_extracted(extracted);
            changes.push(updated_change(entry_kind, &id));
        }
    }
}

fn merge_extracted(
    existing: Option<&RegistryExtracted>,
    discovered: Option<&RegistryExtracted>,
) -> Option<RegistryExtracted> {
    let Some(discovered) = discovered else {
        return existing.cloned().map(|mut extracted| {
            extracted.status = RegistryExtractionStatus::Stale;
            extracted
        });
    };
    let Some(existing) = existing else {
        return Some(discovered.clone());
    };
    let mut merged = existing.clone();
    merged.status = match discovered.status {
        RegistryExtractionStatus::Complete => return Some(discovered.clone()),
        RegistryExtractionStatus::Partial => RegistryExtractionStatus::Partial,
        RegistryExtractionStatus::Unavailable | RegistryExtractionStatus::Stale => {
            RegistryExtractionStatus::Stale
        }
        RegistryExtractionStatus::NotObserved => RegistryExtractionStatus::NotObserved,
    };
    if discovered.source_evidence.is_some() {
        merged
            .source_evidence
            .clone_from(&discovered.source_evidence);
    }
    if discovered.documentation.is_some() {
        merged.documentation.clone_from(&discovered.documentation);
    }
    if discovered.signature.is_some() {
        merged.signature.clone_from(&discovered.signature);
    }
    if discovered.deprecation.is_some() {
        merged.deprecation.clone_from(&discovered.deprecation);
    }
    if !discovered.attachments.is_empty() {
        merged.attachments.clone_from(&discovered.attachments);
    }
    if !discovered.field_gaps.is_empty() {
        merged.field_gaps.clone_from(&discovered.field_gaps);
    }
    merged.extra.extend(discovered.extra.clone());
    Some(merged)
}

fn not_observed(existing: Option<&RegistryExtracted>) -> RegistryExtracted {
    let mut extracted = existing.cloned().unwrap_or_else(|| RegistryExtracted {
        status: RegistryExtractionStatus::NotObserved,
        source_evidence: None,
        documentation: None,
        signature: None,
        deprecation: None,
        attachments: Vec::new(),
        field_gaps: Vec::new(),
        extra: serde_json::Map::new(),
    });
    extracted.status = RegistryExtractionStatus::NotObserved;
    extracted
}

enum IdentityResolution {
    Match(usize),
    NoMatch,
    Conflict(Vec<usize>),
}

fn resolve_component_identity(
    existing: &[RegistryComponent],
    discovered: &RegistryComponent,
) -> IdentityResolution {
    let id_match = existing
        .iter()
        .position(|component| component.id == discovered.id);
    let candidates = existing
        .iter()
        .enumerate()
        .filter(|(_, component)| match &discovered.package {
            Some(package) => {
                component.package.as_ref() == Some(package) && component.symbol == discovered.symbol
            }
            None => component.symbol == discovered.symbol,
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    resolve_identity(id_match, &candidates)
}

fn resolve_token_identity(
    existing: &[RegistryToken],
    discovered: &RegistryToken,
) -> IdentityResolution {
    let id_match = existing.iter().position(|token| token.id == discovered.id);
    let candidates = existing
        .iter()
        .enumerate()
        .filter(|(_, token)| token.key == discovered.key)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    resolve_identity(id_match, &candidates)
}

fn resolve_identity(id_match: Option<usize>, candidates: &[usize]) -> IdentityResolution {
    if let Some(id_index) = id_match {
        // Exact IDs disambiguate ambiguous fallbacks; only conflict when a
        // unique fallback points at a different entry than the ID. Include both
        // indices so conflict retention covers the id hit as well as the
        // fallback match (diagnostics only — neither becomes not_observed).
        if candidates.len() == 1 && candidates[0] != id_index {
            return IdentityResolution::Conflict(vec![id_index, candidates[0]]);
        }
        return IdentityResolution::Match(id_index);
    }

    match candidates {
        [] => IdentityResolution::NoMatch,
        [index] => IdentityResolution::Match(*index),
        _ => IdentityResolution::Conflict(candidates.to_vec()),
    }
}

fn entry_kind_label(entry_kind: RegistryMergeEntryKind) -> &'static str {
    match entry_kind {
        RegistryMergeEntryKind::Component => "component",
        RegistryMergeEntryKind::Token => "token",
    }
}

fn updated_change(entry_kind: RegistryMergeEntryKind, id: &str) -> RegistryMergeChange {
    RegistryMergeChange {
        entry_kind,
        id: id.to_owned(),
        kind: RegistryMergeChangeKind::Updated,
    }
}

fn discovered_changes(discovered: &RegistryDocument) -> Vec<RegistryMergeChange> {
    discovered
        .components
        .iter()
        .map(|component| RegistryMergeChange {
            entry_kind: RegistryMergeEntryKind::Component,
            id: component.id.clone(),
            kind: RegistryMergeChangeKind::Added,
        })
        .chain(discovered.tokens.iter().map(|token| RegistryMergeChange {
            entry_kind: RegistryMergeEntryKind::Token,
            id: token.id.clone(),
            kind: RegistryMergeChangeKind::Added,
        }))
        .collect()
}
