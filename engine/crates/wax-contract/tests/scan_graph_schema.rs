use serde_json::json;

fn validator() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/scan-graph.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

#[test]
fn graph_fixture_validates() {
    let validator = validator();
    let value = json!({
        "schema_version": 1,
        "metadata": {
            "source_scan_path": ".wax/out/scan-merged.json",
            "generated_at": "1970-01-01T00:00:00Z",
            "limits": [
                "module rollups are not available in current scan facts"
            ]
        },
        "nodes": [
            {
                "id": "lang:compose",
                "kind": "language",
                "label": "compose",
                "language": "compose"
            },
            {
                "id": "ds:compose:button",
                "kind": "design_system",
                "label": "button",
                "language": "compose"
            },
            {
                "id": "file:src/a.kt",
                "kind": "file",
                "label": "src/a.kt",
                "language": null
            }
        ],
        "edges": [
            {
                "from": "file:src/a.kt",
                "to": "ds:compose:button",
                "kind": "usage",
                "match_status": "candidate"
            }
        ],
        "metrics": [
            {
                "name": "invocation_adoption_ratio",
                "scope": "repo",
                "value": 0.875
            },
            {
                "name": "invocation_adoption_ratio",
                "scope": "language:compose",
                "value": 1.0
            }
        ]
    });

    assert!(
        validator.is_valid(&value),
        "fixture should validate against scan-graph schema"
    );
}

#[test]
fn rejects_missing_schema_version() {
    let validator = validator();
    let value = json!({
        "metadata": {
            "source_scan_path": ".wax/out/scan-merged.json",
            "generated_at": "1970-01-01T00:00:00Z",
            "limits": ["gap"]
        },
        "nodes": [],
        "edges": [],
        "metrics": []
    });

    assert!(
        !validator.is_valid(&value),
        "schema_version must be required"
    );
}
