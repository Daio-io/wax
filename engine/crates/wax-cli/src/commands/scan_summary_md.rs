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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_baseline::DiagnosticRef;
    use crate::commands::scan_summary::{
        JsonSummary, JsonSummaryAdoption, JsonSummaryDiagnostic, JsonSummaryLanguage,
        JsonSummaryRawInvocations, WrittenArtifact,
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
        JsonSummary {
            schema_version: 1,
            generated_at: "1970-01-01T00:00:00Z".to_owned(),
            repo_root: ".".to_owned(),
            scan_path: ".wax/out/scan-merged.json".to_owned(),
            snapshot_ids: vec![],
            languages: vec![JsonSummaryLanguage {
                id: "compose".to_owned(),
                version: "1.0.0".to_owned(),
                status: "complete".to_owned(),
                parser: "fixture".to_owned(),
                files_scanned: 1,
                coverage_ratio: Some(1.0),
                resolved: 1,
                candidate: 0,
            }],
            adoption: JsonSummaryAdoption {
                coverage_ratio: Some(1.0),
                eligible_invocation_count: 1,
                adopted_invocation_count: 1,
                non_adopted_invocation_count: 0,
                adoption_excluded_invocation_count: 0,
                raw_invocations: JsonSummaryRawInvocations {
                    total: 1,
                    resolved: 1,
                    local: 0,
                    candidate: 0,
                    unresolved: 0,
                },
            },
            diagnostics,
            artifacts: vec![],
            limits: vec!["module rollups are not available".to_owned()],
            deltas: None,
        }
    }

    #[test]
    fn scan_summary_md_renders_required_sections_without_changes() {
        let summary = sample_summary(1);
        let artifacts = [WrittenArtifact {
            format: "json-summary".to_owned(),
            path: ".wax/out/scan-summary.json".to_owned(),
            bytes: Some(12),
        }];

        let markdown = render_markdown_summary(&summary, None, &artifacts);

        assert!(markdown.starts_with("# Wax scan\n\nAdoption: **100.0%**"));
        assert!(markdown.contains("| Language | Status | Coverage | Resolved | Candidate |"));
        assert!(markdown.contains("| compose | complete | 100.0% | 1 | 0 |"));
        assert!(!markdown.contains("## Changes"));
        assert!(markdown.contains("## Diagnostics\n\n- `error_00` (compose): failure 00"));
        assert!(markdown.contains("## Artifacts\n\n- [json-summary](.wax/out/scan-summary.json)"));
        assert!(markdown.contains("## Limits\n\n- module rollups are not available"));
    }

    #[test]
    fn scan_summary_md_includes_changes_when_deltas_present() {
        let summary = sample_summary(0);
        let deltas = SummaryDeltas {
            adoption_coverage_delta: Some(0.25),
            resolved_delta: 2,
            candidate_delta: -1,
            new_error_diagnostics: vec![DiagnosticRef {
                code: "new_error".to_owned(),
                message: "appeared".to_owned(),
                language: Some("compose".to_owned()),
            }],
            resolved_diagnostics: vec![DiagnosticRef {
                code: "old_error".to_owned(),
                message: "gone".to_owned(),
                language: Some("compose".to_owned()),
            }],
        };

        let markdown = render_markdown_summary(&summary, Some(&deltas), &[]);

        assert!(markdown.contains("## Changes"));
        assert!(markdown.contains("- Adoption coverage: +25.0 percentage points"));
        assert!(markdown.contains("- Resolved invocations: +2"));
        assert!(markdown.contains("- Candidate invocations: -1"));
        assert!(markdown.contains("- New error `new_error` (compose): appeared"));
        assert!(markdown.contains("- Resolved error `old_error` (compose): gone"));
        assert!(markdown.contains("- [scan-merged](.wax/out/scan-merged.json)"));
    }

    #[test]
    fn scan_summary_md_caps_diagnostics_at_ten() {
        let summary = sample_summary(11);

        let markdown = render_markdown_summary(&summary, None, &[]);
        let diagnostic_rows = markdown
            .lines()
            .filter(|line| line.starts_with("- `error_"))
            .count();

        assert_eq!(diagnostic_rows, 10);
        assert!(!markdown.contains("error_10"));
    }
}
