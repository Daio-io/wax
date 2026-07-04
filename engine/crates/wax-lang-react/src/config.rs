//! React scan configuration parsing and validation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wax_lang_api::ScanConfig;

/// Parsed React scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative React source roots to scan.
    pub roots: Vec<PathBuf>,
    /// Repo-relative ignore glob patterns applied during source collection.
    pub ignore: Vec<String>,
    /// Optional repo-relative TypeScript config path for resolver hints.
    pub tsconfig: Option<PathBuf>,
    /// Explicit import alias mappings to repo-relative source targets.
    pub aliases: BTreeMap<String, Vec<String>>,
    /// Configured design-system package entrypoint hints.
    pub packages: BTreeMap<String, PackageConfig>,
}

/// Configured source mappings for a design-system package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageConfig {
    /// Package export specifier to repo-relative source target mappings.
    pub exports: BTreeMap<String, String>,
}

/// Whether the request should run the React scanner or return scaffold facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactConfigMode {
    /// No React scan keys were provided.
    Scaffold,
    /// Registry and roots were provided and validated.
    Configured(ReactScanConfig),
}

/// Loads React scan settings from the engine request payload.
pub fn parse_react_scan_config(config: &ScanConfig) -> Result<ReactConfigMode, ConfigError> {
    let has_registry = config.contains_key("registry");
    let has_roots = config.contains_key("roots");
    let has_react_only_config = config.contains_key("ignore")
        || config.contains_key("excludes")
        || config.contains_key("tsconfig")
        || config.contains_key("aliases")
        || config.contains_key("packages");
    if !has_registry && !has_roots && !has_react_only_config {
        return Ok(ReactConfigMode::Scaffold);
    }

    let registry = string_field(
        config,
        &["registry"],
        "registry is required when react scan config is present",
        "registry",
    )?;
    validate_repo_relative_path(registry, "registry")?;

    let roots = string_array_field(config, "roots", true)?;
    for (index, root) in roots.iter().enumerate() {
        validate_repo_relative_path(root, &format!("roots[{index}]"))?;
    }

    let mut ignore = optional_string_array_field(config, "ignore")?;
    for (index, pattern) in ignore.iter().enumerate() {
        validate_repo_relative_path(pattern, &format!("ignore[{index}]"))?;
    }
    let excludes = optional_string_array_field(config, "excludes")?;
    for (index, pattern) in excludes.iter().enumerate() {
        validate_repo_relative_path(pattern, &format!("excludes[{index}]"))?;
    }
    ignore.extend(excludes);

    let tsconfig = optional_string_field(config, "tsconfig")?
        .map(|path| {
            validate_repo_relative_path(path, "tsconfig")?;
            Ok::<PathBuf, ConfigError>(PathBuf::from(path))
        })
        .transpose()?;

    let aliases = parse_aliases(config)?;
    let packages = parse_packages(config)?;

    Ok(ReactConfigMode::Configured(ReactScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots: roots.into_iter().map(PathBuf::from).collect(),
        ignore,
        tsconfig,
        aliases,
        packages,
    }))
}

/// React config parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    reason: String,
}

impl ConfigError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// Returns the human-readable validation failure.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for ConfigError {}

fn string_field<'a>(
    config: &'a ScanConfig,
    keys: &[&str],
    missing_message: &str,
    label: &str,
) -> Result<&'a str, ConfigError> {
    let value = keys
        .iter()
        .find_map(|key| config.get(*key))
        .ok_or_else(|| ConfigError::new(missing_message))?;
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ConfigError::new(format!("{label} must be a non-empty string")))
}

fn optional_string_field<'a>(
    config: &'a ScanConfig,
    key: &str,
) -> Result<Option<&'a str>, ConfigError> {
    config
        .get(key)
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ConfigError::new(format!("{key} must be a non-empty string")))
        })
        .transpose()
}

fn string_array_field(
    config: &ScanConfig,
    key: &str,
    required: bool,
) -> Result<Vec<String>, ConfigError> {
    let Some(value) = config.get(key) else {
        if required {
            return Err(ConfigError::new(format!(
                "{key} is required when react scan config is present"
            )));
        }
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| ConfigError::new(format!("{key} must be a non-empty array of strings")))?;
    if array.is_empty() && required {
        return Err(ConfigError::new(format!(
            "{key} must be a non-empty array of strings"
        )));
    }
    array
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    ConfigError::new(format!("{key}[{index}] must be a non-empty string"))
                })
        })
        .collect()
}

fn optional_string_array_field(config: &ScanConfig, key: &str) -> Result<Vec<String>, ConfigError> {
    string_array_field(config, key, false)
}

fn parse_aliases(config: &ScanConfig) -> Result<BTreeMap<String, Vec<String>>, ConfigError> {
    let Some(value) = config.get("aliases") else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| ConfigError::new("aliases must be an object of string arrays"))?;
    let mut aliases = BTreeMap::new();
    for (alias, value) in object {
        let targets = value
            .as_array()
            .ok_or_else(|| ConfigError::new(format!("aliases.{alias} must be a string array")))?;
        if targets.is_empty() {
            return Err(ConfigError::new(format!(
                "aliases.{alias} must be a non-empty string array"
            )));
        }
        let mut parsed_targets = Vec::with_capacity(targets.len());
        for (index, target) in targets.iter().enumerate() {
            let target = target
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ConfigError::new(format!(
                        "aliases.{alias}[{index}] must be a non-empty string"
                    ))
                })?;
            validate_repo_relative_path(target, &format!("aliases.{alias}[{index}]"))?;
            parsed_targets.push(target.to_owned());
        }
        aliases.insert(alias.clone(), parsed_targets);
    }
    Ok(aliases)
}

fn parse_packages(config: &ScanConfig) -> Result<BTreeMap<String, PackageConfig>, ConfigError> {
    let Some(value) = config.get("packages") else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| ConfigError::new("packages must be an object"))?;
    let mut packages = BTreeMap::new();
    for (package_name, value) in object {
        let package = value.as_object().ok_or_else(|| {
            ConfigError::new(format!("packages.{package_name} must be an object"))
        })?;
        let exports_value = package.get("exports").ok_or_else(|| {
            ConfigError::new(format!("packages.{package_name}.exports is required"))
        })?;
        let exports_object = exports_value.as_object().ok_or_else(|| {
            ConfigError::new(format!(
                "packages.{package_name}.exports must be an object of strings"
            ))
        })?;
        if exports_object.is_empty() {
            return Err(ConfigError::new(format!(
                "packages.{package_name}.exports must not be empty"
            )));
        }
        let mut exports = BTreeMap::new();
        for (export_name, target) in exports_object {
            let target = target
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ConfigError::new(format!(
                        "packages.{package_name}.exports.{export_name} must be a non-empty string"
                    ))
                })?;
            validate_repo_relative_path(
                target,
                &format!("packages.{package_name}.exports.{export_name}"),
            )?;
            exports.insert(export_name.clone(), target.to_owned());
        }
        packages.insert(package_name.clone(), PackageConfig { exports });
    }
    Ok(packages)
}

fn validate_repo_relative_path(path: &str, field: &str) -> Result<(), ConfigError> {
    if path.starts_with('/') || path.starts_with('\\') || has_windows_drive_prefix(path) {
        return Err(ConfigError::new(format!(
            "{field} must be a repo-relative path"
        )));
    }
    if path.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(ConfigError::new(format!(
            "{field} must not contain parent directory segments"
        )));
    }
    Ok(())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && is_separator(bytes[2])
}

fn is_separator(byte: u8) -> bool {
    byte == b'/' || byte == b'\\'
}
