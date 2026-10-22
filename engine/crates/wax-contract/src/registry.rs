//! Lossless design-system registry document types and validation.

use crate::TokenCategory;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use thiserror::Error;

/// Registry JSON schema version. Enrichment fields are additive under version 1.
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Errors returned while parsing or validating registry documents.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// JSON could not be deserialized into the registry contract.
    #[error("invalid registry JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// `schema_version` did not match [`REGISTRY_SCHEMA_VERSION`].
    #[error("unsupported registry schema_version {found}; engine supports {supported}")]
    UnsupportedSchemaVersion {
        /// Schema version found in the input document.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
    /// The document deserialized but violated registry contract invariants.
    #[error("invalid registry contract at {field}: {message}")]
    ContractViolation {
        /// Dotted field path for the violated contract field.
        field: String,
        /// Human-readable validation message.
        message: String,
    },
}

/// Published design-system registry document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryDocument {
    /// Registry schema version. Must be [`REGISTRY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Component entries. May be empty for token-only lookup documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<RegistryComponent>,
    /// Token entries. May be empty for component-only registries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<RegistryToken>,
    /// Unknown root fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Component entry in a published registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryComponent {
    /// Stable component id within the registry.
    pub id: String,
    /// Source symbol used by the language ecosystem.
    pub symbol: String,
    /// Optional package or module identity for resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Exact source-facing aliases for the same component.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Creator-owned metadata for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RegistryMetadata>,
    /// Generated extraction facts for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted: Option<RegistryExtracted>,
    /// Unknown entry fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Token entry in a published registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryToken {
    /// Stable token id within the registry.
    pub id: String,
    /// Exact source-facing token key.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
    /// Exact source-facing aliases for the same token.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Optional canonical source-facing value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Creator-owned metadata for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RegistryMetadata>,
    /// Generated extraction facts for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted: Option<RegistryExtracted>,
    /// Unknown entry fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Creator-owned registry entry metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryMetadata {
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Free-form tags for search and organization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Explicit source associations for this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<RegistrySourceAssociation>,
    /// Unknown metadata fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Kind of explicit source association on a registry entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RegistrySourceKind {
    /// Source declaration for the entry.
    Declaration,
    /// Documentation file associated with the entry.
    Documentation,
    /// Example file associated with the entry.
    Example,
}

/// Explicit creator association between an entry and a repository path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySourceAssociation {
    /// Association kind.
    pub kind: RegistrySourceKind,
    /// Repository-relative path.
    pub path: String,
    /// Optional inclusive start line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    /// Optional inclusive end line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// Optional language id or language hint for the associated file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Unknown association fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Extraction completeness for generated registry facts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RegistryExtractionStatus {
    /// All expected extracted fields were observed.
    Complete,
    /// Some extracted fields are present but incomplete.
    Partial,
    /// Extraction was attempted but facts are unavailable.
    Unavailable,
    /// Previously extracted facts may no longer match source.
    Stale,
    /// Extraction was not attempted for this entry.
    NotObserved,
}

/// Source location evidence captured during extraction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySourceEvidence {
    /// Repository-relative path of the observed declaration.
    pub path: String,
    /// Optional inclusive start line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    /// Optional inclusive end line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// Unknown evidence fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Parameter fact extracted from a public declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryParameter {
    /// Parameter name as written in source.
    pub name: String,
    /// Source type text when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    /// Default expression text when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Requiredness when syntax establishes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Unknown parameter fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Signature facts extracted from a public declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySignature {
    /// Full source signature text when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    /// Extracted parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RegistryParameter>,
    /// Unknown signature fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Deprecation facts extracted or published for an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryDeprecation {
    /// Human-readable deprecation message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Version or date when deprecation began.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Replacement entry id when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
    /// Unknown deprecation fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Materialized documentation or example attachment content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryAttachment {
    /// Attachment kind.
    pub kind: RegistrySourceKind,
    /// Repository-relative path of the attachment source.
    pub path: String,
    /// Materialized file contents.
    pub content: String,
    /// Optional language hint for the attachment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Unknown attachment fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Explicit gap for an extracted field that could not be observed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryFieldGap {
    /// Dotted field path that is missing or incomplete.
    pub field: String,
    /// Human-readable reason the field is unavailable.
    pub reason: String,
    /// Unknown gap fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Generated extraction facts for a registry entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryExtracted {
    /// Extraction completeness status.
    pub status: RegistryExtractionStatus,
    /// Source evidence for the declaration when observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_evidence: Option<RegistrySourceEvidence>,
    /// Extracted documentation text when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    /// Signature and parameter facts when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<RegistrySignature>,
    /// Deprecation facts when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<RegistryDeprecation>,
    /// Materialized attachments for offline lookup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<RegistryAttachment>,
    /// Fields that could not be fully extracted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_gaps: Vec<RegistryFieldGap>,
    /// Unknown extracted fields preserved across round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Deserializes a registry document from JSON and validates the contract.
///
/// # Errors
///
/// Returns [`RegistryError::InvalidJson`] for malformed JSON,  
/// [`RegistryError::UnsupportedSchemaVersion`] for an incompatible schema, or  
/// [`RegistryError::ContractViolation`] when invariants fail.
pub fn registry_from_json(input: &str) -> Result<RegistryDocument, RegistryError> {
    let document: RegistryDocument = serde_json::from_str(input)?;
    document.validate()?;
    Ok(document)
}

/// Serializes a registry document to JSON.
///
/// # Errors
///
/// Returns [`RegistryError::InvalidJson`] when serialization fails.
pub fn registry_to_json(document: &RegistryDocument) -> Result<String, RegistryError> {
    Ok(serde_json::to_string(document)?)
}

impl RegistryDocument {
    /// Validates schema version and entry invariants.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnsupportedSchemaVersion`] or
    /// [`RegistryError::ContractViolation`] when validation fails.
    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(RegistryError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: REGISTRY_SCHEMA_VERSION,
            });
        }

        let mut component_ids = BTreeSet::new();
        for (index, component) in self.components.iter().enumerate() {
            let field = format!("components[{index}]");
            require_non_empty(&format!("{field}.id"), &component.id)?;
            require_non_empty(&format!("{field}.symbol"), &component.symbol)?;
            if let Some(package) = &component.package {
                require_non_empty(&format!("{field}.package"), package)?;
            }
            for (alias_index, alias) in component.aliases.iter().enumerate() {
                require_non_empty(&format!("{field}.aliases[{alias_index}]"), alias)?;
            }
            if !component_ids.insert(component.id.clone()) {
                return Err(RegistryError::ContractViolation {
                    field: format!("{field}.id"),
                    message: format!("duplicate component id `{}`", component.id),
                });
            }
            if let Some(metadata) = &component.metadata {
                validate_metadata(&format!("{field}.metadata"), metadata)?;
            }
            if let Some(extracted) = &component.extracted {
                validate_extracted(&format!("{field}.extracted"), extracted)?;
            }
        }

        let mut token_ids = BTreeSet::new();
        for (index, token) in self.tokens.iter().enumerate() {
            let field = format!("tokens[{index}]");
            require_non_empty(&format!("{field}.id"), &token.id)?;
            require_non_empty(&format!("{field}.key"), &token.key)?;
            if let Some(value) = &token.value {
                require_non_empty(&format!("{field}.value"), value)?;
            }
            for (alias_index, alias) in token.aliases.iter().enumerate() {
                require_non_empty(&format!("{field}.aliases[{alias_index}]"), alias)?;
            }
            if !token_ids.insert(token.id.clone()) {
                return Err(RegistryError::ContractViolation {
                    field: format!("{field}.id"),
                    message: format!("duplicate token id `{}`", token.id),
                });
            }
            if let Some(metadata) = &token.metadata {
                validate_metadata(&format!("{field}.metadata"), metadata)?;
            }
            if let Some(extracted) = &token.extracted {
                validate_extracted(&format!("{field}.extracted"), extracted)?;
            }
        }

        Ok(())
    }
}

fn validate_metadata(field: &str, metadata: &RegistryMetadata) -> Result<(), RegistryError> {
    if let Some(description) = &metadata.description {
        require_non_empty(&format!("{field}.description"), description)?;
    }
    for (index, tag) in metadata.tags.iter().enumerate() {
        require_non_empty(&format!("{field}.tags[{index}]"), tag)?;
    }
    for (index, source) in metadata.sources.iter().enumerate() {
        let source_field = format!("{field}.sources[{index}]");
        require_non_empty(&format!("{source_field}.path"), &source.path)?;
        if let Some(language) = &source.language {
            // Language hints may be free-form (for example `kotlin`).
            require_non_empty(&format!("{source_field}.language"), language)?;
        }
        if let (Some(start), Some(end)) = (source.start_line, source.end_line)
            && start > end
        {
            return Err(RegistryError::ContractViolation {
                field: source_field,
                message: "start_line must be <= end_line".to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_extracted(field: &str, extracted: &RegistryExtracted) -> Result<(), RegistryError> {
    if let Some(documentation) = &extracted.documentation {
        require_non_empty(&format!("{field}.documentation"), documentation)?;
    }
    if let Some(evidence) = &extracted.source_evidence {
        require_non_empty(&format!("{field}.source_evidence.path"), &evidence.path)?;
        if let (Some(start), Some(end)) = (evidence.start_line, evidence.end_line)
            && start > end
        {
            return Err(RegistryError::ContractViolation {
                field: format!("{field}.source_evidence"),
                message: "start_line must be <= end_line".to_owned(),
            });
        }
    }
    if let Some(signature) = &extracted.signature {
        if let Some(type_text) = &signature.type_text {
            require_non_empty(&format!("{field}.signature.type_text"), type_text)?;
        }
        for (index, parameter) in signature.parameters.iter().enumerate() {
            let parameter_field = format!("{field}.signature.parameters[{index}]");
            require_non_empty(&format!("{parameter_field}.name"), &parameter.name)?;
            if let Some(type_text) = &parameter.type_text {
                require_non_empty(&format!("{parameter_field}.type_text"), type_text)?;
            }
            if let Some(default) = &parameter.default {
                require_non_empty(&format!("{parameter_field}.default"), default)?;
            }
        }
    }
    if let Some(deprecation) = &extracted.deprecation {
        if let Some(message) = &deprecation.message {
            require_non_empty(&format!("{field}.deprecation.message"), message)?;
        }
        if let Some(since) = &deprecation.since {
            require_non_empty(&format!("{field}.deprecation.since"), since)?;
        }
        if let Some(replaced_by) = &deprecation.replaced_by {
            require_non_empty(&format!("{field}.deprecation.replaced_by"), replaced_by)?;
        }
    }
    for (index, attachment) in extracted.attachments.iter().enumerate() {
        let attachment_field = format!("{field}.attachments[{index}]");
        require_non_empty(&format!("{attachment_field}.path"), &attachment.path)?;
        // Content may be empty for intentionally blank examples.
        if let Some(language) = &attachment.language {
            require_non_empty(&format!("{attachment_field}.language"), language)?;
        }
    }
    for (index, gap) in extracted.field_gaps.iter().enumerate() {
        let gap_field = format!("{field}.field_gaps[{index}]");
        require_non_empty(&format!("{gap_field}.field"), &gap.field)?;
        require_non_empty(&format!("{gap_field}.reason"), &gap.reason)?;
    }
    Ok(())
}

fn require_non_empty(field: &str, value: &str) -> Result<(), RegistryError> {
    if value.is_empty() {
        Err(RegistryError::ContractViolation {
            field: field.to_owned(),
            message: "must be a non-empty string".to_owned(),
        })
    } else {
        Ok(())
    }
}
