use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_manifest = manifest_dir.join("../../Cargo.toml");

    println!("cargo:rerun-if-changed={}", workspace_manifest.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("Cargo.toml").display()
    );

    let content = fs::read_to_string(&workspace_manifest)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", workspace_manifest.display()));
    let table: toml::Table = toml::from_str(&content)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", workspace_manifest.display()));

    let version = table
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(|dependencies| dependency_version(dependencies, "swc_ecma_parser"))
        .unwrap_or_else(|| {
            panic!(
                "workspace.dependencies.swc_ecma_parser must be set in {}",
                workspace_manifest.display()
            )
        });

    println!("cargo:rustc-env=SWC_PARSER_VERSION={version}");
}

fn dependency_version(dependencies: &toml::Value, name: &str) -> Option<String> {
    match dependencies.get(name)? {
        toml::Value::String(version) => Some(version.clone()),
        toml::Value::Table(table) => table
            .get("version")
            .and_then(toml::Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}
