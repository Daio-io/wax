use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());

    let content = fs::read_to_string(&manifest)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", manifest.display()));
    let table: toml::Table = toml::from_str(&content)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", manifest.display()));
    let version = table
        .get("dependencies")
        .and_then(|dependencies| dependency_version(dependencies, "tree-sitter-swift"))
        .or_else(|| {
            let workspace_manifest = manifest_dir.join("../../Cargo.toml");
            println!("cargo:rerun-if-changed={}", workspace_manifest.display());
            let content = fs::read_to_string(&workspace_manifest).unwrap_or_else(|err| {
                panic!("failed to read {}: {err}", workspace_manifest.display())
            });
            let table: toml::Table = toml::from_str(&content).unwrap_or_else(|err| {
                panic!("failed to parse {}: {err}", workspace_manifest.display())
            });
            table
                .get("workspace")
                .and_then(|workspace| workspace.get("dependencies"))
                .and_then(|dependencies| dependency_version(dependencies, "tree-sitter-swift"))
        })
        .expect("tree-sitter-swift dependency must specify a version");

    println!("cargo:rustc-env=TREE_SITTER_SWIFT_GRAMMAR_VERSION={version}");
}

fn dependency_version(dependencies: &toml::Value, name: &str) -> Option<String> {
    match dependencies.get(name)? {
        toml::Value::String(version) => Some(version.trim_start_matches('=').to_owned()),
        toml::Value::Table(table) => table
            .get("version")
            .and_then(toml::Value::as_str)
            .map(|version| version.trim_start_matches('=').to_owned()),
        _ => None,
    }
}
