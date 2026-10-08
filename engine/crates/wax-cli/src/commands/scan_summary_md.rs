//! Markdown rendering for scan summaries.

use super::scan_summary::JsonSummary;
use std::fmt::Write;

/// Maximum failure diagnostics listed in the Markdown Diagnostics section.
pub const MARKDOWN_DIAGNOSTIC_LIMIT: usize = 10;

/// Renders a scan summary as Markdown suitable for CI output and PR comments.
#[must_use]
pub fn render_markdown_summary(summary: &JsonSummary) -> String {
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

    if let Some(deltas) = summary.deltas.as_ref() {
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
        for diagnostic in summary.diagnostics.iter().take(MARKDOWN_DIAGNOSTIC_LIMIT) {
            let _ = writeln!(
                markdown,
                "- `{}` ({}): {}",
                diagnostic.code, diagnostic.language, diagnostic.message
            );
        }
    }

    markdown.push_str("\n## Artifacts\n\n");
    let _ = writeln!(markdown, "- [scan-merged]({})", summary.scan_path);
    for artifact in &summary.artifacts {
        let _ = writeln!(markdown, "- [{}]({})", artifact.format, artifact.path);
    }

    markdown.push_str("\n## Limits\n\n");
    for limit in &summary.limits {
        let _ = writeln!(markdown, "- {limit}");
    }

    markdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_baseline::{DiagnosticRef, SummaryDeltas};
    use crate::commands::scan_summary::{
        JsonSummary, JsonSummaryDiagnostic, WrittenArtifact, sample_json_summary,
    };

    fn sample_summary(diagnostic_count: usize) -> JsonSummary {
        let diagnostics = (0..diagnostic_count)
            .map(|index| JsonSummaryDiagnostic {
                severity: "error".to_owned(),
                code: format!("error_{index:02}"),
                message: format!("failure {index:02}"),
                language: "compose".to_owned(),
                location: None,
            })
            .collect();
        sample_json_summary(
            Some(1.0),
            1,
            0,
            diagnostics,
            vec!["module rollups are not available".to_owned()],
        )
    }

    #[test]
    fn scan_summary_md_renders_required_sections_without_changes() {
        let mut summary = sample_summary(1);
        summary.artifacts = vec![WrittenArtifact {
            format: "json-summary".to_owned(),
            path: ".wax/out/scan-summary.json".to_owned(),
            bytes: Some(12),
        }];

        let markdown = render_markdown_summary(&summary);

        assert!(markdown.starts_with("# Wax scan\n\nAdoption: **100.0%**"));
        assert!(markdown.contains("| Language | Status | Coverage | Resolved | Candidate |"));
        assert!(markdown.contains("| compose | complete | 100.0% | 1 | 0 |"));
        assert!(!markdown.contains("## Changes"));
        assert!(markdown.contains("## Diagnostics\n\n- `error_00` (compose): failure 00"));
        assert!(markdown.contains(
            "## Artifacts\n\n- [scan-merged](.wax/out/scan-merged.json)\n- [json-summary](.wax/out/scan-summary.json)"
        ));
        assert!(markdown.contains("## Limits\n\n- module rollups are not available"));
    }

    #[test]
    fn scan_summary_md_includes_changes_when_deltas_present() {
        let mut summary = sample_summary(0);
        summary.deltas = Some(SummaryDeltas {
            adoption_coverage_delta: Some(0.25),
            resolved_delta: 2,
            candidate_delta: -1,
            new_error_diagnostics: vec![DiagnosticRef {
                code: "new_error".to_owned(),
                message: "appeared".to_owned(),
                language: Some("compose".to_owned()),
                location: None,
            }],
            resolved_diagnostics: vec![DiagnosticRef {
                code: "old_error".to_owned(),
                message: "gone".to_owned(),
                language: Some("compose".to_owned()),
                location: None,
            }],
        });

        let markdown = render_markdown_summary(&summary);

        assert!(markdown.contains("## Changes"));
        assert!(markdown.contains("- Adoption coverage: +25.0 percentage points"));
        assert!(markdown.contains("- Resolved invocations: +2"));
        assert!(markdown.contains("- Candidate invocations: -1"));
        assert!(markdown.contains("- New error `new_error` (compose): appeared"));
        assert!(markdown.contains("- Resolved error `old_error` (compose): gone"));
        assert!(markdown.contains("- [scan-merged](.wax/out/scan-merged.json)"));
    }

    #[test]
    fn scan_summary_md_links_scan_path_instead_of_inventing_artifact() {
        let mut summary = sample_summary(0);
        summary.scan_path = "reports/prior-merged.json".to_owned();
        summary.artifacts = vec![];

        let markdown = render_markdown_summary(&summary);

        assert!(markdown.contains("## Artifacts\n\n- [scan-merged](reports/prior-merged.json)\n"));
        assert!(!markdown.contains(".wax/out/scan-merged.json"));
    }

    #[test]
    fn scan_summary_md_caps_diagnostics_at_ten() {
        let summary = sample_summary(MARKDOWN_DIAGNOSTIC_LIMIT + 1);

        let markdown = render_markdown_summary(&summary);
        let diagnostic_rows = markdown
            .lines()
            .filter(|line| line.starts_with("- `error_"))
            .count();

        assert_eq!(diagnostic_rows, MARKDOWN_DIAGNOSTIC_LIMIT);
        assert!(!markdown.contains(&format!("error_{:02}", MARKDOWN_DIAGNOSTIC_LIMIT)));
    }
}
