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
        "roots": ["src"]
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

fn scan_request(
    repo_root: &Path,
    language_id: &str,
    config: serde_json::Value,
    snapshot_id: &str,
    scan: impl FnOnce(ScanRequest) -> ScanFacts,
) -> ScanFacts {
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

export function Screen() {
  return <><View /><AsyncImage /><UnknownCard /></>;
}"#,
        Some("@acme/design-system"),
        false,
    );
    for (symbol, origin, scenario) in [
        ("View", CalleeOrigin::Framework, "framework"),
        ("AsyncImage", CalleeOrigin::External, "external"),
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

    let swift = scan_swift(
        r#"import SwiftUI

struct Screen: View {
    var body: some View {
        SwiftUI.Text("Title")
        Kingfisher.KFImage()
        UnknownCard()
    }
}"#,
        Some("AcmeDesignSystem"),
    );
    for (symbol, origin, scenario) in [
        ("Text", CalleeOrigin::Framework, "framework"),
        ("KFImage", CalleeOrigin::External, "external"),
        ("UnknownCard", CalleeOrigin::Application, "application"),
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
