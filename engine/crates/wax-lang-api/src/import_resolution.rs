//! Shared import-aware registry resolution helpers for parser-backed language packs.

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
