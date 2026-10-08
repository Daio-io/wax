//! Static, offline HTML report generation for scan artifacts.

use super::scan_graph::ScanGraph;
use super::scan_summary::JsonSummary;
use std::path::Path;
use thiserror::Error;
use wax_core::{AtomicWriteOptions, write_atomically};

/// Errors from writing an HTML report artifact.
#[derive(Debug, Error)]
pub enum HtmlWriteError {
    /// Atomic replacement of the HTML report failed.
    #[error(transparent)]
    AtomicWrite(#[from] wax_core::AtomicWriteError),
}

/// Inline styles for the self-contained scan report.
const REPORT_CSS: &str = r#"
body { font: 16px system-ui, sans-serif; margin: 2rem auto; max-width: 1100px; color: #18202a; }
section { margin: 2rem 0; } .cards { display: flex; flex-wrap: wrap; gap: 1rem; }
.card { border: 1px solid #d5dbe3; border-radius: .5rem; padding: 1rem; min-width: 12rem; }
table { border-collapse: collapse; width: 100%; } th, td { border-bottom: 1px solid #d5dbe3; padding: .5rem; text-align: left; }
.metric { margin: .7rem 0; } .bar { background: #3867d6; height: .8rem; }
"#;

/// Inline behavior for the self-contained scan report.
const REPORT_JS: &str =
    r#"document.querySelectorAll('a[href]').forEach(link => link.rel = 'noopener');"#;

/// Writes a self-contained HTML report from a scan summary and graph.
///
/// # Errors
///
/// Returns [`HtmlWriteError`] when the report cannot be written.
pub fn write_html_report(
    path: &Path,
    repo_root: &Path,
    summary: &JsonSummary,
    graph: &ScanGraph,
) -> Result<(), HtmlWriteError> {
    let html = render_report(path, repo_root, summary, graph);
    write_atomically(path, html.as_bytes(), AtomicWriteOptions::default())?;
    Ok(())
}

fn render_report(
    path: &Path,
    repo_root: &Path,
    summary: &JsonSummary,
    graph: &ScanGraph,
) -> String {
    let adoption = summary.adoption.coverage_ratio.map_or_else(
        || "Unavailable".to_owned(),
        |ratio| format!("{:.1}%", ratio * 100.0),
    );
    let language_cards = summary
        .languages
        .iter()
        .map(|language| {
            format!(
                "<article class=\"card\"><h3>{}</h3><p>Status: {}</p><p>Coverage: {}</p><p>Files: {}</p><p>Resolved: {}</p><p>Candidate: {}</p></article>",
                escape(&language.id),
                escape(&language.status),
                language.coverage_ratio.map_or_else(|| "Unavailable".to_owned(), |ratio| format!("{:.1}%", ratio * 100.0)),
                language.files_scanned,
                language.resolved,
                language.candidate
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let metrics = if graph.metrics.is_empty() {
        "<p>No graph metrics</p>".to_owned()
    } else {
        graph
            .metrics
            .iter()
            .map(|metric| {
                format!(
                    "<div class=\"metric\"><strong>{}</strong> ({})<div class=\"bar\" style=\"width:{:.1}%\"></div></div>",
                    escape(&metric.name),
                    escape(&metric.scope),
                    (metric.value * 100.0).clamp(0.0, 100.0)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let diagnostics = if summary.diagnostics.is_empty() {
        "<tr><td colspan=\"6\">No diagnostics</td></tr>".to_owned()
    } else {
        summary
            .diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&diagnostic.severity),
                    escape(&diagnostic.code),
                    escape(&diagnostic.message),
                    escape(&diagnostic.language),
                    diagnostic
                        .location
                        .as_ref()
                        .map_or_else(String::new, |location| escape(&location.file)),
                    diagnostic
                        .location
                        .as_ref()
                        .map_or(String::new(), |location| location.line.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut artifact_links = vec![format!(
        "<li><a href=\"{}\">merged scan (raw JSON)</a></li>",
        escape(&relative_artifact_path(path, repo_root, &summary.scan_path))
    )];
    artifact_links.extend(summary.artifacts.iter().map(|artifact| {
        format!(
            "<li><a href=\"{}\">{} ({})</a></li>",
            escape(&relative_artifact_path(path, repo_root, &artifact.path)),
            escape(&artifact.format),
            artifact.bytes.map_or_else(
                || "size unknown".to_owned(),
                |bytes| format!("{bytes} bytes")
            )
        )
    }));
    let artifacts = artifact_links.join("\n");
    let limits = summary
        .limits
        .iter()
        .map(|limit| format!("<li>{}</li>", escape(limit)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Wax scan report</title><style>{REPORT_CSS}</style></head><body><main><h1>Wax scan report</h1><section><h2>Adoption</h2><p id=\"adoption-headline\">Repository adoption: <strong>{adoption}</strong></p></section><section><h2>Languages</h2><div class=\"cards\">{language_cards}</div></section><section><h2>Graph</h2>{metrics}</section><section><h2>Diagnostics</h2><table><thead><tr><th>Severity</th><th>Code</th><th>Message</th><th>Language</th><th>File</th><th>Line</th></tr></thead><tbody>{diagnostics}</tbody></table></section><section><h2>Artifacts</h2><ul>{artifacts}</ul></section><section><h2>Limits</h2><ul>{limits}</ul></section></main><script>{REPORT_JS}</script></body></html>"
    )
}

fn relative_artifact_path(report_path: &Path, repo_root: &Path, artifact_path: &str) -> String {
    let report_parts = portable_parts(report_path.parent().unwrap_or_else(|| Path::new(".")));
    let artifact = Path::new(artifact_path);
    let artifact = if artifact.is_absolute() {
        artifact.to_path_buf()
    } else {
        repo_root.join(artifact)
    };
    let artifact_parts = portable_parts(&artifact);
    let common = report_parts
        .iter()
        .zip(&artifact_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = vec![".."; report_parts.len().saturating_sub(common)];
    relative.extend(artifact_parts[common..].iter().map(String::as_str));
    if relative.is_empty() {
        ".".to_owned()
    } else {
        relative.join("/")
    }
}

fn portable_parts(path: &Path) -> Vec<String> {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    components
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
