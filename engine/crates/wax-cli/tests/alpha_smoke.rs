use std::fs;
use std::path::PathBuf;

#[test]
#[ignore = "run with WAX_ALPHA_SMOKE_SCAN_OUTPUT=/path/to/scan-merged.json and optionally WAX_ALPHA_SMOKE_LANGUAGE_ID / WAX_ALPHA_SMOKE_REQUIRE_NON_SCAFFOLD_COUNTS"]
fn alpha_smoke_scan_output_contract() {
    let path = PathBuf::from(
        std::env::var("WAX_ALPHA_SMOKE_SCAN_OUTPUT")
            .expect("WAX_ALPHA_SMOKE_SCAN_OUTPUT env var must point to scan-merged.json"),
    );
    let output: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read scan output"))
            .expect("parse scan output JSON");

    let language_id =
        std::env::var("WAX_ALPHA_SMOKE_LANGUAGE_ID").unwrap_or_else(|_| "compose".to_owned());
    let require_fixture_counts = std::env::var("WAX_ALPHA_SMOKE_REQUIRE_NON_SCAFFOLD_COUNTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(true);

    assert_scan_output_contract(&output, &language_id, require_fixture_counts);
}

fn assert_scan_output_contract(
    output: &serde_json::Value,
    language_id: &str,
    require_fixture_counts: bool,
) {
    assert_eq!(output["schema_version"], 1, "schema_version must stay v1");

    let language = output["languages"][language_id]
        .as_object()
        .unwrap_or_else(|| panic!("{language_id} language facts should be present"));

    if require_fixture_counts {
        assert_eq!(language["status"], "complete", "{language_id} scan status");

        let counts = language["counts"]
            .as_object()
            .unwrap_or_else(|| panic!("{language_id} counts should be present"));

        for key in [
            "local_component_count",
            "usage_site_count",
            "resolved_count",
        ] {
            let value = counts[key].as_i64().unwrap_or(0);
            assert!(
                value > 0,
                "expected positive {language_id} fixture scan count for {key}, got {value}"
            );
        }
    }
}

#[test]
fn alpha_smoke_contract_rejects_registry_only_counts() {
    let output = serde_json::json!({
        "schema_version": 1,
        "languages": {
            "compose": {
                "status": "complete",
                "counts": {
                    "design_system_component_count": 1,
                    "local_component_count": 0,
                    "usage_site_count": 0,
                    "resolved_count": 0,
                    "candidate_count": 0,
                    "framework_shadow_count": 0
                }
            }
        }
    });

    let panic = std::panic::catch_unwind(|| {
        assert_scan_output_contract(&output, "compose", true);
    })
    .expect_err("registry-only counts should fail the smoke contract");

    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic>");
    assert!(
        message.contains("local_component_count"),
        "unexpected panic: {message}"
    );
}

#[test]
fn alpha_smoke_contract_accepts_fixture_scan_counts() {
    let output = serde_json::json!({
        "schema_version": 1,
        "languages": {
            "compose": {
                "status": "complete",
                "counts": {
                    "design_system_component_count": 1,
                    "local_component_count": 1,
                    "usage_site_count": 1,
                    "resolved_count": 1,
                    "candidate_count": 0,
                    "framework_shadow_count": 0
                }
            }
        }
    });

    assert_scan_output_contract(&output, "compose", true);
}
