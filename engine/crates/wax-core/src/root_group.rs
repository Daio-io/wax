//! Core-owned root-group attribution for location-bearing scan facts.

use std::collections::BTreeMap;
use std::path::Path;

use wax_contract::{LanguageId, ScanFacts, SourceLocation};
use wax_lang_api::{normalize_repo_relative_path, path_matches_any};

/// Configured root groups keyed by repository-wide group id and language id.
pub type RootGroups = BTreeMap<String, BTreeMap<LanguageId, Vec<String>>>;

/// Applies the first deterministic matching root group to every location-bearing fact.
pub fn attribute_scan_facts(facts: &mut ScanFacts, language_id: &LanguageId, groups: &RootGroups) {
    for component in &mut facts.local_components {
        attribute_location(&mut component.location, language_id, groups);
    }
    for site in &mut facts.usage_sites {
        attribute_location(&mut site.location, language_id, groups);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, groups);
        }
    }
    for site in &mut facts.token_sites {
        attribute_location(&mut site.location, language_id, groups);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, groups);
        }
    }
    for site in &mut facts.hardcoded_style_sites {
        attribute_location(&mut site.location, language_id, groups);
        if let Some(parent) = &mut site.parent
            && let Some(location) = &mut parent.location
        {
            attribute_location(location, language_id, groups);
        }
    }
    for diagnostic in &mut facts.diagnostics {
        if let Some(location) = &mut diagnostic.location {
            attribute_location(location, language_id, groups);
        }
    }
}

fn attribute_location(
    location: &mut SourceLocation,
    language_id: &LanguageId,
    groups: &RootGroups,
) {
    location.root_group = None;
    let path = normalize_repo_relative_path(Path::new(&location.file));
    for (group, languages) in groups {
        let Some(roots) = languages.get(language_id) else {
            continue;
        };
        let patterns = roots
            .iter()
            .map(|root| root_file_pattern(root))
            .collect::<Vec<_>>();
        if path_matches_any(&path, &patterns) {
            location.root_group = Some(group.clone());
            return;
        }
    }
}

fn root_file_pattern(root: &str) -> String {
    let normalized = normalize_repo_relative_path(Path::new(root));
    if normalized.ends_with("/**") {
        normalized
    } else {
        format!("{normalized}/**")
    }
}
