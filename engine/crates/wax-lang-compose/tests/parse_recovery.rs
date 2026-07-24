use std::path::{Path, PathBuf};

use wax_contract::{LanguageId, ScanStatus};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_compose::ComposeLanguage;

const FIXTURE_ROOT: &str = "tests/fixtures/kotlin-syntax";
const COMMON_AFTER_CALL: &str = "PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))";

#[test]
fn known_valid_syntax_is_byte_preserving() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT);
    let facts = scan_fixture(&fixture_root, "snap-parse-recovery");

    assert_eq!(facts.status, ScanStatus::Complete);
    assert!(
        facts
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "parse_failed"),
        "expected valid syntax fixtures to scan without parse_failed diagnostics: {:?}",
        facts.diagnostics
    );

    for (file, before_name, after_name) in [
        (
            "app/src/main/kotlin/SuspendLambda.kt",
            "BeforeSuspendLambda",
            "AfterSuspendLambda",
        ),
        (
            "app/src/main/kotlin/WhenGuard.kt",
            "BeforeWhenGuard",
            "AfterWhenGuard",
        ),
        (
            "app/src/main/kotlin/AnnotatedFunctionType.kt",
            "BeforeAnnotatedFunctionType",
            "AfterAnnotatedFunctionType",
        ),
        (
            "app/src/main/kotlin/ExplicitBackingField.kt",
            "BeforeExplicitBackingField",
            "AfterExplicitBackingField",
        ),
        (
            "app/src/main/kotlin/ContextParameter.kt",
            "BeforeContextParameter",
            "AfterContextParameter",
        ),
        (
            "app/src/main/kotlin/ContextReceiver.kt",
            "BeforeContextReceiver",
            "AfterContextReceiver",
        ),
        (
            "app/src/main/kotlin/WhenTrailingComma.kt",
            "BeforeWhenTrailingComma",
            "AfterWhenTrailingComma",
        ),
        (
            "app/src/main/kotlin/AnnotatedTypeArgument.kt",
            "BeforeAnnotatedTypeArgument",
            "AfterAnnotatedTypeArgument",
        ),
    ] {
        assert!(
            facts
                .local_components
                .iter()
                .any(|component| component.symbol == before_name),
            "missing local component {before_name} in {file}"
        );
        assert!(
            facts
                .local_components
                .iter()
                .any(|component| component.symbol == after_name),
            "missing local component {after_name} in {file}"
        );

        let source = fixture_source(&fixture_root, file);
        let (call_line, call_column) =
            find_line_and_column_after(&source, &format!("fun {after_name}"), COMMON_AFTER_CALL);
        assert_fact(
            &facts,
            FactKind::Usage,
            file,
            "PrimaryButton",
            call_line,
            call_column,
        );

        let (token_line, token_column) =
            find_line_and_column_after(&source, &format!("fun {after_name}"), "Spacing.small");
        assert_fact(
            &facts,
            FactKind::Token,
            file,
            "Spacing.small",
            token_line,
            token_column,
        );

        let (dp_line, dp_column) =
            find_line_and_column_after(&source, &format!("fun {after_name}"), "padding");
        assert_fact(&facts, FactKind::Style, file, "7.dp", dp_line, dp_column);
    }

    let when_guard_source = fixture_source(&fixture_root, "app/src/main/kotlin/WhenGuard.kt");
    let (when_guard_line, when_guard_column) = find_line_and_column_after(
        &when_guard_source,
        "when (item)",
        "PrimaryButton(onClick = {})",
    );
    assert_fact(
        &facts,
        FactKind::Usage,
        "app/src/main/kotlin/WhenGuard.kt",
        "PrimaryButton",
        when_guard_line,
        when_guard_column,
    );

    let context_parameter_source =
        fixture_source(&fixture_root, "app/src/main/kotlin/ContextParameter.kt");
    let (context_parameter_line, context_parameter_column) = find_line_and_column_after(
        &context_parameter_source,
        "fun ContextScreen",
        "PrimaryButton(onClick = {})",
    );
    assert_fact(
        &facts,
        FactKind::Usage,
        "app/src/main/kotlin/ContextParameter.kt",
        "PrimaryButton",
        context_parameter_line,
        context_parameter_column,
    );

    let context_receiver_source =
        fixture_source(&fixture_root, "app/src/main/kotlin/ContextReceiver.kt");
    let (context_receiver_line, context_receiver_column) = find_line_and_column_after(
        &context_receiver_source,
        "fun LegacyContextScreen",
        "PrimaryButton(onClick = {})",
    );
    assert_fact(
        &facts,
        FactKind::Usage,
        "app/src/main/kotlin/ContextReceiver.kt",
        "PrimaryButton",
        context_receiver_line,
        context_receiver_column,
    );

    let explicit_backing_field_source =
        fixture_source(&fixture_root, "app/src/main/kotlin/ExplicitBackingField.kt");
    let (initializer_token_line, initializer_token_column) = find_line_and_column_after(
        &explicit_backing_field_source,
        "val spacing",
        "Spacing.small",
    );
    assert_fact(
        &facts,
        FactKind::Token,
        "app/src/main/kotlin/ExplicitBackingField.kt",
        "Spacing.small",
        initializer_token_line,
        initializer_token_column,
    );

    let (initializer_style_line, initializer_style_column) =
        find_line_and_column_after(&explicit_backing_field_source, "val modifier", "padding");
    assert_fact(
        &facts,
        FactKind::Style,
        "app/src/main/kotlin/ExplicitBackingField.kt",
        "7.dp",
        initializer_style_line,
        initializer_style_column,
    );
    assert!(
        facts
            .usage_sites
            .iter()
            .all(|site| site.symbol != "MutableStateFlow")
            && facts
                .local_components
                .iter()
                .all(|component| component.symbol != "MutableStateFlow"),
        "explicit backing-field infrastructure constructors must not become component facts"
    );

    let trailing_comma_source =
        fixture_source(&fixture_root, "app/src/main/kotlin/WhenTrailingComma.kt");
    let (trailing_comma_line, trailing_comma_column) = find_line_and_column_after(
        &trailing_comma_source,
        "when (status)",
        "PrimaryButton(onClick = {})",
    );
    assert_fact(
        &facts,
        FactKind::Usage,
        "app/src/main/kotlin/WhenTrailingComma.kt",
        "PrimaryButton",
        trailing_comma_line,
        trailing_comma_column,
    );
}

fn scan_fixture(fixture_root: &Path, snapshot_id: &str) -> wax_contract::ScanFacts {
    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: snapshot_id.to_owned(),
        config,
    };

    ComposeLanguage::new()
        .scan(&request)
        .expect("compose scan should succeed")
}

fn fixture_source(fixture_root: &Path, relative_file: &str) -> String {
    std::fs::read_to_string(fixture_root.join(relative_file))
        .unwrap_or_else(|err| panic!("failed to read fixture {relative_file}: {err}"))
}

#[derive(Clone, Copy, Debug)]
enum FactKind {
    Usage,
    Token,
    Style,
}

fn assert_fact(
    facts: &wax_contract::ScanFacts,
    kind: FactKind,
    file: &str,
    needle: &str,
    line: u32,
    column: u32,
) {
    let found = match kind {
        FactKind::Usage => facts.usage_sites.iter().any(|site| {
            site.location.file == file
                && site.symbol == needle
                && site.location.line == line
                && site.location.column == Some(column)
        }),
        FactKind::Token => facts.token_sites.iter().any(|site| {
            site.location.file == file
                && site.key == needle
                && site.location.line == line
                && site.location.column == Some(column)
        }),
        FactKind::Style => facts.hardcoded_style_sites.iter().any(|site| {
            site.location.file == file
                && site.value == needle
                && site.location.line == line
                && site.location.column == Some(column)
        }),
    };
    assert!(found, "missing {kind:?} {needle} at {file}:{line}:{column}");
}

fn find_line_and_column_after(source: &str, anchor: &str, needle: &str) -> (u32, u32) {
    let anchor_start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("missing anchor {anchor:?}"));
    let relative_offset = source[anchor_start..]
        .find(needle)
        .unwrap_or_else(|| panic!("missing needle {needle:?} after anchor {anchor:?}"));
    line_and_column(source, anchor_start + relative_offset)
}

fn line_and_column(source: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 1_u32;
    let mut column = 1_u32;

    for &byte in source.as_bytes().iter().take(byte_offset) {
        if byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}
