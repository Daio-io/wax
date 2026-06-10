#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use wax_core::subprocess_discover::SubprocessLanguageDiscoverer;
use wax_core::subprocess_lang::SubprocessLanguageManifest;
use wax_lang_api::{DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION};

#[test]
fn subprocess_discover_parses_discover_symbols_response() {
    let fixture = fixture_discover_script();
    let extractor = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![fixture.path().display().to_string()],
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
}

fn fixture_discover_script() -> DiscoverScriptFixture {
    let temp_dir = TestDir::new("discover-success");
    let script_path = temp_dir.path().join("mock-discover-pack.sh");
    write_script(
        &script_path,
        &format!(
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{}
JSON
"#,
            serde_json::to_string(&serde_json::json!({
                "type": "discover_symbols",
                "api_version": WIRE_API_VERSION,
                "language_id": "compose",
                "symbols": ["PrimaryButton"],
                "diagnostics": [],
            }))
            .unwrap()
        ),
    );
    DiscoverScriptFixture {
        _dir: temp_dir,
        script_path,
    }
}

struct DiscoverScriptFixture {
    _dir: TestDir,
    script_path: PathBuf,
}

impl DiscoverScriptFixture {
    fn path(&self) -> &Path {
        &self.script_path
    }
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
