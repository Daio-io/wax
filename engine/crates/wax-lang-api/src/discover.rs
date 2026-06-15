//! Shared registry discovery types for language packs and the engine.

use serde::{Deserialize, Serialize};

/// One design-system component symbol discovered from source, with optional package identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredRegistrySymbol {
    /// Public component symbol name.
    pub symbol: String,
    /// Design-system package identity when the language pack can infer it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

impl DiscoveredRegistrySymbol {
    /// Creates one discovered symbol record.
    #[must_use]
    pub fn new(symbol: impl Into<String>, package: Option<String>) -> Self {
        Self {
            symbol: symbol.into(),
            package,
        }
    }

    /// Returns symbol names in discovery order.
    #[must_use]
    pub fn symbol_names(components: &[Self]) -> Vec<String> {
        components
            .iter()
            .map(|component| component.symbol.clone())
            .collect()
    }

    /// Builds symbol-only records for backward-compatible wire responses.
    #[must_use]
    pub fn from_symbol_names(symbols: &[String]) -> Vec<Self> {
        symbols
            .iter()
            .map(|symbol| Self::new(symbol.clone(), None))
            .collect()
    }
}

/// Returns structured discover components, falling back to symbol-only records.
#[must_use]
pub fn normalize_discovered_components(
    symbols: Vec<String>,
    components: Vec<DiscoveredRegistrySymbol>,
) -> Vec<DiscoveredRegistrySymbol> {
    if components.is_empty() {
        DiscoveredRegistrySymbol::from_symbol_names(&symbols)
    } else {
        components
    }
}

/// Resolves the npm package name for discovery roots by walking upward for `package.json`.
#[must_use]
pub fn npm_package_name_for_roots(
    repo_root: &std::path::Path,
    roots: &[std::path::PathBuf],
) -> Option<String> {
    for root in roots {
        if let Some(name) = npm_package_name_for_path(repo_root, root) {
            return Some(name);
        }
    }

    None
}

/// Resolves the npm package name for one source file or directory by walking upward for `package.json`.
#[must_use]
pub fn npm_package_name_for_path(
    repo_root: &std::path::Path,
    path: &std::path::Path,
) -> Option<String> {
    let mut current = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };

    loop {
        let package_json = current.join("package.json");
        if package_json.is_file()
            && let Ok(contents) = std::fs::read_to_string(&package_json)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
            && let Some(name) = value.get("name").and_then(serde_json::Value::as_str)
            && !name.is_empty()
        {
            return Some(name.to_owned());
        }

        if current == repo_root {
            break;
        }
        current = current.parent()?;
    }

    None
}

/// Infers a Swift module name from a source file path under `Sources/<Module>/`.
#[must_use]
pub fn swift_module_from_source_path(file_path: &std::path::Path) -> Option<String> {
    let parent = file_path.parent()?;
    if parent.file_name()?.to_str()? != "Sources" {
        let grandparent = parent.parent()?;
        if grandparent.file_name()?.to_str()? == "Sources" {
            return parent.file_name()?.to_str().map(str::to_owned);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn npm_package_name_is_found_above_discovery_root() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let package_dir = tempdir.path().join("packages/design-system");
        std::fs::create_dir_all(package_dir.join("src")).unwrap();
        std::fs::write(
            package_dir.join("package.json"),
            r#"{"name":"@acme/design-system"}"#,
        )
        .unwrap();

        let name = npm_package_name_for_roots(tempdir.path(), &[package_dir.join("src")]);
        assert_eq!(name.as_deref(), Some("@acme/design-system"));
    }

    #[test]
    fn npm_package_name_is_inferred_from_source_file_path() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let package_dir = tempdir.path().join("packages/design-system");
        std::fs::create_dir_all(package_dir.join("src")).unwrap();
        std::fs::write(
            package_dir.join("package.json"),
            r#"{"name":"@acme/design-system"}"#,
        )
        .unwrap();
        let source_file = package_dir.join("src/Button.tsx");

        let name = npm_package_name_for_path(tempdir.path(), &source_file);
        assert_eq!(name.as_deref(), Some("@acme/design-system"));
    }

    #[test]
    fn swift_module_is_inferred_from_sources_layout() {
        let path = PathBuf::from("design-system/Sources/AcmeDesignSystem/Button.swift");
        assert_eq!(
            swift_module_from_source_path(&path).as_deref(),
            Some("AcmeDesignSystem")
        );
    }
}
