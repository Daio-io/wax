//! Shared import-aware registry resolution helpers for parser-backed language packs.

use wax_contract::MatchStatus;

/// Returns whether `package` equals `prefix` or is a subpackage of `prefix`.
#[must_use]
pub fn package_matches_prefix(package: &str, prefix: &str) -> bool {
    package == prefix
        || package
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Returns whether `import_package` is imported from a configured framework package prefix.
#[must_use]
pub fn import_matches_framework_package(
    import_package: &str,
    framework_packages: &[String],
) -> bool {
    framework_packages
        .iter()
        .any(|framework_package| package_matches_prefix(import_package, framework_package))
}

/// Classifies a registry-backed usage site when the component declares an optional `package`.
///
/// When `registry_package` is absent, returns `None` so packs can apply legacy resolution rules.
#[must_use]
pub fn resolve_import_aware_match(
    registry_package: Option<&str>,
    import_package: Option<&str>,
    framework_packages: &[String],
) -> Option<MatchStatus> {
    let registry_package = registry_package?;

    let Some(import_package) = import_package else {
        return Some(MatchStatus::Candidate);
    };

    if import_package == registry_package {
        return Some(MatchStatus::Resolved);
    }
    if import_matches_framework_package(import_package, framework_packages) {
        return Some(MatchStatus::FrameworkShadow);
    }

    None
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

/// Parses optional `framework_packages` from a scan config payload fragment.
pub fn parse_framework_packages(
    value: &serde_json::Value,
) -> Result<Vec<String>, FrameworkPackagesParseError> {
    let array = value
        .as_array()
        .ok_or_else(|| FrameworkPackagesParseError {
            reason: "framework_packages must be an array of strings".to_owned(),
        })?;
    let mut packages = Vec::with_capacity(array.len());
    for (index, entry) in array.iter().enumerate() {
        let package = entry
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| FrameworkPackagesParseError {
                reason: format!("framework_packages[{index}] must be a non-empty string"),
            })?;
        packages.push(package.to_owned());
    }
    packages.sort();
    packages.dedup();
    Ok(packages)
}

/// Error while parsing `framework_packages` from scan config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkPackagesParseError {
    /// Human-readable validation failure.
    pub reason: String,
}

impl std::fmt::Display for FrameworkPackagesParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid framework_packages: {}", self.reason)
    }
}

impl std::error::Error for FrameworkPackagesParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_prefix_matches_subpackages() {
        assert!(package_matches_prefix(
            "androidx.compose.material3",
            "androidx.compose"
        ));
        assert!(!package_matches_prefix(
            "androidx.composefoo",
            "androidx.compose"
        ));
    }

    #[test]
    fn import_aware_match_resolves_ds_and_framework_imports() {
        assert_eq!(
            resolve_import_aware_match(
                Some("com.acme.designsystem"),
                Some("com.acme.designsystem"),
                &["com.foundation.ui".to_owned()],
            ),
            Some(MatchStatus::Resolved)
        );
        assert_eq!(
            resolve_import_aware_match(
                Some("com.acme.designsystem"),
                Some("com.foundation.ui"),
                &["com.foundation.ui".to_owned()],
            ),
            Some(MatchStatus::FrameworkShadow)
        );
        assert_eq!(
            resolve_import_aware_match(
                Some("com.acme.designsystem"),
                None,
                &["com.foundation.ui".to_owned()],
            ),
            Some(MatchStatus::Candidate)
        );
        assert_eq!(
            resolve_import_aware_match(None, Some("SwiftUI"), &["SwiftUI".to_owned()]),
            None
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
