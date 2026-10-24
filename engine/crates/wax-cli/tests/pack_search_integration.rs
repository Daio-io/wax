//! End-to-end publish/merge → search → detail across React, Compose, and Swift.
//!
//! Each pack enriches a seed registry from its discover fixture, merges the
//! enriched facts into a publisher document, then exercises `wax search` and
//! `wax component` against the written registry.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{EnvVarGuard, TestDir, env_lock};
use serde_json::{Value, json};
use wax_contract::{RegistryDocument, registry_to_json};
use wax_core::registry_merge::merge_registry;
use wax_lang_api::{DiscoverRegistryRequest, WIRE_API_VERSION};
use wax_lang_compose::discover_registry_details as compose_details;
use wax_lang_react::discover_registry_details as react_details;
use wax_lang_swift::discover_registry_details as swift_details;

struct PackCase {
    language: &'static str,
    design_system_id: &'static str,
    component_id: &'static str,
    package: &'static str,
    fixture_root: PathBuf,
    roots: Vec<&'static str>,
    enrich: fn(&DiscoverRegistryRequest) -> RegistryDocument,
}

fn enrich_react(request: &DiscoverRegistryRequest) -> RegistryDocument {
    react_details(request).expect("react enrichment").document
}

fn enrich_compose(request: &DiscoverRegistryRequest) -> RegistryDocument {
    compose_details(request)
        .expect("compose enrichment")
        .document
}

fn enrich_swift(request: &DiscoverRegistryRequest) -> RegistryDocument {
    swift_details(request).expect("swift enrichment").document
}

fn pack_cases() -> [PackCase; 3] {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    [
        PackCase {
            language: "react",
            design_system_id: "react-ds",
            component_id: "react.rich-button",
            package: "@acme/design-system",
            fixture_root: manifest.join("../wax-lang-react/tests/fixtures/discover"),
            roots: vec!["design-system/src"],
            enrich: enrich_react,
        },
        PackCase {
            language: "compose",
            design_system_id: "compose-ds",
            component_id: "compose.rich-button",
            package: "com.example.ds",
            fixture_root: manifest.join("../wax-lang-compose/tests/fixtures/discover"),
            roots: vec!["design-system/src/main/kotlin"],
            enrich: enrich_compose,
        },
        PackCase {
            language: "swift",
            design_system_id: "swift-ds",
            component_id: "swift.rich-button",
            package: "AcmeDesignSystem",
            fixture_root: manifest.join("../wax-lang-swift/tests/fixtures/rich-discover"),
            roots: vec!["design-system/Sources"],
            enrich: enrich_swift,
        },
    ]
}

fn seed_registry(component_id: &str, package: &str) -> RegistryDocument {
    serde_json::from_value(json!({
        "schema_version": 1,
        "components": [{
            "id": component_id,
            "symbol": "RichButton",
            "package": package,
            "metadata": {
                "tags": ["seed"]
            }
        }]
    }))
    .expect("seed registry")
}

fn publish_merged_registry(pack: &PackCase) -> RegistryDocument {
    let seed = seed_registry(pack.component_id, pack.package);
    let discovered = (pack.enrich)(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: pack.language.try_into().expect("language id"),
        repo_root: pack.fixture_root.to_string_lossy().into_owned(),
        roots: pack.roots.iter().map(|root| (*root).to_owned()).collect(),
        registry: seed.clone(),
    });
    let merged = merge_registry(Some(&seed), &discovered, true).expect("merge publisher registry");
    let mut document = merged.document;
    document.extra.insert(
        "language".to_owned(),
        Value::String(pack.language.to_owned()),
    );
    assert!(
        document.components[0]
            .extracted
            .as_ref()
            .is_some_and(|extracted| extracted.documentation.as_deref()
                == Some("A button used for important actions.")),
        "{} merge should keep enrichment documentation",
        pack.language
    );
    assert_eq!(
        document.components[0]
            .metadata
            .as_ref()
            .map(|metadata| metadata.tags.as_slice()),
        Some(["seed".to_owned()].as_slice()),
        "{} merge should retain publisher metadata",
        pack.language
    );
    document
}

fn remember_local_registry(
    wax_home: &Path,
    design_system_id: &str,
    language: &str,
    registry_path: &Path,
) {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "registry",
            "add",
            design_system_id,
            &registry_path.display().to_string(),
            "--language",
            language,
            "--name",
            &format!("{language} fixture"),
        ])
        .env("WAX_HOME", wax_home)
        .output()
        .expect("spawn registry add");
    assert!(
        output.status.success(),
        "registry add failed for {language}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn prepare_pack_registry(root: &TestDir, wax_home: &Path, pack: &PackCase) -> RegistryDocument {
    let document = publish_merged_registry(pack);
    let registry_path = root.path.join(format!("{}-guidance.json", pack.language));
    fs::write(
        &registry_path,
        registry_to_json(&document).expect("serialize registry"),
    )
    .expect("write registry");
    remember_local_registry(
        wax_home,
        pack.design_system_id,
        pack.language,
        &registry_path,
    );
    document
}

fn run_wax(wax_home: &Path, repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(args)
        .args(["--repo-root"])
        .arg(repo)
        .env("WAX_HOME", wax_home)
        .output()
        .expect("spawn wax")
}

#[test]
fn parser_packs_publish_merge_then_search_and_detail() {
    let _guard = env_lock();
    let root = TestDir::new("pack-e2e");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);

    for pack in &pack_cases() {
        let _document = prepare_pack_registry(&root, &wax_home, pack);

        let search = run_wax(
            &wax_home,
            &repo,
            &[
                "search",
                "RichButton",
                "--design-system",
                pack.design_system_id,
                "--language",
                pack.language,
                "--kind",
                "component",
                "--format",
                "json",
            ],
        );
        assert!(
            search.status.success(),
            "{} search failed: {}",
            pack.language,
            String::from_utf8_lossy(&search.stderr)
        );
        let search_value: Value =
            serde_json::from_str(String::from_utf8(search.stdout).unwrap().trim()).unwrap();
        assert_eq!(
            search_value["results"][0]["id"], pack.component_id,
            "{} search should find the enriched component",
            pack.language
        );
        let digest = search_value["results"][0]["snapshot"]["digest"]
            .as_str()
            .expect("digest")
            .to_owned();

        let detail = run_wax(
            &wax_home,
            &repo,
            &[
                "component",
                pack.component_id,
                "--design-system",
                pack.design_system_id,
                "--snapshot",
                &digest,
                "--format",
                "json",
            ],
        );
        assert!(
            detail.status.success(),
            "{} detail failed: {}",
            pack.language,
            String::from_utf8_lossy(&detail.stderr)
        );
        let detail_value: Value =
            serde_json::from_str(String::from_utf8(detail.stdout).unwrap().trim()).unwrap();
        assert_eq!(detail_value["kind"], "component");
        assert_eq!(detail_value["entry"]["id"], pack.component_id);
        assert_eq!(
            detail_value["entry"]["extracted"]["documentation"],
            "A button used for important actions."
        );
        assert_eq!(
            search_value["results"][0]["summary"],
            "A button used for important actions."
        );
        assert_eq!(detail_value["snapshot"]["digest"], digest);
    }
}

#[test]
fn parser_packs_search_guidance_then_return_snapshot_pinned_facts() {
    let _guard = env_lock();
    let root = TestDir::new("pack-guidance-snapshot");
    let wax_home = root.path.join("wax-home");
    let repo = root.path.join("repo");
    fs::create_dir_all(&wax_home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let _home = EnvVarGuard::set("WAX_HOME", &wax_home);

    for pack in &pack_cases() {
        let document = prepare_pack_registry(&root, &wax_home, pack);
        let expected_entry = serde_json::to_value(&document.components[0]).unwrap();
        let registry_path = root.path.join(format!("{}-guidance.json", pack.language));

        let search = run_wax(
            &wax_home,
            &repo,
            &[
                "search",
                "touch-first",
                "--design-system",
                pack.design_system_id,
                "--kind",
                "component",
                "--format",
                "json",
            ],
        );
        assert!(
            search.status.success(),
            "{} guidance search failed: {}",
            pack.language,
            String::from_utf8_lossy(&search.stderr)
        );
        let search_value: Value =
            serde_json::from_str(String::from_utf8(search.stdout).unwrap().trim()).unwrap();
        assert_eq!(search_value["results"][0]["id"], pack.component_id);
        assert_eq!(
            search_value["results"][0]["summary"],
            "A button used for important actions."
        );
        let digest = search_value["results"][0]["snapshot"]["digest"]
            .as_str()
            .expect("digest")
            .to_owned();

        let mut refreshed = serde_json::to_value(&document).unwrap();
        refreshed["components"][0]["symbol"] = json!("RefreshedButton");
        refreshed["components"][0]["extracted"]["signature"]["parameters"] = json!([]);
        refreshed["components"][0]["extracted"]["best_practices"] = json!([]);
        fs::write(
            &registry_path,
            serde_json::to_vec_pretty(&refreshed).unwrap(),
        )
        .expect("write refreshed registry");
        let refresh = Command::new(env!("CARGO_BIN_EXE_wax"))
            .args(["registry", "refresh", pack.design_system_id])
            .env("WAX_HOME", &wax_home)
            .output()
            .expect("spawn registry refresh");
        assert!(
            refresh.status.success(),
            "{} refresh failed: {}",
            pack.language,
            String::from_utf8_lossy(&refresh.stderr)
        );

        let detail = run_wax(
            &wax_home,
            &repo,
            &[
                "component",
                pack.component_id,
                "--design-system",
                pack.design_system_id,
                "--snapshot",
                &digest,
                "--format",
                "json",
            ],
        );
        assert!(
            detail.status.success(),
            "{} pinned detail failed: {}",
            pack.language,
            String::from_utf8_lossy(&detail.stderr)
        );
        let detail: Value =
            serde_json::from_str(String::from_utf8(detail.stdout).unwrap().trim()).unwrap();
        assert_eq!(detail["entry"], expected_entry);
        let enum_parameter = detail["entry"]["extracted"]["signature"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["type_kind"] == "enum")
            .expect("enum parameter");
        assert_eq!(
            enum_parameter["allowed_values"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            detail["entry"]["extracted"]["best_practices"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(detail["snapshot"]["digest"], digest);
    }
}
