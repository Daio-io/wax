use serde_json::json;

fn validator() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/scan-summary.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

#[test]
fn summary_fixture_validates() {
    let validator = validator();
    let value = json!({
        "schema_version": 1,
        "generated_at": "1970-01-01T00:00:00Z",
        "repo_root": "/tmp/repo",
        "scan_path": "/tmp/repo/.wax/out/scan-merged.json",
        "snapshot_ids": ["snap-compose"],
        "languages": [
            {
                "id": "compose",
                "version": "0.1.0",
                "status": "complete",
                "parser": "fixture",
                "files_scanned": 1,
                "coverage_ratio": 0.875,
                "resolved": 7,
                "candidate": 1
            }
        ],
        "adoption": {
            "coverage_ratio": 0.875,
            "eligible_invocation_count": 8,
            "adopted_invocation_count": 7,
            "non_adopted_invocation_count": 1,
            "adoption_excluded_invocation_count": 0,
            "raw_invocations": {
                "total": 9,
                "resolved": 7,
                "local": 1,
                "candidate": 1,
                "unresolved": 1
            }
        },
        "diagnostics": [
            {
                "severity": "error",
                "code": "PACK_TIMEOUT",
                "message": "timed out",
                "language": "react",
                "location": {
                    "file": "src/Broken.tsx",
                    "line": 4,
                    "column": 12
                }
            }
        ],
        "artifacts": [
            {
                "format": "json-summary",
                "path": ".wax/out/scan-summary.json",
                "bytes": 128
            }
        ],
        "limits": [
            "module rollups are not available in current scan facts",
            "category rollups are not available in current scan facts",
            "ownership rollups are not available in current scan facts"
        ]
    });

    assert!(
        validator.is_valid(&value),
        "fixture should validate against scan-summary schema"
    );
}
