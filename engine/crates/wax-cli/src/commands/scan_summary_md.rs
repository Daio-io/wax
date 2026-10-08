//! Markdown rendering for scan summaries.

use super::scan_baseline::SummaryDeltas;
use super::scan_summary::{JsonSummary, WrittenArtifact};
use std::fmt::Write;

/// Renders a scan summary as Markdown suitable for CI output and PR comments.
#[must_use]
pub fn render_markdown_summary(
    summary: &JsonSummary,
    deltas: Option<&SummaryDeltas>,
    artifacts: &[WrittenArtifact],
) -> String {
    let mut markdown = String::from("# Wax scan\n\n");
    match summary.adoption.coverage_ratio {
        Some(ratio) => {
            let _ = writeln!(markdown, "Adoption: **{:.1}%**", ratio * 100.0);
        }
        None => markdown.push_str("Adoption: **unavailable**\n"),
    }

    markdown.push_str(
        "\n| Language | Status | Coverage | Resolved | Candidate |\n| --- | --- | ---: | ---: | ---: |\n",
    );
    for language in &summary.languages {
        let coverage = language
            .coverage_ratio
            .map(|ratio| format!("{:.1}%", ratio * 100.0))
            .unwrap_or_else(|| "—".to_owned());
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} |",
            language.id, language.status, coverage, language.resolved, language.candidate
        );
    }

    if let Some(deltas) = deltas {
        markdown.push_str("\n## Changes\n\n");
        match deltas.adoption_coverage_delta {
            Some(delta) => {
                let _ = writeln!(
                    markdown,
                    "- Adoption coverage: {:+.1} percentage points",
                    delta * 100.0
                );
            }
            None => markdown.push_str("- Adoption coverage: unavailable\n"),
        }
        let _ = writeln!(
            markdown,
            "- Resolved invocations: {:+}",
            deltas.resolved_delta
        );
        let _ = writeln!(
            markdown,
            "- Candidate invocations: {:+}",
            deltas.candidate_delta
        );
        for diagnostic in &deltas.new_error_diagnostics {
            let language = diagnostic.language.as_deref().unwrap_or("unknown");
            let _ = writeln!(
                markdown,
                "- New error `{}` ({}): {}",
                diagnostic.code, language, diagnostic.message
            );
        }
        for diagnostic in &deltas.resolved_diagnostics {
            let language = diagnostic.language.as_deref().unwrap_or("unknown");
            let _ = writeln!(
                markdown,
                "- Resolved error `{}` ({}): {}",
                diagnostic.code, language, diagnostic.message
            );
        }
    }

    markdown.push_str("\n## Diagnostics\n\n");
    if summary.diagnostics.is_empty() {
        markdown.push_str("No error diagnostics.\n");
    } else {
        for diagnostic in summary.diagnostics.iter().take(10) {
            let _ = writeln!(
                markdown,
                "- `{}` ({}): {}",
                diagnostic.code, diagnostic.language, diagnostic.message
            );
        }
    }

    markdown.push_str("\n## Artifacts\n\n");
    if artifacts.is_empty() {
        markdown.push_str("- [scan-merged](.wax/out/scan-merged.json)\n");
    } else {
        for artifact in artifacts {
            let _ = writeln!(markdown, "- [{}]({})", artifact.format, artifact.path);
        }
    }

    markdown.push_str("\n## Limits\n\n");
    for limit in &summary.limits {
        let _ = writeln!(markdown, "- {limit}");
    }

    markdown
}
