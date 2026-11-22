//! Cross-pack parity for schema v4 origin and resolution evidence fields.

use std::fs;
use std::path::Path;

use wax_contract::{
    CalleeOrigin, LanguageId, MatchStatus, ResolutionEvidenceKind, ScanFacts, UsageSite,
};
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_compose::ComposeLanguage;
use wax_lang_react::ReactLanguage;
use wax_lang_swift::SwiftLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExpectedOutcome {
    match_status: MatchStatus,
    callee_origin: CalleeOrigin,
    evidence_kind: ResolutionEvidenceKind,
}

fn assert_parity(site: &UsageSite, expected: ExpectedOutcome, pack: &str, scenario: &str) {
    assert_eq!(
        site.match_status, expected.match_status,
        "{pack} {scenario}: match_status"
    );
    assert_eq!(
        site.callee_origin, expected.callee_origin,
        "{pack} {scenario}: callee_origin"
    );
    assert_eq!(
        site.resolution_evidence.kind, expected.evidence_kind,
        "{pack} {scenario}: resolution_evidence.kind"
    );
}

fn write_registry(path: &Path, symbol: &str, package: Option<&str>) {
    let package_field = package
        .map(|value| format!(r#","package":"{value}""#))
        .unwrap_or_default();
    fs::write(
        path,
        format!(
            r#"{{
  "schema_version": 1,
  "components": [{{
    "id": "ds.btn",
    "symbol": "{symbol}"{package_field}
  }}]
}}"#
        ),
    )
    .unwrap();
}

fn scan_compose(source: &str, registry_package: Option<&str>) -> ScanFacts {
    let tmp = tempfile::tempdir().expect("tempdir");
    let registry_dir = tmp.path().join("design-system");
    fs::create_dir_all(&registry_dir).unwrap();
    write_registry(
        &registry_dir.join("registry.json"),
        "DsButton",
        registry_package,
    );
    let source_dir = tmp.path().join("app/src/main/kotlin");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("Screen.kt"), source).unwrap();

    scan_request(
        tmp.path(),
        "compose",
        serde_json::json!({
            "registry": "design-system/registry.json",
            "roots": ["app/src/main/kotlin"]
        }),
        "snap-compose-parity",
        |request| ComposeLanguage::new().scan(&request).expect("compose scan"),
    )
}

fn scan_react(source: &str, registry_package: Option<&str>, with_packages: bool) -> ScanFacts {
    let tmp = tempfile::tempdir().expect("tempdir");
    let registry_dir = tmp.path().join("design-system");
    fs::create_dir_all(&registry_dir).unwrap();
    write_registry(
        &registry_dir.join("registry.json"),
        "DsButton",
        registry_package,
    );
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("ds")).unwrap();
    fs::write(
        src.join("ds/index.ts"),
        "export const DsButton = () => null;\n",
    )
    .unwrap();
    fs::write(src.join("Screen.tsx"), source).unwrap();

    let mut config = serde_json::json!({
        "registry": "design-system/registry.json",
        "roots": ["src"],
        "aliases": {
            "@/*": ["src/*"]
        }
    });
    if with_packages {
        config["packages"] = serde_json::json!({
            "@acme/design-system": {
                "exports": {
                    ".": "src/ds/index.ts"
                }
            }
        });
    }

    scan_request(
        tmp.path(),
        "react",
        config,
        "snap-react-parity",
        |request| ReactLanguage::new().scan(&request).expect("react scan"),
    )
}

fn scan_swift(source: &str, registry_package: Option<&str>) -> ScanFacts {
    let tmp = tempfile::tempdir().expect("tempdir");
    let registry_dir = tmp.path().join("design-system");
    fs::create_dir_all(&registry_dir).unwrap();
    write_registry(
        &registry_dir.join("registry.json"),
        "DsButton",
        registry_package,
    );
    let source_dir = tmp.path().join("app/Sources");
    fs::create_dir_all(source_dir.join("App")).unwrap();
    fs::write(source_dir.join("App/Screen.swift"), source).unwrap();

    scan_request(
        tmp.path(),
        "swift",
        serde_json::json!({
            "registry": "design-system/registry.json",
            "roots": ["app/Sources"]
        }),
        "snap-swift-parity",
        |request| SwiftLanguage::new().scan(&request).expect("swift scan"),
    )
}

fn scan_request<T>(
    repo_root: &Path,
    language_id: &str,
    config: serde_json::Value,
    snapshot_id: &str,
    scan: impl FnOnce(ScanRequest) -> T,
) -> T {
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from(language_id).expect("valid language id"),
        repo_root: repo_root.to_string_lossy().to_string(),
        snapshot_id: snapshot_id.to_owned(),
        config: config
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<ScanConfig>(),
    };
    scan(request)
}

fn first_usage(facts: &ScanFacts) -> &UsageSite {
    facts
        .usage_sites
        .iter()
        .find(|site| site.symbol == "DsButton")
        .unwrap_or_else(|| panic!("expected DsButton usage site, got {:?}", facts.usage_sites))
}

fn usage_for<'a>(facts: &'a ScanFacts, symbol: &str) -> &'a UsageSite {
    facts
        .usage_sites
        .iter()
        .find(|site| site.symbol == symbol)
        .unwrap_or_else(|| panic!("expected {symbol} usage site, got {:?}", facts.usage_sites))
}

#[test]
fn parity_classifies_framework_external_and_application_calls() {
    let compose = scan_compose(
        "package app\nimport androidx.compose.foundation.layout.Box\nimport coil.compose.AsyncImage\n@Composable\nfun Screen() { Box(); AsyncImage(); UnknownCard() }",
        Some("com.acme.designsystem"),
    );
    assert_parity(
        usage_for(&compose, "Box"),
        ExpectedOutcome {
            match_status: MatchStatus::Unresolved,
            callee_origin: CalleeOrigin::Framework,
            evidence_kind: ResolutionEvidenceKind::NoMatchingDefinition,
        },
        "compose",
        "framework",
    );
    assert_parity(
        usage_for(&compose, "AsyncImage"),
        ExpectedOutcome {
            match_status: MatchStatus::Unresolved,
            callee_origin: CalleeOrigin::External,
            evidence_kind: ResolutionEvidenceKind::NoMatchingDefinition,
        },
        "compose",
        "external",
    );
    assert_parity(
        usage_for(&compose, "UnknownCard"),
        ExpectedOutcome {
            match_status: MatchStatus::Unresolved,
            callee_origin: CalleeOrigin::Application,
            evidence_kind: ResolutionEvidenceKind::NoMatchingDefinition,
        },
        "compose",
        "application",
    );

    let react = scan_react(
        r#"import { View } from "react-native";
import { AsyncImage } from "coil";
import { MissingCard } from "@/MissingCard";

export function Screen() {
  return <><View /><AsyncImage /><MissingCard /><UnknownCard /></>;
}"#,
        Some("@acme/design-system"),
        false,
    );
    for (symbol, origin, scenario) in [
        ("View", CalleeOrigin::Framework, "framework"),
        ("AsyncImage", CalleeOrigin::External, "external"),
        ("MissingCard", CalleeOrigin::Unknown, "path_alias"),
        ("UnknownCard", CalleeOrigin::Application, "application"),
    ] {
        assert_parity(
            usage_for(&react, symbol),
            ExpectedOutcome {
                match_status: MatchStatus::Unresolved,
                callee_origin: origin,
                evidence_kind: ResolutionEvidenceKind::NoMatchingDefinition,
            },
            "react",
            scenario,
        );
    }
    assert_eq!(react.counts.invocation_origins.external, 1);
    assert_eq!(react.counts.invocation_origins.unknown, 1);
    assert_eq!(react.counts.adoption.adoption_excluded_invocation_count, 2);

    let swift = scan_swift(
        r#"import SwiftUI
import Kingfisher

struct Screen: View {
    var body: some View {
        SwiftUI.Text("Title")
        KFImage()
        UnknownCard()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    for (symbol, origin, scenario) in [
        ("Text", CalleeOrigin::Framework, "framework"),
        ("KFImage", CalleeOrigin::Unknown, "unknown_without_binding"),
        (
            "UnknownCard",
            CalleeOrigin::Unknown,
            "unknown_without_import",
        ),
    ] {
        assert_parity(
            usage_for(&swift, symbol),
            ExpectedOutcome {
                match_status: MatchStatus::Unresolved,
                callee_origin: origin,
                evidence_kind: ResolutionEvidenceKind::NoMatchingDefinition,
            },
            "swift",
            scenario,
        );
    }
}

#[test]
fn parity_matching_package_import_resolves_with_registry_evidence() {
    let expected = ExpectedOutcome {
        match_status: MatchStatus::Resolved,
        callee_origin: CalleeOrigin::Registry,
        evidence_kind: ResolutionEvidenceKind::RegistryPackageMatch,
    };

    let compose = scan_compose(
        "import com.acme.designsystem.DsButton\n@Composable\nfun Screen() { DsButton() }",
        Some("com.acme.designsystem"),
    );
    assert_parity(
        first_usage(&compose),
        expected,
        "compose",
        "matching_package_import",
    );

    let react = scan_react(
        r#"import { DsButton } from "@acme/design-system";

export function Screen() {
  return <DsButton />;
}"#,
        Some("@acme/design-system"),
        true,
    );
    assert_parity(
        first_usage(&react),
        expected,
        "react",
        "matching_package_import",
    );

    let swift = scan_swift(
        r#"import AcmeDesignSystem

struct Screen: View {
    var body: some View {
        DsButton()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    assert_parity(
        first_usage(&swift),
        expected,
        "swift",
        "matching_package_import",
    );
}

#[test]
fn parity_missing_import_is_candidate_with_registry_origin() {
    let expected = ExpectedOutcome {
        match_status: MatchStatus::Candidate,
        callee_origin: CalleeOrigin::Registry,
        evidence_kind: ResolutionEvidenceKind::RegistryImportMissing,
    };

    let compose = scan_compose(
        "@Composable\nfun Screen() { DsButton() }",
        Some("com.acme.designsystem"),
    );
    assert_parity(first_usage(&compose), expected, "compose", "missing_import");

    let react = scan_react(
        "export function Screen() { return <DsButton />; }",
        Some("@acme/design-system"),
        true,
    );
    assert_parity(first_usage(&react), expected, "react", "missing_import");

    let swift = scan_swift(
        r#"struct Screen: View {
    var body: some View {
        DsButton()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    assert_parity(first_usage(&swift), expected, "swift", "missing_import");
}

#[test]
fn parity_package_mismatch_is_unresolved_external_with_observed_package() {
    let expected = ExpectedOutcome {
        match_status: MatchStatus::Unresolved,
        callee_origin: CalleeOrigin::External,
        evidence_kind: ResolutionEvidenceKind::PackageMismatch,
    };

    let compose = scan_compose(
        "import com.other.widgets.DsButton\n@Composable\nfun Screen() { DsButton() }",
        Some("com.acme.designsystem"),
    );
    let compose_site = first_usage(&compose);
    assert_parity(compose_site, expected, "compose", "package_mismatch");
    assert_eq!(
        compose_site.resolution_evidence.package.as_deref(),
        Some("com.other.widgets")
    );

    let react = scan_react(
        r#"import { DsButton } from "other-widgets";

export function Screen() {
  return <DsButton />;
}"#,
        Some("@acme/design-system"),
        false,
    );
    let react_site = first_usage(&react);
    assert_parity(react_site, expected, "react", "package_mismatch");
    assert_eq!(
        react_site.resolution_evidence.package.as_deref(),
        Some("other-widgets")
    );

    let swift = scan_swift(
        r#"import OtherWidgets

struct Screen: View {
    var body: some View {
        DsButton()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    let swift_site = first_usage(&swift);
    assert_parity(swift_site, expected, "swift", "package_mismatch");
    assert_eq!(
        swift_site.resolution_evidence.package.as_deref(),
        Some("OtherWidgets")
    );
}

#[test]
fn parity_legacy_registry_without_package_uses_name_only_evidence() {
    let expected = ExpectedOutcome {
        match_status: MatchStatus::Resolved,
        callee_origin: CalleeOrigin::Registry,
        evidence_kind: ResolutionEvidenceKind::RegistryNameOnlyLegacy,
    };

    let compose = scan_compose(
        "import com.other.widgets.DsButton\n@Composable\nfun Screen() { DsButton() }",
        None,
    );
    assert_parity(
        first_usage(&compose),
        expected,
        "compose",
        "legacy_name_only",
    );

    let react = scan_react(
        r#"import { DsButton } from "other-widgets";

export function Screen() {
  return <DsButton />;
}"#,
        None,
        false,
    );
    assert_parity(first_usage(&react), expected, "react", "legacy_name_only");

    let swift = scan_swift(
        r#"import OtherWidgets

struct Screen: View {
    var body: some View {
        DsButton()
    }
}"#,
        None,
    );
    assert_parity(first_usage(&swift), expected, "swift", "legacy_name_only");
}

#[test]
fn parity_same_file_local_shadows_registry_with_local_same_file_evidence() {
    let expected = ExpectedOutcome {
        match_status: MatchStatus::Local,
        callee_origin: CalleeOrigin::Local,
        evidence_kind: ResolutionEvidenceKind::LocalSameFile,
    };

    let compose = scan_compose(
        "@Composable\nfun DsButton() {}\n@Composable\nfun Screen() { DsButton() }",
        Some("com.acme.designsystem"),
    );
    assert_parity(
        first_usage(&compose),
        expected,
        "compose",
        "same_file_local",
    );

    let react = scan_react(
        r#"const DsButton = () => <button />;

export function Screen() {
  return <DsButton />;
}"#,
        Some("@acme/design-system"),
        true,
    );
    assert_parity(first_usage(&react), expected, "react", "same_file_local");

    let swift = scan_swift(
        r#"struct DsButton: View {
    var body: some View { EmptyView() }
}

struct Screen: View {
    var body: some View {
        DsButton()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    assert_parity(first_usage(&swift), expected, "swift", "same_file_local");
}

fn scan_registry_fixture<T>(
    language: &str,
    files: &[(&str, &str)],
    components: &[serde_json::Value],
    scan: impl FnOnce(ScanRequest) -> T,
) -> T {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(
        tmp.path().join("registry.json"),
        serde_json::json!({"schema_version": 1, "components": components}).to_string(),
    )
    .unwrap();
    for (file, source) in files {
        let path = tmp.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    scan_request(
        tmp.path(),
        language,
        serde_json::json!({"registry": "registry.json", "roots": ["src"]}),
        "snap-package-parity",
        scan,
    )
}

#[test]
fn parity_shared_symbols_resolve_by_package_in_either_registry_order() {
    let mut components = [
        serde_json::json!({"id":"dialog.root", "symbol":"Root", "package":"@radix-ui/react-dialog"}),
        serde_json::json!({"id":"popover.root", "symbol":"Root", "package":"@radix-ui/react-popover"}),
    ];
    for _ in 0..2 {
        let react = scan_registry_fixture(
            "react",
            &[(
                "src/Screen.tsx",
                r#"import * as Dialog from "@radix-ui/react-dialog";
import * as Popover from "@radix-ui/react-popover";
export const Screen = () => <><Dialog.Root /><Popover.Root /></>;"#,
            )],
            &components,
            |request| ReactLanguage::new().scan(&request).expect("react scan"),
        );
        for (observed, key) in [
            ("Dialog.Root", "@radix-ui/react-dialog#Root"),
            ("Popover.Root", "@radix-ui/react-popover#Root"),
        ] {
            let site = usage_for(&react, observed);
            assert_eq!(site.match_status, MatchStatus::Resolved, "{observed}");
            assert_eq!(site.registry_symbol.as_deref(), Some(key));
            assert_eq!(site.qualified_symbol.as_deref(), Some(key));
            assert!(react.design_system_components.iter().any(|component| {
                component.symbol == "Root"
                    && component.registry_symbol == key
                    && component.id == format!("ds.{key}")
            }));
        }
        assert_eq!(react.counts.registry.used_component_count, 2);
        components.reverse();
    }
}

#[test]
fn parity_native_shared_symbols_resolve_by_package_in_either_registry_order() {
    let compose_files = [
        (
            "src/A.kt",
            "package app\nimport widgets.dialog.Root\n@Composable\nfun A() { Root() }",
        ),
        (
            "src/B.kt",
            "package app\nimport widgets.popover.Root\n@Composable\nfun B() { Root() }",
        ),
    ];
    let swift_files = [
        (
            "src/A.swift",
            "import Dialog\nstruct A: View { var body: some View { Dialog.Root() } }",
        ),
        (
            "src/B.swift",
            "import Popover\nstruct B: View { var body: some View { Popover.Root() } }",
        ),
    ];
    for (language, files, packages) in [
        (
            "compose",
            &compose_files,
            ["widgets.dialog", "widgets.popover"],
        ),
        ("swift", &swift_files, ["Dialog", "Popover"]),
    ] {
        let mut components = [
            serde_json::json!({"id":"dialog.root", "symbol":"Root", "package":packages[0]}),
            serde_json::json!({"id":"popover.root", "symbol":"Root", "package":packages[1]}),
        ];
        for _ in 0..2 {
            let facts =
                scan_registry_fixture(language, files, &components, |request| match language {
                    "compose" => ComposeLanguage::new().scan(&request).expect("compose scan"),
                    "swift" => SwiftLanguage::new().scan(&request).expect("swift scan"),
                    _ => unreachable!(),
                });
            for ((file, _), package) in files.iter().zip(packages) {
                let site = facts
                    .usage_sites
                    .iter()
                    .find(|site| site.location.file == *file && site.symbol == "Root")
                    .unwrap_or_else(|| panic!("missing {language} Root usage in {file}"));
                let key = format!("{package}#Root");
                assert_parity(
                    site,
                    ExpectedOutcome {
                        match_status: MatchStatus::Resolved,
                        callee_origin: CalleeOrigin::Registry,
                        evidence_kind: ResolutionEvidenceKind::RegistryPackageMatch,
                    },
                    language,
                    file,
                );
                assert_eq!(site.registry_symbol.as_deref(), Some(key.as_str()));
                assert_eq!(
                    site.qualified_symbol.as_deref(),
                    Some(format!("{package}.Root").as_str())
                );
                assert!(facts.design_system_components.iter().any(|component| {
                    component.symbol == "Root"
                        && component.registry_symbol == key
                        && component.id == format!("ds.{key}")
                }));
            }
            assert_eq!(facts.counts.registry.used_component_count, 2);
            components.reverse();
        }
    }
}

struct PackageScenario {
    language: &'static str,
    tagged: [&'static str; 2],
    unknown_source: (&'static str, &'static str),
    foreign_source: (&'static str, &'static str),
}

const PACKAGE_SCENARIOS: [PackageScenario; 3] = [
    PackageScenario {
        language: "react",
        tagged: ["@radix-ui/react-dialog", "@radix-ui/react-popover"],
        unknown_source: (
            "src/Screen.tsx",
            "export function Screen() { return <Root />; }",
        ),
        foreign_source: (
            "src/Screen.tsx",
            "import { Root } from 'foreign';\nexport function Screen() { return <Root />; }",
        ),
    },
    PackageScenario {
        language: "compose",
        tagged: ["widgets.dialog", "widgets.popover"],
        unknown_source: ("src/Screen.kt", "@Composable\nfun Screen() { Root() }"),
        foreign_source: (
            "src/Screen.kt",
            "import foreign.Root\n@Composable\nfun Screen() { Root() }",
        ),
    },
    PackageScenario {
        language: "swift",
        tagged: ["Dialog", "Popover"],
        unknown_source: (
            "src/Screen.swift",
            "struct Screen: View { var body: some View { Root() } }",
        ),
        foreign_source: (
            "src/Screen.swift",
            "import Foreign\nstruct Screen: View { var body: some View { Root() } }",
        ),
    },
];

fn scan_for_language(language: &str, request: &ScanRequest) -> Result<ScanFacts, String> {
    match language {
        "react" => ReactLanguage::new()
            .scan(request)
            .map_err(|err| err.to_string()),
        "compose" => ComposeLanguage::new()
            .scan(request)
            .map_err(|err| err.to_string()),
        "swift" => SwiftLanguage::new()
            .scan(request)
            .map_err(|err| err.to_string()),
        _ => panic!("unexpected test language {language}"),
    }
}

impl PackageScenario {
    fn scan(
        &self,
        source: (&str, &str),
        components: &[serde_json::Value],
    ) -> Result<ScanFacts, String> {
        scan_registry_fixture(self.language, &[source], components, |request| {
            scan_for_language(self.language, &request)
        })
    }
}

#[test]
fn parity_missing_package_evidence_does_not_choose_between_tagged_targets() {
    for scenario in &PACKAGE_SCENARIOS {
        let [first, second] = scenario.tagged;
        let first_key = format!("{first}#Root");
        let second_key = format!("{second}#Root");
        let mut components = vec![
            serde_json::json!({"id":"legacy", "symbol":"Root"}),
            serde_json::json!({"id":"first", "symbol":"Root", "package":first}),
        ];

        let unique = scenario.scan(scenario.unknown_source, &components).unwrap();
        let site = usage_for(&unique, "Root");
        assert_eq!(
            site.match_status,
            MatchStatus::Candidate,
            "{}",
            scenario.language
        );
        assert_eq!(site.registry_symbol.as_deref(), Some(first_key.as_str()));
        assert!(
            unique.design_system_components.iter().any(|component| {
                component.id == "ds.Root" && component.registry_symbol == "Root"
            })
        );

        components.push(serde_json::json!({"id":"second", "symbol":"Root", "package":second}));
        for _ in 0..2 {
            for (source, evidence) in [
                (
                    scenario.unknown_source,
                    ResolutionEvidenceKind::NoMatchingDefinition,
                ),
                (
                    scenario.foreign_source,
                    ResolutionEvidenceKind::PackageMismatch,
                ),
            ] {
                let facts = scenario.scan(source, &components).unwrap();
                let site = usage_for(&facts, "Root");
                assert_eq!(
                    site.match_status,
                    MatchStatus::Unresolved,
                    "{}",
                    scenario.language
                );
                assert_eq!(site.resolution_evidence.kind, evidence);
                assert_eq!(site.registry_symbol, None);
                assert!(
                    facts
                        .design_system_components
                        .iter()
                        .any(|component| { component.registry_symbol == second_key })
                );
                assert_eq!(facts.counts.registry.used_component_count, 0);
            }
            components.reverse();
        }
    }
}

#[test]
fn parity_declared_name_wins_over_aliases_and_equal_precedence_is_invalid() {
    for scenario in &PACKAGE_SCENARIOS {
        let package = scenario.tagged[0];
        let mut components = vec![
            serde_json::json!({"id":"first", "symbol":"Alpha", "package":package, "aliases":["Root", "Root"]}),
            serde_json::json!({"id":"second", "symbol":"Beta", "package":package, "aliases":["Root"]}),
            serde_json::json!({"id":"exact", "symbol":"Root", "package":package}),
        ];
        for _ in 0..2 {
            let facts = scenario.scan(scenario.unknown_source, &components).unwrap();
            let site = usage_for(&facts, "Root");
            assert_eq!(site.match_status, MatchStatus::Candidate);
            assert_eq!(
                site.registry_symbol.as_deref(),
                Some("Root"),
                "{}",
                scenario.language
            );
            components.reverse();
        }

        components.pop();
        let error = scenario
            .scan(scenario.unknown_source, &components)
            .expect_err("two equivalent aliases within one package must be rejected");
        assert!(
            error.contains("registry") && error.contains("Root"),
            "{error}"
        );
    }
}

#[test]
fn parity_rejects_generated_component_identity_collisions() {
    for scenario in &PACKAGE_SCENARIOS {
        let [first, second] = scenario.tagged;
        let key = format!("{first}#Root");
        let components = [
            serde_json::json!({"id":"first", "symbol":"Root", "package":first}),
            serde_json::json!({"id":"second", "symbol":"Root", "package":second}),
            serde_json::json!({"id":"literal", "symbol":key}),
        ];
        let error = scenario
            .scan(scenario.unknown_source, &components)
            .expect_err("a generated identity must not collide with a literal symbol");
        assert!(
            error.contains("registry") && error.contains(&key),
            "{error}"
        );

        let same_package = [
            serde_json::json!({"id":"first", "symbol":"Root", "package":first}),
            serde_json::json!({"id":"second", "symbol":"Root", "package":first}),
        ];
        let error = scenario
            .scan(scenario.unknown_source, &same_package)
            .expect_err("duplicate declared symbols within one package must be rejected");
        assert!(
            error.contains("registry") && error.contains(&key),
            "{error}"
        );
    }
}

#[test]
fn react_member_paths_use_runtime_imports_in_scope() {
    let facts = scan_registry_fixture(
        "react",
        &[(
            "src/App.tsx",
            r#"import { Dialog as D, type Dialog as InlineType } from "widgets";
import * as NS from "widgets";
import type * as DeclType from "widgets";
export function App() {
    const shadow = (NS: typeof D) => <NS.Dialog.Panel.Root />;
    return <><D.Panel.Root /><NS.Dialog.Panel.Root /><InlineType.Panel.Root /><DeclType.Dialog.Panel.Root /></>;
}"#,
        )],
        &[serde_json::json!({
            "id":"dialog.root", "symbol":"PanelRoot", "package":"widgets",
            "aliases":["Dialog.Panel.Root"]
        })],
        |request| ReactLanguage::new().scan(&request).expect("react scan"),
    );

    for written in ["D.Panel.Root", "NS.Dialog.Panel.Root"] {
        let site = usage_for(&facts, written);
        assert_eq!(site.match_status, MatchStatus::Resolved, "{written}");
        assert_eq!(
            site.registry_symbol.as_deref(),
            Some("PanelRoot"),
            "{written}"
        );
        assert_eq!(
            site.qualified_symbol.as_deref(),
            Some("widgets#Dialog.Panel.Root"),
            "{written}"
        );
        assert_eq!(site.location.line, 6, "{written}");
    }
    for written in ["InlineType.Panel.Root", "DeclType.Dialog.Panel.Root"] {
        let site = usage_for(&facts, written);
        assert_eq!(site.match_status, MatchStatus::Unresolved, "{written}");
        assert_eq!(site.qualified_symbol, None, "{written}");
    }
    assert_eq!(facts.counts.raw_invocations.resolved, 2);
}

#[test]
fn react_external_default_imports_use_declared_aliases_and_legacy_names() {
    let facts = scan_registry_fixture(
        "react",
        &[(
            "src/App.tsx",
            r#"import Card from "widgets";
import NamedCard from "named-widgets";
import LegacyCard from "old-widgets";
export const App = () => <><Card.Panel /><NamedCard /><LegacyCard /></>;"#,
        )],
        &[
            serde_json::json!({"id":"default.card", "symbol":"DefaultCard", "package":"widgets", "aliases":["default.Panel"]}),
            serde_json::json!({"id":"named.card", "symbol":"NamedCard", "package":"named-widgets"}),
            serde_json::json!({"id":"legacy.card", "symbol":"LegacyCard"}),
        ],
        |request| ReactLanguage::new().scan(&request).expect("react scan"),
    );

    for (written, target, qualified, evidence) in [
        (
            "Card.Panel",
            Some("DefaultCard"),
            "widgets#default.Panel",
            ResolutionEvidenceKind::RegistryPackageMatch,
        ),
        (
            "NamedCard",
            None,
            "named-widgets#default",
            ResolutionEvidenceKind::NoMatchingDefinition,
        ),
        (
            "LegacyCard",
            Some("LegacyCard"),
            "old-widgets#default",
            ResolutionEvidenceKind::RegistryNameOnlyLegacy,
        ),
    ] {
        let site = usage_for(&facts, written);
        assert_eq!(site.registry_symbol.as_deref(), target, "{written}");
        assert_eq!(
            site.qualified_symbol.as_deref(),
            Some(qualified),
            "{written}"
        );
        assert_eq!(site.resolution_evidence.kind, evidence, "{written}");
    }
}
