//! React registry symbol loading and resolver index.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use wax_contract::DesignSystemComponent;

const REGISTRY_SCHEMA_VERSION: u64 = 1;

/// React registry resolver index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactRegistryIndex {
    /// Design-system components available to React.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Map from any observed name (symbol or alias) to canonical registry symbol.
    pub resolve_targets: BTreeMap<String, String>,
    /// Optional import package per canonical registry symbol.
    pub component_packages: BTreeMap<String, Option<String>>,
}

/// Kind of registry load failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryErrorKind {
    /// Configured registry file does not exist.
    NotFound,
    /// Registry exists but is malformed or incompatible with React scanning.
    Invalid,
}

/// Errors while loading the React registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    kind: RegistryErrorKind,
    reason: String,
}

impl RegistryError {
    fn not_found(reason: impl Into<String>) -> Self {
        Self {
            kind: RegistryErrorKind::NotFound,
            reason: reason.into(),
        }
    }

    fn invalid(reason: impl Into<String>) -> Self {
        Self {
            kind: RegistryErrorKind::Invalid,
            reason: reason.into(),
        }
    }

    /// Returns the registry failure category.
    #[must_use]
    pub fn kind(&self) -> RegistryErrorKind {
        self.kind
    }

    /// Returns the human-readable registry load failure.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for RegistryError {}

/// Loads registry symbols and aliases into a React resolver index.
pub fn load_react_registry(path: &Path) -> Result<ReactRegistryIndex, RegistryError> {
    let raw = fs::read_to_string(path).map_err(|err| registry_read_error(path, err))?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
        RegistryError::invalid(format!(
            "registry JSON is invalid at {}: {err}",
            path.display()
        ))
    })?;
    validate_schema_version(&value, path)?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            RegistryError::invalid(format!(
                "registry JSON at {} must contain a components array",
                path.display()
            ))
        })?;

    let mut design_system_components = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    let mut component_packages = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if !component_available_to_react(component, index)? {
            continue;
        }

        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                RegistryError::invalid(format!("components[{index}] is missing symbol"))
            })?;
        let package = component
            .get("package")
            .map(|value| {
                value.as_str().ok_or_else(|| {
                    RegistryError::invalid(format!("components[{index}].package must be a string"))
                })
            })
            .transpose()?
            .map(str::to_owned);
        if let Some(package) = &package
            && package.is_empty()
        {
            return Err(RegistryError::invalid(format!(
                "components[{index}].package must not be empty"
            )));
        }

        design_system_components.push(DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.to_owned(),
            registry_symbol: symbol.to_owned(),
        });
        resolve_targets.insert(symbol.to_owned(), symbol.to_owned());
        component_packages.insert(symbol.to_owned(), package);
        if let Some(aliases) = component
            .get("aliases")
            .and_then(serde_json::Value::as_array)
        {
            for (alias_index, alias) in aliases.iter().enumerate() {
                let alias_symbol = alias.as_str().ok_or_else(|| {
                    RegistryError::invalid(format!(
                        "components[{index}].aliases[{alias_index}] must be a string"
                    ))
                })?;
                resolve_targets.insert(alias_symbol.to_owned(), symbol.to_owned());
            }
        }
    }

    if design_system_components.is_empty() {
        return Err(RegistryError::invalid(format!(
            "registry at {} must declare at least one React component symbol",
            path.display()
        )));
    }

    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    Ok(ReactRegistryIndex {
        design_system_components,
        resolve_targets,
        component_packages,
    })
}

fn registry_read_error(path: &Path, err: io::Error) -> RegistryError {
    if err.kind() == io::ErrorKind::NotFound {
        RegistryError::not_found(format!(
            "design-system registry not found at {}: {err}",
            path.display()
        ))
    } else {
        RegistryError::invalid(format!(
            "failed to read design-system registry {}: {err}",
            path.display()
        ))
    }
}

fn validate_schema_version(value: &serde_json::Value, path: &Path) -> Result<(), RegistryError> {
    let Some(schema_version) = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    else {
        return Err(RegistryError::invalid(format!(
            "registry JSON at {} must contain schema_version {}",
            path.display(),
            REGISTRY_SCHEMA_VERSION
        )));
    };
    if schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(RegistryError::invalid(format!(
            "registry JSON at {} has unsupported schema_version {schema_version}; expected {}",
            path.display(),
            REGISTRY_SCHEMA_VERSION
        )));
    }
    Ok(())
}

fn component_available_to_react(
    component: &serde_json::Value,
    index: usize,
) -> Result<bool, RegistryError> {
    let Some(targets_value) = component.get("targets") else {
        return Ok(true);
    };
    if targets_value.is_null() {
        return Ok(true);
    }
    let Some(targets) = targets_value.as_array() else {
        return Err(RegistryError::invalid(format!(
            "components[{index}].targets must be an array of strings"
        )));
    };
    for (target_index, target) in targets.iter().enumerate() {
        let Some(target) = target.as_str() else {
            return Err(RegistryError::invalid(format!(
                "components[{index}].targets[{target_index}] must be a string"
            )));
        };
        if target == "react" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct RegistryFixture {
        file: tempfile::NamedTempFile,
    }

    impl RegistryFixture {
        fn write(contents: &str) -> Self {
            let mut file =
                tempfile::NamedTempFile::new().expect("registry fixture should be created");
            file.write_all(contents.as_bytes())
                .expect("registry fixture should be written");
            Self { file }
        }

        fn path(&self) -> &Path {
            self.file.path()
        }
    }

    #[test]
    fn registry_loads_symbols_when_targets_are_omitted() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button"}]}"#,
        );

        let index = load_react_registry(fixture.path()).expect("registry should load");
        assert_eq!(index.design_system_components.len(), 1);
        assert_eq!(index.design_system_components[0].symbol, "Button");
        assert_eq!(
            index.resolve_targets.get("Button"),
            Some(&"Button".to_owned())
        );
    }

    #[test]
    fn registry_loads_symbols_when_targets_are_null() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":null}]}"#,
        );

        let index = load_react_registry(fixture.path()).expect("registry should load");
        assert_eq!(index.design_system_components.len(), 1);
        assert_eq!(index.design_system_components[0].symbol, "Button");
    }

    #[test]
    fn registry_loads_symbols_when_targets_include_react() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        );

        let index = load_react_registry(fixture.path()).expect("registry should load");
        assert_eq!(index.design_system_components.len(), 1);
        assert_eq!(index.design_system_components[0].symbol, "Button");
    }

    #[test]
    fn registry_excludes_symbols_when_targets_exclude_react() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["compose"]}]}"#,
        );

        let err =
            load_react_registry(fixture.path()).expect_err("compose-only registry should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(
            err.reason().contains("at least one React component symbol"),
            "unexpected error: {}",
            err.reason()
        );
    }

    #[test]
    fn registry_builds_alias_maps_for_react_components() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","aliases":["PrimaryButton"],"targets":["react","compose"]}]}"#,
        );

        let index = load_react_registry(fixture.path()).expect("registry should load");
        assert_eq!(
            index.resolve_targets.get("PrimaryButton"),
            Some(&"Button".to_owned())
        );
    }

    #[test]
    fn registry_rejects_missing_components_array() {
        let fixture = RegistryFixture::write(r#"{"schema_version":1}"#);

        let err = load_react_registry(fixture.path()).expect_err("missing components should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("components array"));
    }

    #[test]
    fn registry_rejects_non_string_symbol() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":42}]}"#,
        );

        let err = load_react_registry(fixture.path()).expect_err("non-string symbol should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("missing symbol"));
    }

    #[test]
    fn registry_rejects_malformed_json() {
        let fixture = RegistryFixture::write("{not-json");

        let err = load_react_registry(fixture.path()).expect_err("malformed JSON should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("registry JSON is invalid"));
    }

    #[test]
    fn registry_rejects_missing_file() {
        let path = std::path::Path::new("/tmp/wax-react-missing-registry.json");

        let err = load_react_registry(path).expect_err("missing registry file should fail");
        assert_eq!(err.kind(), RegistryErrorKind::NotFound);
        assert!(err.reason().contains("design-system registry not found"));
    }

    #[test]
    fn registry_rejects_missing_schema_version() {
        let fixture = RegistryFixture::write(
            r#"{"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        );

        let err =
            load_react_registry(fixture.path()).expect_err("missing schema_version should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("schema_version"));
    }

    #[test]
    fn registry_rejects_unsupported_schema_version() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":2,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        );

        let err = load_react_registry(fixture.path())
            .expect_err("unsupported schema_version should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("unsupported schema_version 2"));
    }

    #[test]
    fn registry_rejects_string_targets() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":"react"}]}"#,
        );

        let err = load_react_registry(fixture.path()).expect_err("string targets should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("targets must be an array of strings"));
    }

    #[test]
    fn registry_rejects_non_string_target_entries() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":[42,"react"]}]}"#,
        );

        let err =
            load_react_registry(fixture.path()).expect_err("non-string target entry should fail");
        assert_eq!(err.kind(), RegistryErrorKind::Invalid);
        assert!(err.reason().contains("targets[0] must be a string"));
    }
}
