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

    assert_eq!(output["schema_version"], 1, "schema_version must stay v1");

    let language_id =
        std::env::var("WAX_ALPHA_SMOKE_LANGUAGE_ID").unwrap_or_else(|_| "compose".to_owned());
    let language = output["languages"][&language_id]
        .as_object()
        .unwrap_or_else(|| panic!("{language_id} language facts should be present"));

    if std::env::var("WAX_ALPHA_SMOKE_REQUIRE_NON_SCAFFOLD_COUNTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
    {
        let counts = language["counts"]
            .as_object()
            .unwrap_or_else(|| panic!("{language_id} counts should be present"));

        let total = [
            "design_system_component_count",
            "local_component_count",
            "usage_site_count",
            "resolved_count",
            "candidate_count",
        ]
        .iter()
        .map(|key| counts[*key].as_i64().unwrap_or(0))
        .sum::<i64>();

        assert!(
            total > 0,
            "expected non-scaffold {language_id} counts in smoke output"
        );
    }
}
