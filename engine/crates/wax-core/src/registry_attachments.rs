//! Safe materialization of creator-associated registry sources.

use std::fs;
use std::io;
use std::path::{Component, Path};

use thiserror::Error;
use wax_contract::{
    Diagnostic, DiagnosticSeverity, RegistryAttachment, RegistryDocument, RegistryExtracted,
    RegistryExtractionStatus, RegistrySourceAssociation,
};

/// Typed failures returned while materializing registry attachments.
#[derive(Debug, Error)]
pub enum RegistryAttachmentError {
    /// The repository root could not be resolved.
    #[error("failed to resolve repository root {path}: {source}")]
    ResolveRepoRoot {
        /// Repository root that could not be resolved.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// An attachment path was not a safe repository-relative path.
    #[error(
        "registry attachment path `{path}` must be repository-relative without parent traversal"
    )]
    UnsafePath {
        /// Rejected creator-authored path.
        path: String,
    },
    /// An attachment resolved outside the repository, including through a symlink.
    #[error("registry attachment path `{path}` resolves outside the repository")]
    PathEscapesRepo {
        /// Rejected creator-authored path.
        path: String,
    },
    /// An association resolved to a path type that cannot be materialized.
    #[error("registry attachment path `{path}` is not a regular file")]
    UnsupportedAssociation {
        /// Creator-authored attachment path.
        path: String,
    },
    /// An attachment file could not be resolved.
    #[error("failed to resolve registry attachment `{path}`: {source}")]
    ResolveAttachment {
        /// Creator-authored attachment path.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// An attachment file could not be read as UTF-8 text.
    #[error("failed to read registry attachment `{path}`: {source}")]
    ReadAttachment {
        /// Creator-authored attachment path.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// An attachment requested an invalid line range.
    #[error("registry attachment `{path}` has invalid line range {start:?}..={end:?}")]
    InvalidLineRange {
        /// Creator-authored attachment path.
        path: String,
        /// Optional inclusive start line.
        start: Option<u32>,
        /// Optional inclusive end line.
        end: Option<u32>,
    },
}

/// Materializes creator-associated repository files into offline attachments.
///
/// # Errors
///
/// Returns [`RegistryAttachmentError`] when the repository root cannot be
/// resolved, an association is unsafe or invalid, or an associated file cannot
/// be resolved or read.
pub fn materialize_registry_attachments(
    repo_root: &Path,
    document: &mut RegistryDocument,
) -> Result<Vec<Diagnostic>, RegistryAttachmentError> {
    let canonical_repo_root =
        fs::canonicalize(repo_root).map_err(|source| RegistryAttachmentError::ResolveRepoRoot {
            path: repo_root.display().to_string(),
            source,
        })?;
    let mut diagnostics = Vec::new();

    for component in &mut document.components {
        let Some(metadata) = &component.metadata else {
            continue;
        };
        let attachments =
            materialize_sources(&canonical_repo_root, &metadata.sources, &mut diagnostics)?;
        append_unique_attachments(&mut component.extracted, attachments);
    }
    for token in &mut document.tokens {
        let Some(metadata) = &token.metadata else {
            continue;
        };
        let attachments =
            materialize_sources(&canonical_repo_root, &metadata.sources, &mut diagnostics)?;
        append_unique_attachments(&mut token.extracted, attachments);
    }

    Ok(diagnostics)
}

fn ensure_extracted(extracted: &mut Option<RegistryExtracted>) -> &mut RegistryExtracted {
    extracted.get_or_insert_with(|| RegistryExtracted {
        status: RegistryExtractionStatus::Partial,
        source_evidence: None,
        documentation: None,
        signature: None,
        deprecation: None,
        attachments: Vec::new(),
        field_gaps: Vec::new(),
        best_practices: Vec::new(),
        extra: serde_json::Map::new(),
    })
}

fn append_unique_attachments(
    extracted: &mut Option<RegistryExtracted>,
    attachments: Vec<RegistryAttachment>,
) {
    if attachments.is_empty() {
        return;
    }
    let existing = &mut ensure_extracted(extracted).attachments;
    for attachment in attachments {
        if !existing.contains(&attachment) {
            existing.push(attachment);
        }
    }
}

fn materialize_sources(
    canonical_repo_root: &Path,
    sources: &[RegistrySourceAssociation],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<RegistryAttachment>, RegistryAttachmentError> {
    let mut attachments = Vec::with_capacity(sources.len());
    for source in sources {
        if source.path.contains("://") {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "registry_attachment_unsupported".to_owned(),
                message: format!(
                    "associated registry source `{}` is not a repository file",
                    source.path
                ),
                location: None,
            });
            continue;
        }
        match materialize_source(canonical_repo_root, source) {
            Ok(attachment) => attachments.push(attachment),
            Err(RegistryAttachmentError::ResolveAttachment {
                path,
                source: source_error,
            }) if source_error.kind() == io::ErrorKind::NotFound => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "registry_attachment_missing".to_owned(),
                    message: format!("associated registry source `{path}` does not exist"),
                    location: None,
                });
            }
            Err(RegistryAttachmentError::UnsupportedAssociation { path }) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "registry_attachment_unsupported".to_owned(),
                    message: format!(
                        "associated registry source `{path}` is not a regular repository file"
                    ),
                    location: None,
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(attachments)
}

fn materialize_source(
    canonical_repo_root: &Path,
    source: &RegistrySourceAssociation,
) -> Result<RegistryAttachment, RegistryAttachmentError> {
    let relative_path = Path::new(&source.path);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RegistryAttachmentError::UnsafePath {
            path: source.path.clone(),
        });
    }
    if source.start_line == Some(0)
        || source.end_line == Some(0)
        || matches!((source.start_line, source.end_line), (Some(start), Some(end)) if start > end)
    {
        return Err(RegistryAttachmentError::InvalidLineRange {
            path: source.path.clone(),
            start: source.start_line,
            end: source.end_line,
        });
    }

    let candidate = canonical_repo_root.join(relative_path);
    let canonical_path = fs::canonicalize(&candidate).map_err(|source_error| {
        RegistryAttachmentError::ResolveAttachment {
            path: source.path.clone(),
            source: source_error,
        }
    })?;
    if !canonical_path.starts_with(canonical_repo_root) {
        return Err(RegistryAttachmentError::PathEscapesRepo {
            path: source.path.clone(),
        });
    }
    if !canonical_path.is_file() {
        return Err(RegistryAttachmentError::UnsupportedAssociation {
            path: source.path.clone(),
        });
    }
    let content = fs::read_to_string(&canonical_path).map_err(|source_error| {
        RegistryAttachmentError::ReadAttachment {
            path: source.path.clone(),
            source: source_error,
        }
    })?;
    let content = excerpt(&content, source)?;

    Ok(RegistryAttachment {
        kind: source.kind,
        path: source.path.clone(),
        content,
        language: source.language.clone(),
        extra: source.extra.clone(),
    })
}

fn excerpt(
    content: &str,
    source: &RegistrySourceAssociation,
) -> Result<String, RegistryAttachmentError> {
    if source.start_line.is_none() && source.end_line.is_none() {
        return Ok(content.to_owned());
    }
    let start = source.start_line.unwrap_or(1) as usize;
    let end = source.end_line.map_or(usize::MAX, |line| line as usize);
    let lines = content.lines().collect::<Vec<_>>();
    if start > lines.len() || end.min(lines.len()) < start {
        return Err(RegistryAttachmentError::InvalidLineRange {
            path: source.path.clone(),
            start: source.start_line,
            end: source.end_line,
        });
    }
    let last = end.min(lines.len());
    let mut excerpt = lines[start - 1..last].join("\n");
    if last > start - 1 {
        excerpt.push('\n');
    }
    Ok(excerpt)
}
