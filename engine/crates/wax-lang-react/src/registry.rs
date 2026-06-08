//! React registry symbol loading and resolver index.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use wax_contract::DesignSystemComponent;

/// React registry resolver index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactRegistryIndex {
    /// Design-system components available to React.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Map from any observed name (symbol or alias) to canonical registry symbol.
    pub resolve_targets: BTreeMap<String, String>,
}

/// Errors while loading the React registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    reason: String,
}

impl RegistryError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
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
    let raw = fs::read_to_string(path).map_err(|err| {
        RegistryError::new(format!(
            "failed to read design-system registry {}: {err}",
            path.display()
        ))
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
        RegistryError::new(format!(
            "registry JSON is invalid at {}: {err}",
            path.display()
        ))
    })?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            RegistryError::new(format!(
                "registry JSON at {} must contain a components array",
                path.display()
            ))
        })?;

    let mut design_system_components = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if !component_available_to_react(component) {
            continue;
        }

        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| RegistryError::new(format!("components[{index}] is missing symbol")))?;
        design_system_components.push(DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.to_owned(),
            registry_symbol: symbol.to_owned(),
        });
        resolve_targets.insert(symbol.to_owned(), symbol.to_owned());
        if let Some(aliases) = component
            .get("aliases")
            .and_then(serde_json::Value::as_array)
        {
            for (alias_index, alias) in aliases.iter().enumerate() {
                let alias_symbol = alias.as_str().ok_or_else(|| {
                    RegistryError::new(format!(
                        "components[{index}].aliases[{alias_index}] must be a string"
                    ))
                })?;
                resolve_targets.insert(alias_symbol.to_owned(), symbol.to_owned());
            }
        }
    }

    if design_system_components.is_empty() {
        return Err(RegistryError::new(format!(
            "registry at {} must declare at least one React component symbol",
            path.display()
        )));
    }

    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    Ok(ReactRegistryIndex {
        design_system_components,
        resolve_targets,
    })
}

fn component_available_to_react(component: &serde_json::Value) -> bool {
    match component.get("targets") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Array(targets)) => targets
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|target| target == "react"),
        Some(_) => false,
    }
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
        assert!(err.reason().contains("components array"));
    }

    #[test]
    fn registry_rejects_non_string_symbol() {
        let fixture = RegistryFixture::write(
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":42}]}"#,
        );

        let err = load_react_registry(fixture.path()).expect_err("non-string symbol should fail");
        assert!(err.reason().contains("missing symbol"));
    }

    #[test]
    fn registry_rejects_malformed_json() {
        let fixture = RegistryFixture::write("{not-json");

        let err = load_react_registry(fixture.path()).expect_err("malformed JSON should fail");
        assert!(err.reason().contains("registry JSON is invalid"));
    }
}
