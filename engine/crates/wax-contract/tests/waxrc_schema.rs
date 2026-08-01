use serde_json::json;

fn validator() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/waxrc.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn config(registry: serde_json::Value) -> serde_json::Value {
    json!({
        "schema_version": 2,
        "languages": { "react": { "registry": registry } }
    })
}

#[test]
fn accepts_all_three_registry_shapes() {
    let validator = validator();
    for registry in [
        json!(".wax/react.registry.json"),
        json!({ "source": "https://cdn.example.com/react.json", "upstream": "acme/react" }),
        json!({ "git": "git@github.com:acme/design-system.git", "tag": "v2.4.1" }),
    ] {
        assert!(validator.is_valid(&config(registry)));
    }
}

#[test]
fn rejects_invalid_git_registry_shapes() {
    let validator = validator();
    for registry in [
        json!({ "git": "https://github.com/acme/design-system.git" }),
        json!({ "tag": "v2.4.1" }),
        json!({ "git": "", "tag": "v2.4.1" }),
        json!({ "git": "   ", "tag": "v2.4.1" }),
        json!({ "git": "https://example.com/repo.git", "tag": "" }),
        json!({ "git": "https://example.com/repo.git", "tag": "   " }),
        json!({ "git": null, "tag": "v2.4.1" }),
        json!({ "git": "https://example.com/repo.git", "tag": null }),
        json!({ "git": "https://example.com/repo.git", "tag": "v2.4.1", "source": "x" }),
        json!({ "git": "https://example.com/repo.git", "tag": "v2.4.1", "upstream": "acme/react" }),
        json!({ "git": "https://example.com/repo.git", "tag": "v2.4.1", "path": "x" }),
    ] {
        assert!(!validator.is_valid(&config(registry)));
    }
}
