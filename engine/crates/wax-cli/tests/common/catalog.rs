use std::fs;
fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Wax Test",
            "-c",
            "user.email=test@example.com",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub fn seed_catalogue(root: &std::path::Path) {
    let repo = root.join("remote");
    fs::create_dir_all(repo.join("catalog")).unwrap();
    git(&repo, &["init", "-qb", "main"]);
    let bundle = repo.join("systems/acme/1");
    fs::create_dir_all(bundle.join(".wax/registries")).unwrap();
    fs::write(bundle.join(".wax/wax.config.json"), r#"{"schema_version":2,"design_systems":{"acme":{"name":"Acme","registries":{"react":{"source":".wax/registries/react.json"},"compose":{"source":".wax/registries/compose.json"}}}}}"#).unwrap();
    fs::write(bundle.join("provenance.json"), "{\"upstream\":[]}").unwrap();
    for language in ["compose", "react"] {
        fs::write(bundle.join(format!(".wax/registries/{language}.json")), format!(r##"{{"schema_version":1,"language":"{language}","components":[{{"id":"button","symbol":"Button"}}],"tokens":[{{"id":"primary","key":"color.primary","value":"#fff","category":"color"}}]}}"##)).unwrap();
    }
    let artifact = |path: &str| serde_json::json!({"path":path,"sha256":wax_core::digest::sha256_hex(&fs::read(bundle.join(path)).unwrap())});
    let index = serde_json::json!({"schema_version":1,"entries":[{"id":"acme","name":"Acme","revision":1,"subdir":"systems/acme/1","minimum_wax_version":"0.0.0","definition":artifact(".wax/wax.config.json"),"provenance":artifact("provenance.json"),"registries":{"compose":artifact(".wax/registries/compose.json"),"react":artifact(".wax/registries/react.json")},"upstream":[],"coverage":["buttons"],"gaps":["icons"]}]});
    fs::write(repo.join("catalog/index.json"), index.to_string()).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "catalogue"]);
}

pub fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(args)
        .current_dir(root)
        .env("WAX_HOME", root.join("home"))
        .env("GIT_CONFIG_COUNT", "1")
        .env(
            "GIT_CONFIG_KEY_0",
            format!("url.{}.insteadOf", root.join("remote").display()),
        )
        .env(
            "GIT_CONFIG_VALUE_0",
            wax_core::registry_catalog::PUBLIC_REGISTRY_REPOSITORY,
        )
        .output()
        .unwrap()
}
pub fn change_registry(root: &std::path::Path) {
    let repo = root.join("remote");
    let path = repo.join("systems/acme/1/.wax/registries/react.json");
    fs::write(&path, r#"{"schema_version":1,"language":"react","components":[{"id":"button","symbol":"ChangedButton"}]}"#).unwrap();
    let index = repo.join("catalog/index.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    json["entries"][0]["registries"]["react"]["sha256"] =
        wax_core::digest::sha256_hex(&fs::read(path).unwrap()).into();
    fs::write(index, json.to_string()).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "change"]);
}
