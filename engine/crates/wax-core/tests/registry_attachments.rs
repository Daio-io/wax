use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use wax_contract::{RegistryDocument, registry_from_json};
use wax_core::registry_attachments::{RegistryAttachmentError, materialize_registry_attachments};

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp repo");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn registry(json: &str) -> RegistryDocument {
    registry_from_json(json).expect("valid registry fixture")
}

#[test]
fn materializes_requested_inclusive_line_excerpt() {
    let repo = TestRepo::new("registry-attachment-lines");
    fs::create_dir_all(repo.path().join("docs")).expect("create docs directory");
    fs::write(
        repo.path().join("docs/button.md"),
        "heading\nfirst detail\nsecond detail\nfooter\n",
    )
    .expect("write documentation");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [{
                "kind": "documentation",
                "path": "docs/button.md",
                "start_line": 2,
                "end_line": 3,
                "language": "markdown"
              }]
            }
          }]
        }"#,
    );

    let diagnostics = materialize_registry_attachments(repo.path(), &mut document)
        .expect("attachment materialization should succeed");

    assert!(diagnostics.is_empty());
    let attachment = &document.components[0]
        .extracted
        .as_ref()
        .expect("generated extraction")
        .attachments[0];
    assert_eq!(attachment.content, "first detail\nsecond detail\n");
    assert_eq!(attachment.path, "docs/button.md");
    assert_eq!(attachment.language.as_deref(), Some("markdown"));
}

#[test]
fn missing_associated_file_is_reported_without_failing_other_attachments() {
    let repo = TestRepo::new("registry-attachment-missing");
    fs::create_dir_all(repo.path().join("docs")).expect("create docs directory");
    fs::write(repo.path().join("docs/button.md"), "Button docs\n").expect("write documentation");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [
                {"kind": "documentation", "path": "docs/missing.md"},
                {"kind": "documentation", "path": "docs/button.md"}
              ]
            }
          }]
        }"#,
    );

    let diagnostics = materialize_registry_attachments(repo.path(), &mut document)
        .expect("a missing association should be non-fatal");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "registry_attachment_missing");
    assert_eq!(
        document.components[0]
            .extracted
            .as_ref()
            .expect("generated extraction")
            .attachments
            .len(),
        1
    );
}

#[test]
fn external_link_association_is_reported_without_network_access() {
    let repo = TestRepo::new("registry-attachment-external-link");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [{
                "kind": "documentation",
                "path": "https://example.com/button"
              }]
            }
          }]
        }"#,
    );

    let diagnostics = materialize_registry_attachments(repo.path(), &mut document)
        .expect("external links should be skipped without failing publication");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "registry_attachment_unsupported");
    assert!(document.components[0].extracted.is_none());
}

#[test]
fn directory_association_is_reported_as_unsupported() {
    let repo = TestRepo::new("registry-attachment-directory");
    fs::create_dir_all(repo.path().join("examples")).expect("create examples directory");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [{"kind": "example", "path": "examples"}]
            }
          }]
        }"#,
    );

    let diagnostics = materialize_registry_attachments(repo.path(), &mut document)
        .expect("unsupported associations should be non-fatal");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "registry_attachment_unsupported");
}

#[test]
fn parent_traversal_is_rejected() {
    let repo = TestRepo::new("registry-attachment-parent-traversal");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "tokens": [{
            "id": "color.brand",
            "key": "color.brand",
            "category": "color",
            "metadata": {
              "sources": [{"kind": "declaration", "path": "../tokens.json"}]
            }
          }]
        }"#,
    );

    let error = materialize_registry_attachments(repo.path(), &mut document)
        .expect_err("parent traversal must be rejected");

    assert!(matches!(error, RegistryAttachmentError::UnsafePath { .. }));
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let repo = TestRepo::new("registry-attachment-symlink");
    let outside_path = repo.path().with_extension("outside.md");
    fs::write(&outside_path, "outside\n").expect("write outside file");
    symlink(&outside_path, repo.path().join("linked.md")).expect("create escaping symlink");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [{"kind": "example", "path": "linked.md"}]
            }
          }]
        }"#,
    );

    let error = materialize_registry_attachments(repo.path(), &mut document)
        .expect_err("symlink escape must be rejected");

    assert!(matches!(
        error,
        RegistryAttachmentError::PathEscapesRepo { .. }
    ));
    fs::remove_file(outside_path).expect("remove outside file");
}

#[test]
fn zero_line_number_is_rejected_as_an_invalid_association() {
    let repo = TestRepo::new("registry-attachment-invalid-lines");
    fs::write(repo.path().join("Button.kt"), "fun Button() = Unit\n").expect("write declaration");
    let mut document = registry(
        r#"{
          "schema_version": 1,
          "components": [{
            "id": "button",
            "symbol": "Button",
            "metadata": {
              "sources": [{
                "kind": "declaration",
                "path": "Button.kt",
                "start_line": 0
              }]
            }
          }]
        }"#,
    );

    let error = materialize_registry_attachments(repo.path(), &mut document)
        .expect_err("line numbers are one-based");

    assert!(matches!(
        error,
        RegistryAttachmentError::InvalidLineRange { .. }
    ));
}
