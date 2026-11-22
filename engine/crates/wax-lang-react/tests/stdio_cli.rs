use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wax_contract::ScanStatus;
use wax_lang_api::{WireErrorCode, WirePackResponse};

#[test]
fn stdio_cli_advertises_registry_details_v1() {
    let request = json!({
        "type": "capabilities",
        "api_version": 1,
        "language_id": "react"
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    assert!(matches!(
        response,
        WirePackResponse::Capabilities {
            registry_details_v1: true,
            ..
        }
    ));
}

#[test]
fn stdio_cli_emits_registry_details_for_rich_discovery() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");
    let request = json!({
        "type": "discover_registry",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo_root,
        "roots": ["design-system/src"],
        "registry": {
            "schema_version": 1,
            "components": [{"id": "rich", "symbol": "RichButton"}]
        }
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    assert!(matches!(
        response,
        WirePackResponse::RegistryDetails { document, diagnostics, .. }
            if document.components[0].details.is_some() && diagnostics.is_empty()
    ));
}

#[test]
fn stdio_cli_emits_discover_symbols_for_fixture_roots() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo_root.display().to_string(),
        "roots": ["design-system/src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols { symbols, .. } => {
            assert!(symbols.contains(&"Button".to_owned()));
            assert!(!symbols.contains(&"InlineMemo".to_owned()));
            assert!(!symbols.contains(&"InlineRef".to_owned()));
            assert!(symbols.contains(&"MemoButton".to_owned()));
            assert!(!symbols.contains(&"PrivateBadge".to_owned()));
            assert!(!symbols.contains(&"lowerBadge".to_owned()));
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}

#[test]
fn stdio_cli_preserves_legacy_discover_response_shape() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/discover");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo_root,
        "roots": ["design-system/src"]
    });

    let output = run_stdio_request(&request);
    let response: Value = serde_json::from_str(output.trim()).unwrap();
    let mut fields = response
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    fields.sort();

    assert_eq!(
        fields,
        [
            "api_version",
            "components",
            "diagnostics",
            "language_id",
            "symbols",
            "type"
        ]
    );
}

#[test]
fn stdio_cli_returns_config_invalid_for_missing_discover_root() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().display().to_string(),
        "roots": ["missing-subdir"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::ConfigInvalid);
            assert!(message.contains("discovery root does not exist"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn stdio_cli_returns_config_invalid_for_bad_discover_language_id() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "compose",
        "repo_root": temp.path().display().to_string(),
        "roots": ["src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::ConfigInvalid);
            assert!(message.contains("invalid react language id"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn stdio_cli_returns_discover_symbols_with_parse_failure_diagnostic() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let src = temp.path().join("src");
    fs::create_dir_all(&src).expect("src dir should be created");
    fs::write(src.join("Broken.tsx"), "export const Broken = (")
        .expect("broken fixture should be written");

    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().display().to_string(),
        "roots": ["src"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols {
            symbols,
            diagnostics,
            ..
        } => {
            assert!(symbols.is_empty());
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code, "parse_failed");
            assert_eq!(
                diagnostics[0]
                    .location
                    .as_ref()
                    .map(|location| location.file.as_str()),
                Some("src/Broken.tsx")
            );
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}

fn run_stdio_request(request: &Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout must be valid UTF-8")
}

#[test]
fn stdio_cli_returns_api_version_unsupported_for_bad_discover_version() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"discover","api_version":2,"language_id":"react","repo_root":"/tmp/repo","roots":["src"]}
"#,
            )
            .expect("failed to write discover request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::Error {
            api_version,
            language_id,
            code,
            ..
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(code, WireErrorCode::ApiVersionUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_emits_complete_scan_facts_for_small_fixture() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        let repo_root = format!("{}/tests/fixtures/small", env!("CARGO_MANIFEST_DIR"));
        let request = serde_json::json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "react",
            "repo_root": repo_root,
            "snapshot_id": "snap-cli",
            "config": {
                "registry": "design-system/registry.json",
                "roots": ["src"],
                "packages": {
                    "@acme/design-system": {
                        "exports": {
                            ".": "src/ds/index.ts"
                        }
                    }
                }
            }
        });
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(facts.snapshot_id, "snap-cli");
            assert_eq!(facts.status, ScanStatus::Complete);
            assert_eq!(facts.language.id.as_str(), "react");
            assert_eq!(facts.language.parser_name, "swc");
            assert_eq!(
                facts.language.parser_version,
                wax_lang_react::SWC_PARSER_VERSION,
                "parser_version must track the pinned SWC crate version"
            );
            assert_eq!(facts.counts.raw_invocations.total, 5);
            assert_eq!(facts.counts.raw_invocations.resolved, 5);
            assert_eq!(facts.counts.registry.component_count, 2);
            assert_eq!(facts.counts.definitions.local_definition_count, 4);
            assert_eq!(facts.metrics.files_scanned, 6);
            assert!(
                facts
                    .usage_sites
                    .iter()
                    .any(|site| site.symbol == "PrimaryBtn"
                        && site.registry_symbol.as_deref() == Some("PrimaryButton")),
                "alias usage must resolve to PrimaryButton"
            );
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_emits_one_scan_facts_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(
                br#"{"type":"scan","api_version":1,"language_id":"react","repo_root":"/tmp/repo","snapshot_id":"snap-cli","config":{}}
"#,
            )
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(facts.language.id.as_str(), "react");
            assert_eq!(facts.snapshot_id, "snap-cli");
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_returns_partial_scan_facts_for_parse_failures() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let registry_dir = temp.path().join("design-system");
    std::fs::create_dir_all(&registry_dir).expect("registry dir should be created");
    std::fs::write(
        registry_dir.join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
    )
    .expect("registry fixture should be written");
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir should be created");
    std::fs::write(src_dir.join("App.tsx"), "export {}").expect("valid source fixture");
    std::fs::write(
        src_dir.join("Broken.tsx"),
        "export function Broken() { return <button><span></button>; }",
    )
    .expect("invalid source fixture");

    let request = serde_json::json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().to_string_lossy(),
        "snapshot_id": "snap-partial",
        "config": {
            "registry": "design-system/registry.json",
            "roots": ["src"]
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "react");
            assert_eq!(facts.snapshot_id, "snap-partial");
            assert_eq!(facts.status, ScanStatus::Partial);
            assert_eq!(facts.metrics.files_scanned, 2);
            assert!(
                facts
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "parse_failed"),
                "expected parse_failed diagnostic, got: {:?}",
                facts.diagnostics
            );
        }
        other => panic!("expected scan_facts response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_cli_returns_registry_not_found_for_missing_registry() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir should be created");
    std::fs::write(src_dir.join("App.tsx"), "export {}").expect("source fixture");

    let request = serde_json::json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": temp.path().to_string_lossy(),
        "snapshot_id": "snap-missing-registry",
        "config": {
            "registry": "design-system/registry.json",
            "roots": ["src"]
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-react"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-react");

    {
        let stdin = child.stdin.as_mut().expect("child stdin must be piped");
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .expect("failed to write scan request");
    }

    let output = child.wait_with_output().expect("failed to read output");

    assert!(
        output.status.success(),
        "wax-lang-react exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let mut lines = stdout.lines();
    let response: WirePackResponse =
        serde_json::from_str(lines.next().expect("expected one stdout line"))
            .expect("stdout line must be a wire response");

    match response {
        WirePackResponse::Error { code, message, .. } => {
            assert_eq!(code, WireErrorCode::RegistryNotFound);
            assert!(message.contains("react registry not found"));
        }
        other => panic!("expected error response, got {other:?}"),
    }
    assert_eq!(lines.next(), None, "expected exactly one stdout line");
}

#[test]
fn stdio_scan_attributes_radix_jsx_to_the_exported_binding() {
    use wax_contract::{MatchStatus, ResolutionEvidenceKind};

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/App.tsx"),
        r#"import { CheckIcon, Cross1Icon as Cross, WrongIcon as Cross1Icon } from "@radix-ui/react-icons";
import { Dialog, Dialog as D } from "@radix-ui/themes";
import { Dialog as Foreign } from "other-widgets";
export function App() {
    return <><CheckIcon /><Cross /><Dialog.Root /><D.Root /><Cross1Icon /><Foreign.Root /></>;
}"#,
    )
    .unwrap();
    fs::write(
        repo.path().join("react.json"),
        json!({
            "schema_version": 1,
            "components": [
                {"id":"ds.check-icon","symbol":"CheckIcon","package":"@radix-ui/react-icons"},
                {"id":"ds.cross-1-icon","symbol":"Cross1Icon","package":"@radix-ui/react-icons"},
                {"id":"ds.dialog-root","symbol":"Dialog.Root","package":"@radix-ui/themes"}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let output = run_stdio_request(&json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo.path(),
        "snapshot_id": "snap-radix-aliases",
        "config": {"registry": "react.json", "roots": ["src"]}
    }));
    let WirePackResponse::ScanFacts { facts, .. } = serde_json::from_str(output.trim()).unwrap()
    else {
        panic!("expected scan facts, got {output}");
    };

    for (written, symbol, qualified) in [
        ("CheckIcon", "CheckIcon", "@radix-ui/react-icons#CheckIcon"),
        ("Cross", "Cross1Icon", "@radix-ui/react-icons#Cross1Icon"),
        ("Dialog.Root", "Dialog.Root", "@radix-ui/themes#Dialog.Root"),
        ("D.Root", "Dialog.Root", "@radix-ui/themes#Dialog.Root"),
    ] {
        let site = facts
            .usage_sites
            .iter()
            .find(|site| site.symbol == written)
            .unwrap();
        assert_eq!(site.match_status, MatchStatus::Resolved, "{written}");
        assert_eq!(site.registry_symbol.as_deref(), Some(symbol), "{written}");
        assert_eq!(
            site.qualified_symbol.as_deref(),
            Some(qualified),
            "{written}"
        );
    }

    for (written, qualified, evidence) in [
        (
            "Cross1Icon",
            "@radix-ui/react-icons#WrongIcon",
            ResolutionEvidenceKind::NoMatchingDefinition,
        ),
        (
            "Foreign.Root",
            "other-widgets#Dialog.Root",
            ResolutionEvidenceKind::PackageMismatch,
        ),
    ] {
        let site = facts
            .usage_sites
            .iter()
            .find(|site| site.symbol == written)
            .unwrap();
        assert_eq!(site.match_status, MatchStatus::Unresolved, "{written}");
        assert_eq!(site.registry_symbol, None, "{written}");
        assert_eq!(
            site.qualified_symbol.as_deref(),
            Some(qualified),
            "{written}"
        );
        assert_eq!(site.resolution_evidence.kind, evidence, "{written}");
    }
}

#[test]
fn stdio_scan_rejects_invalid_css_variant_links() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join("src")).unwrap();
    for links in [json!(["missing"]), json!("missing"), json!(null)] {
        fs::write(
            repo.path().join("react.json"),
            json!({
                "schema_version": 1,
                "components": [],
                "tokens": [
                    {"id":"css.blue","key":"var(--blue-9)","category":"color","css_variants": links}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let output = run_stdio_request(&json!({
            "type": "scan",
            "api_version": 1,
            "language_id": "react",
            "repo_root": repo.path(),
            "snapshot_id": "snap-invalid-css",
            "config": {"registry": "react.json", "roots": ["src"]}
        }));
        assert!(
            matches!(
                serde_json::from_str::<WirePackResponse>(output.trim()).unwrap(),
                WirePackResponse::Error {
                    code: WireErrorCode::ScanFailed,
                    ..
                }
            ),
            "{output}"
        );
    }
}

fn scan_css_fixture(repo: &std::path::Path, tokens: &[Value]) -> wax_contract::ScanFacts {
    fs::write(
        repo.join("react.json"),
        json!({"schema_version": 1, "components": [], "tokens": tokens}).to_string(),
    )
    .unwrap();
    let output = run_stdio_request(&json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo,
        "snapshot_id": "snap-css",
        "config": {"registry": "react.json", "roots": ["src", "src/nested"], "ignore": ["**/ignored.css"]}
    }));
    let WirePackResponse::ScanFacts { facts, .. } = serde_json::from_str(output.trim()).unwrap()
    else {
        panic!("expected scan facts, got {output}");
    };
    *facts
}

#[test]
fn stdio_scan_counts_one_logical_reference_for_the_radix_css_probe() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/nested")).unwrap();
    fs::write(
        repo.path().join("src/app.css"),
        ".sample { color: var(--blue-9); }",
    )
    .unwrap();

    let facts = scan_css_fixture(
        repo.path(),
        &[
            json!({"id":"css.blue-9","key":"var(--blue-9)","category":"color","css_variants":["blue.blue9","blueDark.blue9","blueP3.blue9","blueDarkP3.blue9"]}),
            json!({"id":"blue.blue9","key":"blue.blue9","category":"color","package":"@radix-ui/colors","value":"#0090ff"}),
            json!({"id":"blueDark.blue9","key":"blueDark.blue9","category":"color","package":"@radix-ui/colors","value":"#0090ff"}),
            json!({"id":"blueP3.blue9","key":"blueP3.blue9","category":"color","package":"@radix-ui/colors","value":"color(display-p3 0.247 0.556 0.969)"}),
            json!({"id":"blueDarkP3.blue9","key":"blueDarkP3.blue9","category":"color","package":"@radix-ui/colors","value":"color(display-p3 0.247 0.556 0.969)"}),
        ],
    );
    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.metrics.files_scanned, 1);
    let [site] = facts.token_sites.as_slice() else {
        panic!("expected one CSS reference, got {:?}", facts.token_sites);
    };
    assert_eq!(site.id, "token.react:src/app.css:1:18:css.blue-9");
    assert_eq!(site.token_id, "css.blue-9");
    assert_eq!(site.key, "var(--blue-9)");
    assert_eq!(site.location.file, "src/app.css");
    assert_eq!((site.location.line, site.location.column), (1, Some(18)));
    assert_eq!(site.parent, None);
    assert_eq!(facts.counts.tokens.token_reference_site_count, 1);
    assert_eq!(facts.counts.tokens.used_token_count, 1);
}

#[test]
fn stdio_scan_matches_literal_css_arguments_and_independent_fallbacks() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/nested")).unwrap();
    fs::write(
        repo.path().join("src/nested/values.css"),
        r#".sample { color: VAR( /* note */ --blue-9 , var(--blue-a9)) }
.unregistered { color: var(--unknown, var(--blue-a9)) }
.multi { /* µ */ color: var(--blue-9); background: var(--blue-9) }
.case { color: var(--Blue-9) }
.invalid { a: var(--blue-9 extra); b: var(--blue-9; red); c: var("--blue-9"); d: var(--bl\ue-9); e: var(--blue-9 + 1) }
.bare { --blue-9: blue; color: --blue-9; content: "var(--blue-9)" }
/* color: var(--blue-9) */
[data-key="var(--blue-9)"] { color: blue }
@supports (color: var(--blue-9)) { .nested { color: blue } }"#,
    )
    .unwrap();
    let facts = scan_css_fixture(
        repo.path(),
        &[
            json!({"id":"css.blue-9","key":"var(--blue-9)","category":"color"}),
            json!({"id":"css.blue-a9","key":"var(--blue-a9)","category":"color","css_variants":["blueA.blueA9","blueDarkA.blueA9","blueP3A.blueA9","blueDarkP3A.blueA9"]}),
            json!({"id":"blueA.blueA9","key":"blueA.blueA9","category":"color","value":"#0090ff"}),
            json!({"id":"blueDarkA.blueA9","key":"blueDarkA.blueA9","category":"color","value":"#0090ff"}),
            json!({"id":"blueP3A.blueA9","key":"blueP3A.blueA9","category":"color","value":"color(display-p3 0 0.412 0.961 / 0.753)"}),
            json!({"id":"blueDarkP3A.blueA9","key":"blueDarkP3A.blueA9","category":"color","value":"color(display-p3 0.251 0.573 0.996 / 0.967)"}),
        ],
    );
    assert_eq!(
        facts.status,
        ScanStatus::Complete,
        "{:?}",
        facts.diagnostics
    );
    assert_eq!(facts.metrics.files_scanned, 1);
    let sites = facts
        .token_sites
        .iter()
        .map(|site| {
            assert_eq!(site.location.file, "src/nested/values.css");
            assert_eq!(site.parent, None);
            (
                site.location.line,
                site.location.column,
                site.token_id.as_str(),
                site.key.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (1, Some(18), "css.blue-9", "var(--blue-9)"),
            (1, Some(45), "css.blue-a9", "var(--blue-a9)"),
            (2, Some(39), "css.blue-a9", "var(--blue-a9)"),
            (3, Some(26), "css.blue-9", "var(--blue-9)"),
            (3, Some(53), "css.blue-9", "var(--blue-9)"),
        ]
    );
}

#[test]
fn stdio_scan_reports_malformed_css_and_keeps_clean_siblings() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/nested")).unwrap();
    fs::write(
        repo.path().join("src/bad.css"),
        ".a { color: var(--blue-9); }\n.b { color: var(--blue-9) ;; background: url( }\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/good.css"),
        ".a { color: var(--blue-9); }",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/ignored.css"),
        ".a { color: var(--blue-9); }",
    )
    .unwrap();
    let facts = scan_css_fixture(
        repo.path(),
        &[json!({"id":"css.blue-9","key":"var(--blue-9)","category":"color"})],
    );
    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.metrics.files_scanned, 2);
    let [diagnostic] = facts.diagnostics.as_slice() else {
        panic!("expected one diagnostic, got {:?}", facts.diagnostics);
    };
    assert_eq!(diagnostic.code, "parse_failed");
    assert_eq!(diagnostic.location.as_ref().unwrap().file, "src/bad.css");
    let files: Vec<_> = facts
        .token_sites
        .iter()
        .map(|site| site.location.file.as_str())
        .collect();
    assert!(files.contains(&"src/good.css"), "{files:?}");
    assert!(!files.contains(&"src/ignored.css"), "{files:?}");
}

#[test]
fn stdio_scan_ignores_css_without_eligible_unscoped_keys() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/nested")).unwrap();
    fs::write(
        repo.path().join("src/app.css"),
        ".a { color: var(--blue-9); }\n.broken { color: ",
    )
    .unwrap();
    fs::write(repo.path().join("src/index.ts"), "export const x = 1;\n").unwrap();
    let facts = scan_css_fixture(
        repo.path(),
        &[
            json!({"id":"scoped.blue-9","key":"var(--blue-9)","category":"color","package":"@scope/css","value":"#0090ff"}),
        ],
    );
    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.metrics.files_scanned, 1);
    assert!(facts.token_sites.is_empty());
    assert!(facts.diagnostics.is_empty());
}
