use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wax_contract::{LanguageId, MergedScan, ScanFacts, TokenInferenceClassification};
use wax_core::paths::WaxHome;
use wax_core::{AtomicWriteError, Engine, EngineError, ScanOptions};

struct ScanOutputFixture {
    repo: PathBuf,
    wax_home: PathBuf,
}

impl ScanOutputFixture {
    fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            wax_home: Some(WaxHome::new(&self.wax_home)),
            ..ScanOptions::default()
        }
    }
}

fn fixture(name: &str, languages: &[&str]) -> ScanOutputFixture {
    let root = temp_dir(name);
    let repo = root.join("repo");
    let wax_home = root.join("wax-home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&wax_home).unwrap();

    write_waxrc(&repo, languages);
    write_default_registry(&repo, languages);
    write_lockfile(&repo, languages);
    write_installed_packs(&wax_home, languages);

    ScanOutputFixture { repo, wax_home }
}

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

const DEFAULT_REGISTRY_JSON: &str =
    r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#;

fn language_registry_relative_path(language: &str) -> String {
    format!(".wax/{language}.registry.json")
}

fn write_default_registry(repo: &Path, languages: &[&str]) {
    fs::create_dir_all(repo.join(".wax")).unwrap();
    for language in languages {
        fs::write(
            repo.join(language_registry_relative_path(language)),
            DEFAULT_REGISTRY_JSON,
        )
        .unwrap();
    }
}

fn write_waxrc(repo: &Path, languages: &[&str]) {
    let languages = languages
        .iter()
        .map(|language| format!(r#"    "{language}": {{}}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine": {{ "scan_concurrency": 2 }},
  "languages": {{
{languages}
  }}
}}"#
        ),
    )
    .unwrap();
}

fn write_lockfile(repo: &Path, languages: &[&str]) {
    let registry_entries = languages
        .iter()
        .map(|language| {
            let path = language_registry_relative_path(language);
            let registry_sha256 = file_sha256(&repo.join(&path));
            format!(
                r#"    "{language}": {{
      "source": "{path}",
      "sha256": "{registry_sha256}"
    }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let entries = languages
        .iter()
        .map(|language| {
            format!(
                r#"    "{language}": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "https://registry.example.invalid/{language}.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/{language}-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo.join(".wax/wax.lock.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "registries": {{
{registry_entries}
  }},
  "languages": {{
{entries}
  }}
}}"#
        ),
    )
    .unwrap();
}

fn file_sha256(path: &Path) -> String {
    let digest = Sha256::digest(fs::read(path).unwrap());
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn write_installed_packs(wax_home: &Path, languages: &[&str]) {
    let mut state_entries = Vec::new();
    for language in languages {
        let install_dir = wax_home.join(format!("langs/{language}/0.1.0"));
        fs::create_dir_all(&install_dir).unwrap();
        let script = install_dir.join("pack.sh");
        fs::write(&script, fixture_script()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }
        fs::write(
            install_dir.join("manifest.json"),
            format!(
                r#"{{
  "id": "{language}",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./pack.sh", "{language}"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}"#
            ),
        )
        .unwrap();
        state_entries.push(format!(
            r#"    "{language}": {{
      "0.1.0": {{ "install_dir": "{}" }}
    }}"#,
            install_dir.display()
        ));
    }

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
{}
  }}
}}"#,
            state_entries.join(",\n")
        ),
    )
    .unwrap();
}

fn fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
case "$language" in
  compose) parse_extract_ms=5 ;;
  react) parse_extract_ms=3 ;;
  *) parse_extract_ms=7 ;;
esac
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":5,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-$language","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":$parse_extract_ms,"files_scanned":1},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0}}}}
JSON
"#
}

fn token_fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":5,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-$language","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":11,"files_scanned":1},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0},"tokens":{"configured_token_count":1,"used_token_count":1,"token_reference_site_count":1,"hardcoded_style_candidate_count":1,"token_references_by_category":{"color":1,"spacing":0,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"hardcoded_by_category":{"color":0,"spacing":1,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"parent_scopes_with_token_references":0,"parent_scopes_with_hardcoded_candidates":0}},"design_system_tokens":[{"id":"color.primary","key":"Theme.colors.primary","category":"color"}],"token_sites":[{"id":"token.compose:src/Screen.kt:1:1:color.primary","location":{"file":"src/Screen.kt","line":1,"column":1},"token_id":"color.primary","key":"Theme.colors.primary","category":"color"}],"hardcoded_style_sites":[{"id":"hardcoded.compose:src/Screen.kt:2:12:spacing","location":{"file":"src/Screen.kt","line":2,"column":12},"value":"8.dp","category":"spacing","context":"unknown"}]}}
JSON
"#
}

fn token_inference_fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":5,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-$language","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":11,"files_scanned":1},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0},"tokens":{"configured_token_count":2,"used_token_count":0,"token_reference_site_count":0,"hardcoded_style_candidate_count":3,"token_references_by_category":{"color":0,"spacing":0,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"hardcoded_by_category":{"color":0,"spacing":2,"typography":1,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"parent_scopes_with_token_references":0,"parent_scopes_with_hardcoded_candidates":0}},"design_system_tokens":[{"id":"spacing.s","key":"Spacing.s","category":"spacing","value":"4px"},{"id":"type.body","key":"var(--text-body)","category":"typography","value":"16px"}],"token_sites":[],"hardcoded_style_sites":[{"id":"hardcoded.react:src/Card.tsx:1:10:spacing","location":{"file":"src/Card.tsx","line":1,"column":10},"value":"3","category":"spacing","context":"padding"},{"id":"hardcoded.react:src/Card.tsx:2:10:spacing","location":{"file":"src/Card.tsx","line":2,"column":10},"value":"200","category":"spacing","context":"width"},{"id":"hardcoded.react:src/Card.tsx:3:10:Typography","location":{"file":"src/Card.tsx","line":3,"column":10},"value":"16px","category":"typography","context":"typography"}]}}
JSON
"#
}

fn duplicate_hardcoded_fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":5,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-$language","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":11,"files_scanned":1},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0},"tokens":{"configured_token_count":0,"used_token_count":0,"token_reference_site_count":0,"hardcoded_style_candidate_count":2,"token_references_by_category":{"color":0,"spacing":0,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"hardcoded_by_category":{"color":0,"spacing":2,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"parent_scopes_with_token_references":0,"parent_scopes_with_hardcoded_candidates":0}},"hardcoded_style_sites":[{"id":"hardcoded.$language:src/Screen:2:12:spacing","location":{"file":"src/Screen","line":2,"column":12},"value":"8","category":"spacing","context":"unknown"},{"id":"hardcoded.$language:src/Screen:2:12:spacing","location":{"file":"src/Screen","line":2,"column":12},"value":"8","category":"spacing","context":"unknown"}]}}
JSON
"#
}

fn overwrite_pack_script(wax_home: &Path, language: &str, script: &str) {
    let script_path = wax_home.join(format!("langs/{language}/0.1.0/pack.sh"));
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
    }
}

#[test]
fn scan_output_groups_registry_aliases_by_canonical_key_and_status() {
    let fixture = fixture("scan-output-package-identities", &["react"]);
    let components = serde_json::json!([
        {"id":"ds.dialog.root", "symbol":"Root", "package":"@radix-ui/react-dialog"},
        {"id":"ds.popover.root", "symbol":"Root", "package":"@radix-ui/react-popover"}
    ]);
    fs::write(
        fixture.repo.join(language_registry_relative_path("react")),
        serde_json::json!({"schema_version": 1, "components": components}).to_string(),
    )
    .unwrap();
    write_lockfile(&fixture.repo, &["react"]);

    let usages = serde_json::json!([
        {
            "id":"dialog-usage", "location":{"file":"src/App.tsx", "line":1},
            "symbol":"Dialog.Root", "qualified_symbol":"@radix-ui/react-dialog#Root",
            "registry_symbol":"@radix-ui/react-dialog#Root", "match_status":"resolved",
            "callee_origin":"registry",
            "resolution_evidence":{"kind":"registry_package_match", "package":"@radix-ui/react-dialog"}
        },
        {
            "id":"popover-usage", "location":{"file":"src/App.tsx", "line":2},
            "symbol":"Popover.Root", "qualified_symbol":"@radix-ui/react-popover#Root",
            "registry_symbol":"@radix-ui/react-popover#Root", "match_status":"resolved",
            "callee_origin":"registry",
            "resolution_evidence":{"kind":"registry_package_match", "package":"@radix-ui/react-popover"}
        },
        {
            "id":"dialog-alias", "location":{"file":"src/App.tsx", "line":3},
            "symbol":"D.Root", "qualified_symbol":"@radix-ui/react-dialog#Root",
            "registry_symbol":"@radix-ui/react-dialog#Root", "match_status":"resolved",
            "callee_origin":"registry",
            "resolution_evidence":{"kind":"registry_package_match", "package":"@radix-ui/react-dialog"}
        },
        {
            "id":"popover-export-alias", "location":{"file":"src/App.tsx", "line":4},
            "symbol":"Popover.AnchorRoot", "qualified_symbol":"@radix-ui/react-popover#AnchorRoot",
            "registry_symbol":"@radix-ui/react-popover#Root", "match_status":"resolved",
            "callee_origin":"registry",
            "resolution_evidence":{"kind":"registry_package_match", "package":"@radix-ui/react-popover"}
        },
        {
            "id":"dialog-candidate", "location":{"file":"src/App.tsx", "line":5},
            "symbol":"Root", "registry_symbol":"@radix-ui/react-dialog#Root",
            "match_status":"candidate", "callee_origin":"registry",
            "resolution_evidence":{"kind":"registry_import_missing"}
        }
    ]);
    let component_facts = serde_json::json!([
        {"id":"ds.@radix-ui/react-dialog#Root", "symbol":"Root", "registry_symbol":"@radix-ui/react-dialog#Root"},
        {"id":"ds.@radix-ui/react-popover#Root", "symbol":"Root", "registry_symbol":"@radix-ui/react-popover#Root"}
    ]);
    let mut facts: ScanFacts = serde_json::from_value(serde_json::json!({
        "schema_version": 5,
        "language": {
            "id": "react", "version": "0.1.0", "ecosystem": "test",
            "parser_name": "fixture", "parser_version": "1.0.0"
        },
        "snapshot_id": "snap-react",
        "scanned_at": "1970-01-01T00:00:00Z",
        "status": "complete",
        "design_system_components": component_facts,
        "local_components": [],
        "usage_sites": usages,
        "diagnostics": [],
        "metrics": {"parse_extract_ms": 3, "files_scanned": 1},
        "counts": wax_contract::CountSummary::default()
    }))
    .unwrap();
    facts.recompute_counts().unwrap();
    let response = serde_json::json!({
        "type": "scan_facts", "api_version": 1, "language_id": "react", "facts": facts
    });
    let script = format!("#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{response}\nJSON\n");
    overwrite_pack_script(&fixture.wax_home, "react", &script);

    Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("scan should pass");
    let output = fs::read(fixture.repo.join(".wax/out/scan-merged.json")).unwrap();
    let merged: MergedScan = serde_json::from_slice(&output).unwrap();
    merged.validate().expect("merged facts should validate");
    assert_eq!(merged.repo_summary.counts.registry.component_count, 2);
    assert_eq!(merged.repo_summary.counts.registry.used_component_count, 2);
    let symbol_ids = merged
        .symbol_usage_summary
        .iter()
        .map(|summary| summary.symbol_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        symbol_ids,
        [
            "react:registry:@radix-ui/react-dialog#Root",
            "react:registry:@radix-ui/react-popover#Root",
            "react:candidate:@radix-ui/react-dialog#Root",
        ]
    );
    for (summary, count, qualified) in [
        (
            &merged.symbol_usage_summary[0],
            2,
            Some("@radix-ui/react-dialog#Root"),
        ),
        (&merged.symbol_usage_summary[1], 2, None),
        (&merged.symbol_usage_summary[2], 1, None),
    ] {
        assert_eq!(summary.symbol, "Root");
        assert_eq!(summary.raw_invocation_count, count);
        assert_eq!(summary.qualified_symbol.as_deref(), qualified);
    }
    let facts = merged.languages.values().next().unwrap();
    assert_eq!(
        facts
            .usage_sites
            .iter()
            .map(|site| site.symbol.as_str())
            .collect::<Vec<_>>(),
        [
            "Dialog.Root",
            "Popover.Root",
            "D.Root",
            "Popover.AnchorRoot",
            "Root"
        ]
    );
}

#[test]
fn scan_output_merges_token_counts_summaries_and_ratio() {
    let fixture = fixture("scan-output-tokens", &["compose"]);
    overwrite_pack_script(&fixture.wax_home, "compose", token_fixture_script());

    let merged = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("scan should pass");

    assert_eq!(merged.repo_summary.counts.tokens.configured_token_count, 1);
    assert_eq!(merged.repo_summary.counts.tokens.used_token_count, 1);
    assert_eq!(
        merged.repo_summary.counts.tokens.token_reference_site_count,
        1
    );
    assert_eq!(
        merged
            .repo_summary
            .counts
            .tokens
            .hardcoded_style_candidate_count,
        1
    );
    assert_eq!(
        merged.token_inference.counts.unassessed_observation_count,
        1
    );
    let expected: u64 = merged
        .languages
        .values()
        .map(|facts| facts.metrics.parse_extract_ms)
        .sum();
    assert!(expected > 0);
    assert_eq!(merged.repo_summary.metrics.parse_extract_ms, expected);
    assert_eq!(merged.token_usage_summary.len(), 1);
    assert_eq!(
        merged.languages[&LanguageId::try_from("compose").unwrap()]
            .token_usage_summary
            .len(),
        1
    );
}

#[test]
fn scan_output_classifies_exact_near_and_unmatched_token_inference() {
    let fixture = fixture("scan-output-token-inference", &["react"]);
    overwrite_pack_script(&fixture.wax_home, "react", token_inference_fixture_script());

    let merged = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("scan should pass");

    assert_eq!(merged.token_inference.numeric_tolerance, 2.0);
    assert_eq!(merged.token_inference.counts.hardcoded_observation_count, 3);
    assert_eq!(
        merged
            .token_inference
            .counts
            .near_replacement_candidate_count,
        1
    );
    assert_eq!(merged.token_inference.counts.unmatched_observation_count, 1);
    assert_eq!(
        merged
            .token_inference
            .counts
            .exact_replacement_candidate_count,
        1
    );
    assert_eq!(
        merged.token_inference.counts.unassessed_observation_count,
        0
    );
    assert_eq!(merged.token_inference.counts.assessed_observation_count, 3);
    assert_eq!(merged.token_inference.sites.len(), 3);

    let exact = merged
        .token_inference
        .sites
        .iter()
        .find(|row| row.site_id == "hardcoded.react:src/Card.tsx:3:10:Typography")
        .expect("inference row for the bare Tailwind typography value");
    assert_eq!(exact.classification, TokenInferenceClassification::Exact);
    let [suggestion] = exact.suggestions.as_slice() else {
        panic!(
            "expected a single exact suggestion, got {:?}",
            exact.suggestions
        );
    };
    assert_eq!(suggestion.token_key, "var(--text-body)");
    assert_eq!(suggestion.canonical_value, "16px");

    let near = merged
        .token_inference
        .sites
        .iter()
        .find(|row| row.classification == TokenInferenceClassification::Near)
        .expect("near row");
    assert_eq!(near.site_id, "hardcoded.react:src/Card.tsx:1:10:spacing");
    assert_eq!(near.suggestions[0].distance, Some(1.0));

    let unmatched = merged
        .token_inference
        .sites
        .iter()
        .find(|row| row.classification == TokenInferenceClassification::Unmatched)
        .expect("unmatched row");
    assert_eq!(
        unmatched.site_id,
        "hardcoded.react:src/Card.tsx:2:10:spacing"
    );
    assert!(unmatched.suggestions.is_empty());
    assert!(unmatched.confidence.is_none());
}

#[test]
fn scan_rejects_duplicate_hardcoded_ids_before_writing_outputs() {
    let fixture = fixture("scan-output-duplicate-hardcoded", &["compose"]);
    overwrite_pack_script(
        &fixture.wax_home,
        "compose",
        duplicate_hardcoded_fixture_script(),
    );

    let err = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect_err("duplicate site ids must fail");

    assert!(matches!(
        err,
        EngineError::ScanFacts(wax_contract::ScanFactsError::ContractViolation {
            field,
            ..
        }) if field == "languages.compose.hardcoded_style_sites[1].id"
    ));
    assert!(!fixture.repo.join(".wax/out/scan-merged.json").exists());
}

#[test]
fn scan_output_writes_per_language_and_merged_output_json() {
    let fixture = fixture("scan-output", &["react", "compose"]);

    let returned = Engine::scan_repo_with_options(
        &fixture.repo,
        ScanOptions {
            scan_concurrency: Some(2),
            ..fixture.scan_options()
        },
    )
    .expect("scan should pass");

    let out_dir = fixture.repo.join(".wax/out");
    let languages_dir = out_dir.join("languages");
    let merged: MergedScan =
        serde_json::from_str(&fs::read_to_string(out_dir.join("scan-merged.json")).unwrap())
            .unwrap();
    let compose: ScanFacts =
        serde_json::from_str(&fs::read_to_string(languages_dir.join("compose.json")).unwrap())
            .unwrap();
    let react: ScanFacts =
        serde_json::from_str(&fs::read_to_string(languages_dir.join("react.json")).unwrap())
            .unwrap();

    assert_eq!(merged, returned);
    assert_eq!(
        [
            merged.schema_version,
            compose.schema_version,
            react.schema_version
        ],
        [5, 5, 5]
    );
    assert_eq!(
        compose.language.id,
        LanguageId::try_from("compose").unwrap()
    );
    assert_eq!(react.language.id, LanguageId::try_from("react").unwrap());
    assert_eq!(
        merged.languages.keys().collect::<Vec<_>>(),
        vec![
            &LanguageId::try_from("compose").unwrap(),
            &LanguageId::try_from("react").unwrap()
        ]
    );
    assert_eq!(
        merged.languages[&LanguageId::try_from("compose").unwrap()]
            .metrics
            .parse_extract_ms,
        5
    );
    assert_eq!(
        merged.languages[&LanguageId::try_from("react").unwrap()]
            .metrics
            .parse_extract_ms,
        3
    );
    let expected: u64 = merged
        .languages
        .values()
        .map(|facts| facts.metrics.parse_extract_ms)
        .sum();
    assert_eq!(expected, 8);
    assert_eq!(merged.repo_summary.metrics.parse_extract_ms, expected);
    assert!(
        fs::read_dir(&out_dir)
            .unwrap()
            .chain(fs::read_dir(&languages_dir).unwrap())
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
        "atomic write temp files should be cleaned up after successful scan"
    );
}

#[test]
fn scan_output_can_replace_existing_output_from_prior_scan() {
    let fixture = fixture("scan-output-replace", &["compose"]);

    Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("first scan should pass");
    let out_dir = fixture.repo.join(".wax/out");
    let languages_dir = out_dir.join("languages");
    fs::write(out_dir.join(".scan-merged.json.0.tmp"), "stale temp").unwrap();
    fs::write(languages_dir.join(".compose.json.0.tmp"), "stale temp").unwrap();

    let returned = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("second scan should replace output");

    let merged: MergedScan =
        serde_json::from_str(&fs::read_to_string(out_dir.join("scan-merged.json")).unwrap())
            .unwrap();
    let compose: ScanFacts =
        serde_json::from_str(&fs::read_to_string(languages_dir.join("compose.json")).unwrap())
            .unwrap();

    assert_eq!(merged, returned);
    assert_eq!(
        compose.language.id,
        LanguageId::try_from("compose").unwrap()
    );
    assert_eq!(
        fs::read_to_string(out_dir.join(".scan-merged.json.0.tmp")).unwrap(),
        "stale temp"
    );
    assert_eq!(
        fs::read_to_string(languages_dir.join(".compose.json.0.tmp")).unwrap(),
        "stale temp"
    );
}

#[test]
fn scan_output_removes_language_files_missing_from_latest_scan() {
    let fixture = fixture("scan-output-prune", &["react", "compose"]);

    Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("first scan should pass");
    let languages_dir = fixture.repo.join(".wax/out/languages");
    assert!(languages_dir.join("compose.json").exists());
    assert!(languages_dir.join("react.json").exists());

    write_waxrc(&fixture.repo, &["compose"]);
    let returned = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("second scan should pass");

    assert!(languages_dir.join("compose.json").exists());
    assert!(
        !languages_dir.join("react.json").exists(),
        "stale disabled language output should be removed"
    );
    assert_eq!(
        returned.languages.keys().collect::<Vec<_>>(),
        vec![&LanguageId::try_from("compose").unwrap()]
    );
}

#[cfg(unix)]
#[test]
fn scan_output_preserves_previous_json_when_output_write_fails() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture("scan-output-failure-preserves", &["compose"]);

    Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("first scan should pass");
    let out_dir = fixture.repo.join(".wax/out");
    let languages_dir = out_dir.join("languages");
    let merged_path = out_dir.join("scan-merged.json");
    let compose_path = languages_dir.join("compose.json");
    let original_merged = fs::read_to_string(&merged_path).unwrap();
    let original_compose = fs::read_to_string(&compose_path).unwrap();

    let mut permissions = fs::metadata(&languages_dir).unwrap().permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&languages_dir, permissions).unwrap();

    let error = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect_err("unwritable output should fail");

    let mut permissions = fs::metadata(&languages_dir).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&languages_dir, permissions).unwrap();

    assert!(matches!(
        error,
        EngineError::AtomicWrite(AtomicWriteError::CreateTemp { .. })
    ));
    assert_eq!(fs::read_to_string(&merged_path).unwrap(), original_merged);
    assert_eq!(fs::read_to_string(&compose_path).unwrap(), original_compose);
    serde_json::from_str::<MergedScan>(&original_merged).unwrap();
    serde_json::from_str::<ScanFacts>(&original_compose).unwrap();
}

#[cfg(unix)]
#[test]
fn scan_output_keeps_old_language_files_when_merged_output_write_fails() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture(
        "scan-output-merged-failure-preserves",
        &["react", "compose"],
    );

    Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("first scan should pass");
    let out_dir = fixture.repo.join(".wax/out");
    let languages_dir = out_dir.join("languages");
    let merged_path = out_dir.join("scan-merged.json");
    let original_merged = fs::read_to_string(&merged_path).unwrap();
    assert!(languages_dir.join("react.json").exists());

    write_waxrc(&fixture.repo, &["compose"]);

    let mut out_permissions = fs::metadata(&out_dir).unwrap().permissions();
    out_permissions.set_mode(0o555);
    fs::set_permissions(&out_dir, out_permissions).unwrap();

    let error = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect_err("merged output write should fail");

    let mut out_permissions = fs::metadata(&out_dir).unwrap().permissions();
    out_permissions.set_mode(0o755);
    fs::set_permissions(&out_dir, out_permissions).unwrap();

    assert!(matches!(
        error,
        EngineError::AtomicWrite(AtomicWriteError::CreateTemp { .. })
    ));
    assert_eq!(fs::read_to_string(&merged_path).unwrap(), original_merged);
    assert!(
        languages_dir.join("react.json").exists(),
        "old merged output still references react, so react output must remain"
    );
    serde_json::from_str::<MergedScan>(&original_merged).unwrap();
}

fn css_token_fixture_script() -> &'static str {
    r#"#!/bin/sh
set -eu
language="$1"
cat >/dev/null
cat <<JSON
{"type":"scan_facts","api_version":1,"language_id":"$language","facts":{"schema_version":5,"language":{"id":"$language","version":"0.1.0","ecosystem":"test","parser_name":"fixture","parser_version":"1.0.0"},"snapshot_id":"snap-$language","scanned_at":"1970-01-01T00:00:00Z","status":"complete","design_system_components":[],"local_components":[],"usage_sites":[],"diagnostics":[],"metrics":{"invocation_adoption_ratio":null,"registry_resolution_ratio":null,"parse_extract_ms":1,"files_scanned":3},"counts":{"registry":{"component_count":0,"used_component_count":0,"resolved_raw_invocation_count":0,"candidate_raw_invocation_count":0},"definitions":{"local_definition_count":0,"invoked_local_definition_count":0,"unused_local_definition_count":0},"raw_invocations":{"total":0,"resolved":0,"local":0,"candidate":0,"unresolved":0},"adoption":{"eligible_invocation_count":0,"adopted_invocation_count":0,"non_adopted_invocation_count":0},"parent_scopes":{"total":0,"with_resolved_invocations":0,"with_local_invocations":0,"with_unresolved_invocations":0},"invocation_origins":{"registry":0,"local":0,"framework":0,"external":0,"application":0,"unknown":0},"tokens":{"configured_token_count":2,"used_token_count":2,"token_reference_site_count":3,"hardcoded_style_candidate_count":0,"token_references_by_category":{"color":3,"spacing":0,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"hardcoded_by_category":{"color":0,"spacing":0,"typography":0,"radius":0,"elevation":0,"aspect_ratio":0,"unknown":0},"parent_scopes_with_token_references":0,"parent_scopes_with_hardcoded_candidates":0}},"design_system_tokens":[{"id":"css.blue-9","key":"var(--blue-9)","category":"color"},{"id":"blue.blue9","key":"blue.blue9","category":"color"}],"token_sites":[{"id":"token.react:src/app.css:1:18:css.blue-9","location":{"file":"src/app.css","line":1,"column":18},"token_id":"css.blue-9","key":"var(--blue-9)","category":"color"},{"id":"token.react:src/ts.ts:1:1:blue.blue9","location":{"file":"src/ts.ts","line":1,"column":1},"token_id":"blue.blue9","key":"blue.blue9","category":"color"},{"id":"token.react:src/App.tsx:1:42:css.blue-9","location":{"file":"src/App.tsx","line":1,"column":42},"token_id":"css.blue-9","key":"var(--blue-9)","category":"color"}],"hardcoded_style_sites":[]}}
JSON
"#
}

#[test]
fn scan_output_merges_css_and_tailwind_class_references_by_root_group() {
    let fixture = fixture("scan-output-css-tokens", &["react"]);
    fs::write(
        fixture.repo.join(".wax/wax.config.json"),
        r#"{"schema_version":2,"languages":{"react":{"roots":{"web":["src"]}}}}"#,
    )
    .unwrap();
    overwrite_pack_script(&fixture.wax_home, "react", css_token_fixture_script());

    let merged = Engine::scan_repo_with_options(&fixture.repo, fixture.scan_options())
        .expect("scan should pass");

    let facts = &merged.languages[&LanguageId::try_from("react").unwrap()];
    let css = facts
        .token_sites
        .iter()
        .find(|site| site.token_id == "css.blue-9")
        .expect("css site");
    assert_eq!(css.location.root_group.as_deref(), Some("web"));
    let class = facts
        .token_sites
        .iter()
        .find(|site| site.id == "token.react:src/App.tsx:1:42:css.blue-9")
        .expect("Tailwind class site");
    assert_eq!(class.location.root_group.as_deref(), Some("web"));
    let tokens = &merged.repo_summary.counts.tokens;
    assert_eq!(tokens.token_reference_site_count, 3);
    assert_eq!(tokens.used_token_count, 2);
    let used: Vec<_> = merged
        .token_usage_summary
        .iter()
        .map(|summary| (summary.token_id.as_str(), summary.reference_count))
        .collect();
    assert!(used.contains(&("css.blue-9", 2)), "{used:?}");
    assert!(used.contains(&("blue.blue9", 1)), "{used:?}");
}
