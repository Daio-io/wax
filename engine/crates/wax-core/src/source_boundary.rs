//! Core-owned source-boundary attribution for location-bearing scan facts.

use std::path::Path;

use wax_contract::{LanguageId, ScanFacts, SourceLocation};
use wax_lang_api::{normalize_repo_relative_path, path_matches_any};

use crate::config::waxrc::SourceBoundaryConfig;

/// Applies the first matching configured boundary to every location-bearing fact.
pub fn attribute_scan_facts(
    facts: &mut ScanFacts,
    language_id: &LanguageId,
    boundaries: &[SourceBoundaryConfig],
) {
    for component in &mut facts.local_components {
        attribute_location(&mut component.location, language_id, boundaries);
    }
    for site in &mut facts.usage_sites {
        attribute_location(&mut site.location, language_id, boundaries);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, boundaries);
        }
    }
    for site in &mut facts.token_sites {
        attribute_location(&mut site.location, language_id, boundaries);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, boundaries);
        }
    }
    for site in &mut facts.hardcoded_style_sites {
        attribute_location(&mut site.location, language_id, boundaries);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, boundaries);
        }
    }
    for diagnostic in &mut facts.diagnostics {
        if let Some(location) = &mut diagnostic.location {
            attribute_location(location, language_id, boundaries);
        }
    }
}

fn attribute_location(
    location: &mut SourceLocation,
    language_id: &LanguageId,
    boundaries: &[SourceBoundaryConfig],
) {
    location.boundary_id = None;
    for boundary in boundaries {
        if !language_matches(boundary, language_id) {
            continue;
        }
        let path = normalize_repo_relative_path(Path::new(&location.file));
        let includes = normalized_patterns(&boundary.include);
        let excludes = normalized_patterns(&boundary.exclude);
        if !path_matches_any(&path, &includes) {
            continue;
        }
        if path_matches_any(&path, &excludes) {
            break;
        }
        location.boundary_id = Some(boundary.id.clone());
        break;
    }
}

fn language_matches(boundary: &SourceBoundaryConfig, language_id: &LanguageId) -> bool {
    boundary
        .languages
        .as_ref()
        .is_none_or(|languages| languages.iter().any(|id| id == language_id))
}

fn normalized_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| normalize_repo_relative_path(Path::new(pattern)))
        .collect()
}
