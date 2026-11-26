//! Shared import-aware registry resolution helpers for parser-backed language packs.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use thiserror::Error;
use wax_contract::{CalleeOrigin, DesignSystemComponent, MatchStatus, ResolutionEvidenceKind};

/// Errors returned while indexing registry component definitions.
#[derive(Debug, Error)]
pub enum RegistryComponentError {
    /// The registry does not contain a components array.
    #[error("registry JSON must contain a components array")]
    MissingComponents,
    /// A component field is missing or malformed.
    #[error("components[{index}]{reason}")]
    InvalidField {
        /// Component array index.
        index: usize,
        /// Detail about the invalid field.
        reason: String,
    },
    /// Multiple components would produce the same canonical identity.
    #[error("duplicate component registry identity {0:?}")]
    IdentityCollision(String),
    /// More than one component claims the same alias in one package.
    #[error("ambiguous component alias {alias:?} for package {package:?}")]
    AmbiguousAlias {
        /// Alias claimed by multiple components.
        alias: String,
        /// Package shared by the components, if declared.
        package: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComponentTarget {
    package: Option<String>,
    component: usize,
}

/// Component facts and a package-aware index of their declared names and aliases.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryComponentIndex {
    components: Vec<DesignSystemComponent>,
    names: BTreeMap<String, Vec<ComponentTarget>>,
}

impl RegistryComponentIndex {
    /// Indexes the component symbols, packages, and aliases from a registry JSON document.
    ///
    /// # Errors
    ///
    /// Returns a registry component error for missing, malformed, or ambiguous definitions.
    pub fn from_json(value: &Value) -> Result<Self, RegistryComponentError> {
        let components = value
            .get("components")
            .and_then(Value::as_array)
            .ok_or(RegistryComponentError::MissingComponents)?;
        Self::from_components(components)
    }

    /// Indexes an already shape-checked array of component entries.
    ///
    /// # Errors
    ///
    /// Returns a registry component error for malformed or ambiguous definitions.
    pub fn from_components(components: &[Value]) -> Result<Self, RegistryComponentError> {
        let mut entries = Vec::with_capacity(components.len());
        let mut symbol_counts = BTreeMap::new();
        for (index, component) in components.iter().enumerate() {
            let invalid_field = |reason| RegistryComponentError::InvalidField { index, reason };
            let symbol = component
                .get("symbol")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_field(" is missing symbol".to_owned()))?;
            let package = component
                .get("package")
                .map(|value| {
                    let package = value
                        .as_str()
                        .ok_or_else(|| invalid_field(".package must be a string".to_owned()))?;
                    if package.is_empty() {
                        return Err(invalid_field(".package must not be empty".to_owned()));
                    }
                    Ok(package)
                })
                .transpose()?;
            let mut aliases = Vec::new();
            if let Some(values) = component.get("aliases").and_then(Value::as_array) {
                for (alias_index, alias) in values.iter().enumerate() {
                    aliases.push(alias.as_str().ok_or_else(|| {
                        invalid_field(format!(".aliases[{alias_index}] must be a string"))
                    })?);
                }
            }
            *symbol_counts.entry(symbol).or_insert(0) += 1;
            entries.push((symbol, package, aliases));
        }

        entries.sort_by_key(|&(symbol, package, _)| (symbol, package));
        let mut index = Self::default();
        for (symbol, package, _) in &entries {
            let registry_symbol = match package {
                Some(package) if symbol_counts[symbol] > 1 => format!("{package}#{symbol}"),
                _ => (*symbol).to_owned(),
            };
            let target = ComponentTarget {
                package: package.map(str::to_owned),
                component: index.components.len(),
            };
            index.components.push(DesignSystemComponent {
                id: format!("ds.{registry_symbol}"),
                symbol: (*symbol).to_owned(),
                registry_symbol,
            });
            index
                .names
                .entry((*symbol).to_owned())
                .or_default()
                .push(target);
        }
        let mut identities = BTreeSet::new();
        for component in &index.components {
            if !identities.insert(&component.registry_symbol) {
                return Err(RegistryComponentError::IdentityCollision(
                    component.registry_symbol.clone(),
                ));
            }
        }

        // All declared symbols are indexed before aliases so they win within a package.
        for (component, (_, package, aliases)) in entries.into_iter().enumerate() {
            for alias in aliases {
                let targets = index.names.entry(alias.to_owned()).or_default();
                if let Some(existing) = targets
                    .iter()
                    .find(|target| target.package.as_deref() == package)
                {
                    if existing.component == component
                        || index.components[existing.component].symbol == alias
                    {
                        continue;
                    }
                    return Err(RegistryComponentError::AmbiguousAlias {
                        alias: alias.to_owned(),
                        package: package.map(str::to_owned),
                    });
                }
                targets.push(ComponentTarget {
                    package: package.map(str::to_owned),
                    component,
                });
            }
        }
        Ok(index)
    }

    /// Returns all registered component facts in deterministic order.
    #[must_use]
    pub fn components(&self) -> &[DesignSystemComponent] {
        &self.components
    }

    /// Returns the owned component facts in deterministic order.
    #[must_use]
    pub fn into_components(self) -> Vec<DesignSystemComponent> {
        self.components
    }

    /// Returns whether the name appears as a symbol or alias, regardless of package evidence.
    #[must_use]
    pub fn contains_name(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// Resolves a name using the observed package. A mismatch has no target.
    /// Returns no match when the name is absent or the package evidence cannot distinguish targets.
    #[must_use]
    pub fn resolve(
        &self,
        name: &str,
        package: Option<&str>,
    ) -> Option<(RegistryImportMatch, Option<&DesignSystemComponent>)> {
        let targets = self.names.get(name)?;
        let mut tagged = targets.iter().filter(|target| target.package.is_some());
        let Some(first) = tagged.next() else {
            return Some((
                RegistryImportMatch::LegacyNameOnly,
                Some(&self.components[targets[0].component]),
            ));
        };
        let target = match package {
            Some(package) => std::iter::once(first)
                .chain(tagged)
                .find(|target| target.package.as_deref() == Some(package)),
            None => {
                if tagged.next().is_some() {
                    return None;
                }
                Some(first)
            }
        };
        let Some(target) = target else {
            return Some((RegistryImportMatch::Mismatch, None));
        };
        Some((
            resolve_import_aware_match(target.package.as_deref(), package),
            Some(&self.components[target.component]),
        ))
    }
}

/// Classifies a registry-backed usage site against the package implied by its import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryImportMatch {
    /// The import package exactly matches the registry package.
    Resolved,
    /// The registry package is present, but import evidence is missing or ambiguous.
    Candidate,
    /// Import evidence explicitly names a different package.
    Mismatch,
    /// The registry has no package and therefore uses legacy name-only matching.
    LegacyNameOnly,
}

/// Usage-site classification derived from a registry match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryUsageClassification {
    /// Resolution status of the usage.
    pub match_status: MatchStatus,
    /// Origin of the callee.
    pub callee_origin: CalleeOrigin,
    /// Evidence supporting the classification.
    pub evidence_kind: ResolutionEvidenceKind,
}

/// Classifies a registry match for a usage site.
///
/// `import_ambiguous` distinguishes ambiguous from missing import evidence for
/// candidates. `unresolved_origin` supplies the pack-specific origin only for
/// package mismatches and absent matches.
#[must_use]
pub fn classify_registry_usage(
    registry_match: Option<RegistryImportMatch>,
    import_ambiguous: bool,
    unresolved_origin: impl FnOnce() -> CalleeOrigin,
) -> RegistryUsageClassification {
    let (match_status, callee_origin, evidence_kind) = match registry_match {
        Some(RegistryImportMatch::Resolved) => (
            MatchStatus::Resolved,
            CalleeOrigin::Registry,
            ResolutionEvidenceKind::RegistryPackageMatch,
        ),
        Some(RegistryImportMatch::LegacyNameOnly) => (
            MatchStatus::Resolved,
            CalleeOrigin::Registry,
            ResolutionEvidenceKind::RegistryNameOnlyLegacy,
        ),
        Some(RegistryImportMatch::Candidate) => (
            MatchStatus::Candidate,
            CalleeOrigin::Registry,
            if import_ambiguous {
                ResolutionEvidenceKind::RegistryImportAmbiguous
            } else {
                ResolutionEvidenceKind::RegistryImportMissing
            },
        ),
        Some(RegistryImportMatch::Mismatch) => (
            MatchStatus::Unresolved,
            unresolved_origin(),
            ResolutionEvidenceKind::PackageMismatch,
        ),
        None => (
            MatchStatus::Unresolved,
            unresolved_origin(),
            ResolutionEvidenceKind::NoMatchingDefinition,
        ),
    };
    RegistryUsageClassification {
        match_status,
        callee_origin,
        evidence_kind,
    }
}

/// Classifies a registry-backed usage site against the package implied by its import.
#[must_use]
pub fn resolve_import_aware_match(
    registry_package: Option<&str>,
    import_package: Option<&str>,
) -> RegistryImportMatch {
    let Some(registry_package) = registry_package else {
        return RegistryImportMatch::LegacyNameOnly;
    };

    let Some(import_package) = import_package else {
        return RegistryImportMatch::Candidate;
    };

    if import_package == registry_package {
        RegistryImportMatch::Resolved
    } else {
        RegistryImportMatch::Mismatch
    }
}

/// Returns the npm package root for a module import specifier.
///
/// Examples: `@acme/design-system` -> `@acme/design-system`,
/// `@acme/design-system/button` -> `@acme/design-system`, `lodash/debounce` -> `lodash`.
#[must_use]
pub fn npm_import_package_root(specifier: &str) -> String {
    if let Some(rest) = specifier.strip_prefix('@') {
        let mut segments = rest.split('/');
        let scope = segments.next().unwrap_or("");
        let name = segments.next().unwrap_or("");
        if scope.is_empty() || name.is_empty() {
            return format!("@{rest}");
        }
        return format!("@{scope}/{name}");
    }

    specifier.split('/').next().unwrap_or(specifier).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_usage_classification_follows_the_parity_table() {
        use crate::classify_registry_usage;
        use wax_contract::{CalleeOrigin, MatchStatus, ResolutionEvidenceKind};

        let cases = [
            (
                Some(RegistryImportMatch::Resolved),
                false,
                MatchStatus::Resolved,
                CalleeOrigin::Registry,
                ResolutionEvidenceKind::RegistryPackageMatch,
            ),
            (
                Some(RegistryImportMatch::LegacyNameOnly),
                false,
                MatchStatus::Resolved,
                CalleeOrigin::Registry,
                ResolutionEvidenceKind::RegistryNameOnlyLegacy,
            ),
            (
                Some(RegistryImportMatch::Candidate),
                false,
                MatchStatus::Candidate,
                CalleeOrigin::Registry,
                ResolutionEvidenceKind::RegistryImportMissing,
            ),
            (
                Some(RegistryImportMatch::Candidate),
                true,
                MatchStatus::Candidate,
                CalleeOrigin::Registry,
                ResolutionEvidenceKind::RegistryImportAmbiguous,
            ),
            (
                Some(RegistryImportMatch::Mismatch),
                false,
                MatchStatus::Unresolved,
                CalleeOrigin::External,
                ResolutionEvidenceKind::PackageMismatch,
            ),
            (
                None,
                false,
                MatchStatus::Unresolved,
                CalleeOrigin::Application,
                ResolutionEvidenceKind::NoMatchingDefinition,
            ),
        ];

        for (registry_match, import_ambiguous, status, origin, evidence) in cases {
            let usage = classify_registry_usage(registry_match, import_ambiguous, || {
                assert_ne!(origin, CalleeOrigin::Registry, "{registry_match:?}");
                origin
            });
            assert_eq!(
                (usage.match_status, usage.callee_origin, usage.evidence_kind),
                (status, origin, evidence),
                "{registry_match:?}, import_ambiguous={import_ambiguous}"
            );
        }
    }

    #[test]
    fn import_aware_match_resolves_only_matching_ds_imports() {
        assert_eq!(
            resolve_import_aware_match(
                Some("com.acme.designsystem"),
                Some("com.acme.designsystem"),
            ),
            RegistryImportMatch::Resolved
        );
        assert_eq!(
            resolve_import_aware_match(Some("com.acme.designsystem"), Some("com.foundation.ui"),),
            RegistryImportMatch::Mismatch
        );
        assert_eq!(
            resolve_import_aware_match(Some("com.acme.designsystem"), None),
            RegistryImportMatch::Candidate
        );
        assert_eq!(
            resolve_import_aware_match(None, Some("SwiftUI")),
            RegistryImportMatch::LegacyNameOnly
        );
    }

    #[test]
    fn npm_import_package_root_handles_scoped_packages() {
        assert_eq!(
            npm_import_package_root("@acme/design-system"),
            "@acme/design-system"
        );
        assert_eq!(
            npm_import_package_root("@acme/design-system/button"),
            "@acme/design-system"
        );
        assert_eq!(npm_import_package_root("lodash/debounce"), "lodash");
    }
}
