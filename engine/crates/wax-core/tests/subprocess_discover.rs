#![cfg(unix)]

use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use wax_core::subprocess_discover::SubprocessLanguageDiscoverer;
use wax_core::subprocess_lang::SubprocessLanguageManifest;
use wax_lang_api::{DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION};

#[test]
fn subprocess_discover_parses_discover_symbols_response() {
    let temp_dir = TestDir::new("discover-success");
    let request_path = temp_dir.path().join("request.json");
    let script_path = temp_dir.path().join("mock-discover-pack.sh");
    write_discover_script(&script_path);

    let extractor = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![
            script_path.to_string_lossy().into_owned(),
            request_path.to_string_lossy().into_owned(),
        ],
        timeout: Duration::from_secs(5),
    });

    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let result = extractor.discover(request).unwrap();

    assert_eq!(result.symbols, vec!["PrimaryButton".to_owned()]);
    assert!(result.diagnostics.is_empty());

    let request_json: Value =
        serde_json::from_str(&fs::read_to_string(&request_path).unwrap()).unwrap();
    assert_eq!(
        request_json,
        json!({
            "type": "discover",
            "api_version": WIRE_API_VERSION,
            "language_id": "compose",
            "repo_root": "/tmp/repo",
            "roots": ["design-system/src/main/kotlin"],
        })
    );
}

fn write_discover_script(script_path: &Path) {
    write_script(
        script_path,
        &format!(
            r#"#!/bin/sh
cat > "$1"
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&json!({
                "type": "discover_symbols",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "symbols": ["PrimaryButton"],
                "diagnostics": [],
            }))
            .unwrap()
        ),
    );
}

fn write_script(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("wax-core-subprocess-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
