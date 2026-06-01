use std::fs;
use std::path::PathBuf;

#[test]
#[ignore = "run with WAX_ALPHA_SMOKE_SCAN_OUTPUT=/path/to/scan-merged.json"]
fn alpha_smoke_scan_output_contract() {
    let path = PathBuf::from(
        std::env::var("WAX_ALPHA_SMOKE_SCAN_OUTPUT")
            .expect("WAX_ALPHA_SMOKE_SCAN_OUTPUT env var must point to scan-merged.json"),
    );
    let output: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read scan output"))
            .expect("parse scan output JSON");

    assert_eq!(output["schema_version"], 1, "schema_version must stay v1");

    let compose = output["languages"]["compose"]
        .as_object()
        .expect("compose language facts should be present");
    let counts = compose["counts"]
        .as_object()
        .expect("compose counts should be present");

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
        "expected non-scaffold compose counts in smoke output"
    );
}
