//! Shared import-aware registry resolution helpers for parser-backed language packs.

use wax_contract::MatchStatus;

/// Classifies a registry-backed usage site when the component declares an optional `package`.
///
/// When `registry_package` is absent, returns `None` so packs can apply legacy resolution rules.
#[must_use]
pub fn resolve_import_aware_match(
    registry_package: Option<&str>,
    import_package: Option<&str>,
) -> Option<MatchStatus> {
    let registry_package = registry_package?;

    let Some(import_package) = import_package else {
        return Some(MatchStatus::Candidate);
    };

    if import_package == registry_package {
        Some(MatchStatus::Resolved)
    } else {
        None
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
            Some(MatchStatus::Resolved)
        );
        assert_eq!(
            resolve_import_aware_match(Some("com.acme.designsystem"), Some("com.foundation.ui"),),
            None
        );
        assert_eq!(
            resolve_import_aware_match(Some("com.acme.designsystem"), None),
            Some(MatchStatus::Candidate)
        );
        assert_eq!(resolve_import_aware_match(None, Some("SwiftUI")), None);
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
