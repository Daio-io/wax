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
    fs::write(
        repo.path().join("src/index.tsx"),
        r#"export const App = () => <div className="bg-(--blue-9)" />;"#,
    )
    .unwrap();
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

fn scan_package_fixture(repo: &std::path::Path, config: Value) -> wax_contract::ScanFacts {
    let output = run_stdio_request(&json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "react",
        "repo_root": repo,
        "snapshot_id": "snap-copied-package",
        "config": config
    }));
    let WirePackResponse::ScanFacts { facts, .. } = serde_json::from_str(output.trim()).unwrap()
    else {
        panic!("expected scan facts, got {output}");
    };
    *facts
}

#[test]
fn stdio_scan_attributes_copied_components_through_path_imports() {
    use wax_contract::{MatchStatus, ResolutionEvidenceKind};

    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("src/components/ui")).unwrap();
    fs::create_dir_all(root.join("src/app")).unwrap();
    fs::write(
        root.join("src/components/ui/button.tsx"),
        "export function Button() { return <button />; }",
    )
    .unwrap();
    fs::write(
        root.join("src/components/ui/index.ts"),
        r#"export { Button as Primary } from "./button";"#,
    )
    .unwrap();
    fs::write(
        root.join("src/app/ui.ts"),
        r#"export { Primary as Action } from "../components/ui";"#,
    )
    .unwrap();
    fs::write(
        root.join("src/app/page.tsx"),
        r#"import { Button as TsButton } from "@/components/ui/button";
import { Button as RelativeButton } from "../components/ui/button";
import { Primary as AliasedButton } from "ui";
import { Action as BarrelButton } from "./ui";
export function Page() { return <><TsButton /><RelativeButton /><AliasedButton /><BarrelButton /></>; }"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        root.join("react.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button","package":"neobrutalism-components"}]}"#,
    )
    .unwrap();

    let facts = scan_package_fixture(
        root,
        json!({
            "registry": "react.json",
            "roots": ["src"],
            "tsconfig": "tsconfig.json",
            "aliases": {"ui": ["src/components/ui"]},
            "packages": {"neobrutalism-components": {"exports": {"./*": "src/components/ui/*"}}}
        }),
    );

    assert_eq!(facts.usage_sites.len(), 4);
    for site in facts.usage_sites {
        assert_eq!(site.match_status, MatchStatus::Resolved, "{}", site.symbol);
        assert_eq!(site.registry_symbol.as_deref(), Some("Button"));
        assert_eq!(
            site.resolution_evidence.kind,
            ResolutionEvidenceKind::RegistryPackageMatch
        );
        assert_eq!(
            site.resolution_evidence.package.as_deref(),
            Some("neobrutalism-components")
        );
        assert!(
            site.qualified_symbol
                .unwrap()
                .starts_with("neobrutalism-components#")
        );
    }
}

#[test]
fn stdio_scan_attributes_namespace_members_through_unowned_barrels() {
    use wax_contract::{MatchStatus, ResolutionEvidenceKind};

    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("src/ui")).unwrap();
    fs::create_dir_all(root.join("src/app")).unwrap();
    fs::write(
        root.join("src/ui/button.tsx"),
        "export function Button() { return <button />; }",
    )
    .unwrap();
    fs::write(
        root.join("src/app/barrel.ts"),
        r#"export { Button } from "../ui/button";"#,
    )
    .unwrap();
    fs::write(
        root.join("src/app/page.tsx"),
        r#"import * as UI from "./barrel";
export function Page() { return <UI.Button />; }"#,
    )
    .unwrap();
    fs::write(
        root.join("react.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button","package":"copied-ui"}]}"#,
    )
    .unwrap();

    let facts = scan_package_fixture(
        root,
        json!({
            "registry": "react.json",
            "roots": ["src"],
            "packages": {"copied-ui": {"exports": {"./*": "src/ui/*"}}}
        }),
    );

    let [site] = facts.usage_sites.as_slice() else {
        panic!("expected one namespace usage, got {:?}", facts.usage_sites);
    };
    assert_eq!(site.match_status, MatchStatus::Resolved);
    assert_eq!(site.registry_symbol.as_deref(), Some("Button"));
    assert_eq!(
        site.resolution_evidence.kind,
        ResolutionEvidenceKind::RegistryPackageMatch
    );
    assert_eq!(
        site.resolution_evidence.package.as_deref(),
        Some("copied-ui")
    );
    assert_eq!(site.qualified_symbol.as_deref(), Some("copied-ui#Button"));
}

#[test]
fn stdio_scan_excludes_owned_component_and_style_facts() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("src/components/ui")).unwrap();
    fs::write(
        root.join("src/components/ui/button.tsx"),
        r##"export function Button() { return <button />; }
export function AlertDialog() {
    return <Button className="bg-(--blue-9) text-[10px]" style={{ color: "#fff", background: theme.blue }} />;
}"##,
    )
    .unwrap();
    fs::write(
        root.join("src/App.tsx"),
        r##"import { Button } from "./components/ui/button";
export function App() { return <Button style={{ color: "#000", background: theme.blue }} />; }"##,
    )
    .unwrap();
    for file in ["src/app.css", "src/components/ui/button.css"] {
        fs::write(root.join(file), ".a { color: var(--blue-9); }").unwrap();
    }
    fs::write(
        root.join("react.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}],"tokens":[{"id":"css.blue-9","key":"var(--blue-9)","category":"color"},{"id":"theme.blue","key":"theme.blue","category":"color"}]}"#,
    )
    .unwrap();

    let mut config = json!({"registry": "react.json", "roots": ["src"]});
    let baseline = scan_package_fixture(root, config.clone());
    let is_owned = |file: &str| file.starts_with("src/components/ui/");
    assert!(
        baseline
            .usage_sites
            .iter()
            .any(|site| is_owned(&site.location.file))
    );
    assert!(
        baseline
            .local_components
            .iter()
            .any(|site| is_owned(&site.location.file))
    );
    assert!(
        baseline
            .hardcoded_style_sites
            .iter()
            .any(|site| is_owned(&site.location.file) && site.value == "10px")
    );
    for file in [
        "src/components/ui/button.tsx",
        "src/components/ui/button.css",
    ] {
        assert!(
            baseline
                .token_sites
                .iter()
                .any(|site| site.location.file == file),
            "{file}"
        );
    }

    assert!(baseline.token_sites.iter().any(|site| {
        site.location.file == "src/components/ui/button.tsx" && site.token_id == "css.blue-9"
    }));

    config["packages"] = json!({"copied-ui": {"exports": {"./*": "src/components/ui/*"}}});
    let facts = scan_package_fixture(root, config);
    assert_eq!(facts.metrics.files_scanned, 4);
    assert_eq!(facts.usage_sites.len(), 1);
    assert_eq!(facts.usage_sites[0].location.file, "src/App.tsx");
    assert_eq!(
        facts.usage_sites[0].resolution_evidence.kind,
        wax_contract::ResolutionEvidenceKind::RegistryNameOnlyLegacy
    );
    assert_eq!(
        facts.usage_sites[0].match_status,
        wax_contract::MatchStatus::Resolved
    );
    assert_eq!(facts.local_components.len(), 1);
    assert_eq!(facts.local_components[0].location.file, "src/App.tsx");
    assert_eq!(facts.hardcoded_style_sites.len(), 1);
    assert_eq!(facts.hardcoded_style_sites[0].location.file, "src/App.tsx");
    let token_files: Vec<_> = facts
        .token_sites
        .iter()
        .map(|site| site.location.file.as_str())
        .collect();
    assert_eq!(token_files, ["src/App.tsx", "src/app.css"]);
}

#[test]
fn stdio_scan_reports_parse_failures_in_owned_files() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("src/ui")).unwrap();
    fs::write(
        root.join("src/ui/bad.tsx"),
        "export function Broken() { return <button><span></button>; }",
    )
    .unwrap();
    fs::write(root.join("src/ui/bad.css"), ".broken { color: ").unwrap();
    fs::write(
        root.join("react.json"),
        r#"{"schema_version":1,"components":[]}"#,
    )
    .unwrap();

    let facts = scan_package_fixture(
        root,
        json!({
            "registry": "react.json",
            "roots": ["src"],
            "packages": {"copied-ui": {"exports": {"./*": "src/ui/*"}}}
        }),
    );

    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.metrics.files_scanned, 2);
    let failures: Vec<_> = facts
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "parse_failed")
        .map(|diagnostic| diagnostic.location.as_ref().unwrap().file.as_str())
        .collect();
    assert_eq!(failures, ["src/ui/bad.css", "src/ui/bad.tsx"]);
}

#[test]
fn stdio_scan_prefers_specific_package_owners_and_warns_on_a_tie() {
    use wax_contract::{DiagnosticSeverity, MatchStatus, ResolutionEvidenceKind};

    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("src/ui/nested")).unwrap();
    for (file, symbol) in [
        ("src/ui/button.tsx", "Button"),
        ("src/ui/nested/card.tsx", "Card"),
        ("src/ui/nested/dialog.tsx", "Dialog"),
    ] {
        fs::write(
            root.join(file),
            format!("export function {symbol}() {{ return <div />; }}"),
        )
        .unwrap();
    }
    fs::write(
        root.join("src/App.tsx"),
        r#"import { Button } from "./ui/button";
import { Card } from "./ui/nested/card";
import { Dialog } from "./ui/nested/dialog";
export function App() { return <><Button /><Card /><Dialog /></>; }"#,
    )
    .unwrap();
    fs::write(
        root.join("react.json"),
        json!({
            "schema_version": 1,
            "components": [
                {"id":"ds.button","symbol":"Button","package":"a-ui"},
                {"id":"ds.card","symbol":"Card","package":"a-ui"},
                {"id":"ds.dialog","symbol":"Dialog","package":"z-exact"}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let facts = scan_package_fixture(
        root,
        json!({
            "registry": "react.json",
            "roots": ["src"],
            "packages": {
                "a-ui": {"exports": {"./*": "src/ui/*"}},
                "b-ui": {"exports": {"./*": "src/ui/*.tsx", "./other/*": "src/ui/*"}},
                "z-nested": {"exports": {"./*": "src/ui/nested/*"}},
                "z-exact": {"exports": {".": "src/ui/nested/dialog.tsx"}}
            }
        }),
    );
    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.usage_sites.len(), 3);
    for (symbol, package, status, kind) in [
        (
            "Button",
            "a-ui",
            MatchStatus::Resolved,
            ResolutionEvidenceKind::RegistryPackageMatch,
        ),
        (
            "Card",
            "z-nested",
            MatchStatus::Unresolved,
            ResolutionEvidenceKind::PackageMismatch,
        ),
        (
            "Dialog",
            "z-exact",
            MatchStatus::Resolved,
            ResolutionEvidenceKind::RegistryPackageMatch,
        ),
    ] {
        let site = facts
            .usage_sites
            .iter()
            .find(|site| site.symbol == symbol)
            .unwrap();
        assert_eq!(
            site.resolution_evidence.package.as_deref(),
            Some(package),
            "{symbol}"
        );
        assert_eq!(site.match_status, status, "{symbol}");
        assert_eq!(site.resolution_evidence.kind, kind, "{symbol}");
        assert_eq!(
            site.qualified_symbol.as_deref(),
            Some(format!("{package}#{symbol}").as_str())
        );
    }
    let [warning] = facts.diagnostics.as_slice() else {
        panic!("expected one conflict warning, got {:?}", facts.diagnostics);
    };
    assert_eq!(warning.severity, DiagnosticSeverity::Warning);
    assert_eq!(warning.code, "package_ownership_conflict");
    for text in ["a-ui", "b-ui", "src/ui/button.tsx"] {
        assert!(warning.message.contains(text), "{warning:?}");
    }
}

fn scan_tailwind_fixture(source: &str, properties: &[&str]) -> wax_contract::ScanFacts {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/nested")).unwrap();
    fs::write(repo.path().join("src/App.tsx"), source).unwrap();
    let tokens = properties
        .iter()
        .map(|property| {
            json!({
                "id": format!("css.{property}"),
                "key": format!("var(--{property})"),
                "category": "color"
            })
        })
        .collect::<Vec<_>>();
    let facts = scan_css_fixture(repo.path(), &tokens);
    assert_eq!(facts.status, ScanStatus::Complete);
    facts
}

#[test]
fn stdio_scan_reports_arbitrary_text_values_without_a_registry() {
    use wax_contract::{StyleContext, TokenCategory};

    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="text-[10px] bg-[#ffffff]" />;"#,
        &[],
    );
    let sites = facts
        .hardcoded_style_sites
        .iter()
        .map(|site| {
            (
                site.id.as_str(),
                site.location.column,
                site.value.as_str(),
                site.category,
                site.context,
                site.parent.as_ref().map(|parent| parent.symbol.as_str()),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        sites,
        [
            (
                "hardcoded.react:src/App.tsx:1:42:Typography",
                Some(42),
                "10px",
                TokenCategory::Typography,
                StyleContext::Typography,
                Some("App")
            ),
            (
                "hardcoded.react:src/App.tsx:1:54:Color",
                Some(54),
                "#ffffff",
                TokenCategory::Color,
                StyleContext::Color,
                Some("App")
            ),
        ]
    );
}

#[test]
fn stdio_scan_classifies_tailwind_arbitrary_utilities_and_omits_non_literals() {
    let facts = scan_tailwind_fixture(
        r##"export const App = () => <div className="hover:border-x-[rgb(1_2_3)] md:text-[color:red]
            font-[450] leading-[1.3] tracking-[-.05em] px-[13px] scroll-pt-[1rem] -mt-[3px]
            md:-mt-[8px] scroll-mx-[4px] gap-x-[.5em] space-y-[7px] min-w-[25%] max-h-[40dvh]
            [&_svg]:size-[14px] inset-x-[1in] translate-y-[2px] rounded-bl-[6px]
            shadow-[4px_4px_0_0_#000] inset-shadow-[#0008] drop-shadow-[inherit]
            text-shadow-[2px] text-[length:1rem] w-[percentage:50%] leading-[number:1.5]
            text-[color:brand\_blue] p-[length:3px]"
        data-unused="p-[23px]" />;
const ignored = cn(`text-[length:var(--x)] w-[calc(100%-2rem)] bg-[url(/a.png)]
    bg-[var(--x)] p-[theme(spacing.4)] m-[env(safe-area-inset-top)] bg-[color:--x]
    border-[3px] grid-cols-[1fr_auto] font-[family-name:serif] font-[3px]
    bg-[#abcdz] w-[fit-content] w-[2potatoes] p-[#fff]`);
"##,
        &[],
    );
    let sites = facts
        .hardcoded_style_sites
        .iter()
        .map(|site| (site.value.as_str(), site.category, site.context))
        .collect::<Vec<_>>();
    use wax_contract::{StyleContext as Context, TokenCategory as Category};

    assert_eq!(
        sites,
        [
            ("rgb(1 2 3)", Category::Color, Context::Color),
            ("red", Category::Color, Context::Color),
            ("450", Category::Typography, Context::Typography),
            ("1.3", Category::Typography, Context::Typography),
            ("-.05em", Category::Typography, Context::Typography),
            ("13px", Category::Spacing, Context::Padding),
            ("1rem", Category::Spacing, Context::Padding),
            ("-3px", Category::Spacing, Context::Margin),
            ("-8px", Category::Spacing, Context::Margin),
            ("4px", Category::Spacing, Context::Margin),
            (".5em", Category::Spacing, Context::Gap),
            ("7px", Category::Spacing, Context::Gap),
            ("25%", Category::Spacing, Context::Width),
            ("40dvh", Category::Spacing, Context::Height),
            ("14px", Category::Spacing, Context::Size),
            ("1in", Category::Spacing, Context::Unknown),
            ("2px", Category::Spacing, Context::Unknown),
            ("6px", Category::Radius, Context::Radius),
            ("4px 4px 0 0 #000", Category::Elevation, Context::Elevation),
            ("#0008", Category::Color, Context::Color),
            ("inherit", Category::Elevation, Context::Elevation),
            ("2px", Category::Elevation, Context::Elevation),
            ("1rem", Category::Typography, Context::Typography),
            ("50%", Category::Spacing, Context::Width),
            ("1.5", Category::Typography, Context::Typography),
            ("brand_blue", Category::Color, Context::Color),
            ("3px", Category::Spacing, Context::Padding),
        ]
    );
}

#[test]
fn stdio_scan_reports_tailwind_references_and_literals_in_the_same_class_string() {
    let facts = scan_tailwind_fixture(
        r##"export const App = () => <div className={cn("bg-main bg-[#fff]")} />;"##,
        &["color-main"],
    );
    let [reference] = facts.token_sites.as_slice() else {
        panic!("expected one token reference, got {:?}", facts.token_sites);
    };
    let [literal] = facts.hardcoded_style_sites.as_slice() else {
        panic!(
            "expected one literal, got {:?}",
            facts.hardcoded_style_sites
        );
    };

    assert_eq!(reference.key, "var(--color-main)");
    assert_eq!(literal.value, "#fff");
    assert_eq!(literal.parent.as_ref().unwrap().symbol, "App");
}

#[test]
fn stdio_scan_attributes_tailwind_class_to_its_column_and_component() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="hover:bg-main/50" />;"#,
        &["color-main"],
    );
    let [site] = facts.token_sites.as_slice() else {
        panic!(
            "expected one Tailwind reference, got {:?}",
            facts.token_sites
        );
    };

    assert_eq!(site.id, "token.react:src/App.tsx:1:42:css.color-main");
    assert_eq!(site.key, "var(--color-main)");
    assert_eq!(site.category, wax_contract::TokenCategory::Color);
    assert_eq!(site.parent.as_ref().unwrap().symbol, "App");
}

#[test]
fn stdio_scan_reads_class_expressions_and_helper_arguments() {
    let facts = scan_tailwind_fixture(
        r#"const label = "bg-ignored";
const a = cn(on ? "bg-main" : "bg-alt");
const b = clsx({ "bg-conditional": on, ["bg-computed"]: on });
const c = cva("bg-main", { variants: { "bg-ignored": { main: "bg-alt" } } });
const d = tv({ base: "bg-main", variants: { "bg-ignored": { main: "bg-alt" } } });
const e = twMerge({ "bg-ignored": "bg-main" });
const f = helpers.cn("bg-ignored");
export const App = () => <div className={cn(
    cn("bg-main"), ["bg-alt" as string], on && ("bg-main" + " bg-alt"),
    make("bg-main"), () => "bg-ignored", <div title="bg-ignored" />,
    tv({ "bg-ignored": "bg-main" })
)} />;
export const Other = () => <div class={on ? "bg-alt" : "bg-main"} />;
"#,
        &[
            "color-main",
            "color-alt",
            "color-conditional",
            "color-computed",
            "color-ignored",
        ],
    );
    let sites = facts
        .token_sites
        .iter()
        .map(|site| (site.location.line, site.token_id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (2, "css.color-main"),
            (2, "css.color-alt"),
            (3, "css.color-conditional"),
            (3, "css.color-computed"),
            (4, "css.color-main"),
            (4, "css.color-alt"),
            (5, "css.color-main"),
            (5, "css.color-alt"),
            (6, "css.color-main"),
            (9, "css.color-main"),
            (9, "css.color-alt"),
            (9, "css.color-main"),
            (9, "css.color-alt"),
            (10, "css.color-main"),
            (11, "css.color-main"),
            (13, "css.color-alt"),
            (13, "css.color-main"),
        ]
    );
}

#[test]
fn stdio_scan_preserves_complete_classes_across_concatenation_and_templates() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className={cn(
  "bg-main" + "-hover",
  "bg-" + "main",
  "bg-main " + "bg-alt" + tone + "bg-main",
  "bg-main-hover " + (on ? "bg-alt" : "bg-main") + " bg-main-hover",
  "bg-alt " + tone + " bg-alt",
  `${on ? "bg-main" : "bg-alt"}`,
  `bg-${tone}`
)} />;"#,
        &["color-main", "color-main-hover", "color-alt", "color-hover"],
    );
    let sites = facts
        .token_sites
        .iter()
        .map(|site| (site.location.line, site.token_id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (4, "css.color-main"),
            (5, "css.color-main-hover"),
            (5, "css.color-alt"),
            (5, "css.color-main"),
            (5, "css.color-main-hover"),
            (6, "css.color-alt"),
            (6, "css.color-alt"),
            (7, "css.color-main"),
            (7, "css.color-alt"),
        ]
    );
}

#[test]
fn stdio_scan_counts_registered_numeric_tokens_but_not_derived_spacing() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="bg-500 font-400 p-500 m-4 w-1/2 bg-500/50" />;"#,
        &[
            "color-500",
            "font-weight-400",
            "spacing-500",
            "spacing",
            "spacing-1",
        ],
    );
    let tokens = facts
        .token_sites
        .iter()
        .map(|site| site.token_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        [
            "css.color-500",
            "css.font-weight-400",
            "css.spacing-500",
            "css.color-500",
        ]
    );
}

#[test]
fn stdio_scan_counts_tokens_referenced_by_slash_modifiers() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="
text-sm/(--body-leading) md:text-sm/tight text-main/tight bg-main/[var(--alpha)]
bg-main/(--color-main) text-sm/6 text-(--body-leading)/tight
" />;"#,
        &[
            "text-sm",
            "body-leading",
            "leading-tight",
            "color-main",
            "alpha",
        ],
    );
    let sites = facts
        .token_sites
        .iter()
        .map(|site| (site.location.line, site.token_id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (2, "css.body-leading"),
            (2, "css.text-sm"),
            (2, "css.leading-tight"),
            (2, "css.text-sm"),
            (2, "css.color-main"),
            (2, "css.alpha"),
            (2, "css.color-main"),
            (3, "css.color-main"),
            (3, "css.text-sm"),
            (3, "css.body-leading"),
        ]
    );
}

#[test]
fn stdio_scan_ignores_condition_values_in_class_helper_objects() {
    let facts = scan_tailwind_fixture(
        r#"const a = clsx({ "bg-main": status === "bg-alt", "bg-other": pick("bg-alt") });
const b = cn({ "bg-main": on ? "bg-alt" : "bg-alt" });
const c = cva("bg-main", { variants: { tone: { alt: "bg-alt" } } });
"#,
        &["color-main", "color-alt", "color-other"],
    );
    let sites = facts
        .token_sites
        .iter()
        .map(|site| (site.location.line, site.token_id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (1, "css.color-main"),
            (1, "css.color-other"),
            (2, "css.color-main"),
            (3, "css.color-main"),
            (3, "css.color-alt"),
        ]
    );
}

#[test]
fn stdio_scan_locates_complete_static_classes_in_templates() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className={cn(`bg-main bg-main`, `
  é bg-main
  bg-ignored${value}bg-ignored
  ${value} bg-main
  bg-ignored\${value}
`)} />;
"#,
        &["color-main", "color-ignored"],
    );
    let sites = facts
        .token_sites
        .iter()
        .map(|site| {
            (
                site.location.line,
                site.location.column,
                site.token_id.as_str(),
                site.parent.as_ref().unwrap().symbol.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (1, Some(46), "css.color-main", "App"),
            (1, Some(54), "css.color-main", "App"),
            (2, Some(5), "css.color-main", "App"),
            (4, Some(12), "css.color-main", "App"),
        ]
    );
}

#[test]
fn stdio_scan_resolves_tailwind_utilities_in_namespace_order() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="
bg-main text-sm text-main-foreground shadow-shadow font-base tracking-wide leading-tight rounded-ee-base
inset-shadow-soft drop-shadow-soft text-shadow-soft ring-offset-edge border-x-edge
gap-x-room max-h-room max-w-room min-w-flex scroll-mt-room scroll-px-room
blur-soft perspective-near aspect-card ease-smooth animate-fade
bg-red-500 p-4 bg-[#fff] ring-offset-missing
" />;"#,
        &[
            "color-main",
            "text-sm",
            "color-sm",
            "color-main-foreground",
            "shadow-shadow",
            "color-shadow",
            "font-weight-base",
            "font-base",
            "tracking-wide",
            "leading-tight",
            "radius-base",
            "inset-shadow-soft",
            "drop-shadow-soft",
            "text-shadow-soft",
            "color-soft",
            "spacing-shadow-soft",
            "color-edge",
            "color-offset-edge",
            "color-x-edge",
            "spacing-room",
            "container-room",
            "spacing-flex",
            "blur-soft",
            "perspective-near",
            "aspect-card",
            "ease-smooth",
            "animate-fade",
            "spacing-4",
            "color-offset-missing",
        ],
    );
    let tokens = facts
        .token_sites
        .iter()
        .map(|site| site.token_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        [
            "css.color-main",
            "css.text-sm",
            "css.color-main-foreground",
            "css.shadow-shadow",
            "css.font-weight-base",
            "css.tracking-wide",
            "css.leading-tight",
            "css.radius-base",
            "css.inset-shadow-soft",
            "css.drop-shadow-soft",
            "css.text-shadow-soft",
            "css.color-edge",
            "css.color-edge",
            "css.spacing-room",
            "css.spacing-room",
            "css.container-room",
            "css.spacing-flex",
            "css.spacing-room",
            "css.spacing-room",
            "css.blur-soft",
            "css.perspective-near",
            "css.aspect-card",
            "css.ease-smooth",
            "css.animate-fade",
            "css.spacing-4",
        ]
    );
}

#[test]
fn stdio_scan_resolves_tailwind_modifiers_and_direct_css_variables() {
    let facts = scan_tailwind_fixture(
        r#"export const App = () => <div className="
!hover:bg-main/50 hover:bg-main! md:-translate-x-boxShadowX
[&_[data-size='a:b']]:focus:bg-main/50 group-hover/item:bg-main
bg-[var(--chart-1)] bg-(--chart-1) bg-(color:--chart-1)
bg-[color:var(--chart-1)] bg-[var(--chart-1,url(https://example.com/x))]/50
[&_[data-value=']:/']]:bg-main/[0.5] bg-[var(--chart-1,url('/chart(1.png'))]/50
bg-[calc(var(--chart-1)*2)] bg-[var(--chart-1,red)_other()] bg-(--missing)
" />;"#,
        &["color-main", "spacing-boxShadowX", "chart-1"],
    );
    let tokens = facts
        .token_sites
        .iter()
        .map(|site| site.token_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        [
            "css.color-main",
            "css.color-main",
            "css.spacing-boxShadowX",
            "css.color-main",
            "css.color-main",
            "css.chart-1",
            "css.chart-1",
            "css.chart-1",
            "css.chart-1",
            "css.chart-1",
            "css.color-main",
            "css.chart-1",
        ]
    );
}

#[test]
fn stdio_scan_reads_class_helper_object_keys_without_entering_methods() {
    let facts = scan_tailwind_fixture(
        r#"const classes = clsx({ "bg-main"() { return "bg-ignored"; } });"#,
        &["color-main", "color-ignored"],
    );
    let tokens = facts
        .token_sites
        .iter()
        .map(|site| site.token_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tokens, ["css.color-main"]);
}

#[test]
fn stdio_scan_reads_optional_class_helper_calls_by_identifier() {
    let facts = scan_tailwind_fixture(
        r#"const classes = clsx?.({ "bg-main": on });
const variants = cva?.({ "bg-ignored": "bg-main" });
const ignored = helpers.cn?.("bg-ignored");
export const App = () => <div className={format?.("bg-main")} />;
"#,
        &["color-main", "color-ignored"],
    );
    let tokens = facts
        .token_sites
        .iter()
        .map(|site| site.token_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tokens, ["css.color-main"; 3]);
}
