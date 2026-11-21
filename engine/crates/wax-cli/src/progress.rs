//! Terminal progress spinners for long-running CLI commands.

use crate::presentation::{Presentation, TerminalContext};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::Duration;
use wax_core::progress::{
    ScanProgress, ScanProgressEvent, ValidateProgress, ValidateProgressEvent,
};

/// Spinner shown on stderr for eligible human terminal commands.
pub struct CliProgress {
    bar: Option<ProgressBar>,
}

impl Default for CliProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl CliProgress {
    /// Starts a spinner when all streams and the environment allow human output.
    pub fn new() -> Self {
        let presentation = if TerminalContext::detect().is_human() {
            Presentation::Human
        } else {
            Presentation::Plain
        };
        Self::for_presentation(presentation)
    }

    /// Starts a spinner only for explicitly selected human presentation.
    pub fn for_presentation(presentation: Presentation) -> Self {
        if presentation == Presentation::Plain {
            return Self { bar: None };
        }

        let bar = ProgressBar::new_spinner();
        let style = ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        bar.set_style(style);
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar: Some(bar) }
    }

    /// Whether human progress was enabled.
    pub fn is_enabled(&self) -> bool {
        self.bar.is_some()
    }

    /// Updates the spinner message.
    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.set_message(message.into());
        }
    }

    /// Clears the spinner without leaving a line behind.
    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}

impl Drop for CliProgress {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Builds a scan progress sink when [`CliProgress::is_enabled`]; otherwise a no-op sink.
pub fn optional_scan_progress_sink(progress: &Arc<CliProgress>) -> ScanProgress {
    if progress.is_enabled() {
        let progress = Arc::clone(progress);
        ScanProgress::new(move |event| {
            progress.set_message(scan_progress_message(event));
        })
    } else {
        ScanProgress::default()
    }
}

/// Builds a validate progress sink when [`CliProgress::is_enabled`]; otherwise a no-op sink.
pub fn optional_validate_progress_sink(progress: &Arc<CliProgress>) -> ValidateProgress {
    if progress.is_enabled() {
        let progress = Arc::clone(progress);
        ValidateProgress::new(move |event| {
            progress.set_message(validate_progress_message(event));
        })
    } else {
        ValidateProgress::default()
    }
}

/// Maps a scan progress event to a user-facing spinner message.
pub fn scan_progress_message(event: ScanProgressEvent) -> String {
    match event {
        ScanProgressEvent::Preparing => "Preparing scan…".to_owned(),
        ScanProgressEvent::Installing {
            language_id,
            version,
        } => format!("Installing language pack {language_id}@{version}…"),
        ScanProgressEvent::Scanning { language_id } => format!("Scanning with {language_id}…"),
        ScanProgressEvent::ScanComplete { language_id } => {
            format!("Finished {language_id} scan…")
        }
        ScanProgressEvent::LanguagesScanning { completed, total } => {
            format!("Scanning languages ({completed}/{total})…")
        }
        ScanProgressEvent::WritingOutputs => "Writing scan output…".to_owned(),
    }
}

/// Maps a validate progress event to a user-facing spinner message.
pub fn validate_progress_message(event: ValidateProgressEvent) -> String {
    match event {
        ValidateProgressEvent::LoadingConfig => "Loading wax configuration…".to_owned(),
        ValidateProgressEvent::ValidatingLanguage { language_id } => {
            format!("Validating {language_id} registry…")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{scan_progress_message, validate_progress_message};
    use crate::presentation::TerminalContext;
    use std::str::FromStr;
    use wax_contract::LanguageId;
    use wax_core::progress::{ScanProgressEvent, ValidateProgressEvent};

    #[test]
    fn scan_progress_messages_cover_all_events() {
        let compose = LanguageId::from_str("compose").unwrap();
        assert_eq!(
            scan_progress_message(ScanProgressEvent::Preparing),
            "Preparing scan…"
        );
        assert_eq!(
            scan_progress_message(ScanProgressEvent::Installing {
                language_id: compose.clone(),
                version: "0.1.0".to_owned(),
            }),
            "Installing language pack compose@0.1.0…"
        );
        assert_eq!(
            scan_progress_message(ScanProgressEvent::Scanning {
                language_id: compose.clone(),
            }),
            "Scanning with compose…"
        );
        assert_eq!(
            scan_progress_message(ScanProgressEvent::ScanComplete {
                language_id: compose,
            }),
            "Finished compose scan…"
        );
        assert_eq!(
            scan_progress_message(ScanProgressEvent::LanguagesScanning {
                completed: 2,
                total: 5,
            }),
            "Scanning languages (2/5)…"
        );
        assert_eq!(
            scan_progress_message(ScanProgressEvent::WritingOutputs),
            "Writing scan output…"
        );
    }

    #[test]
    fn validate_progress_messages_cover_all_events() {
        let compose = LanguageId::from_str("compose").unwrap();
        assert_eq!(
            validate_progress_message(ValidateProgressEvent::LoadingConfig),
            "Loading wax configuration…"
        );
        assert_eq!(
            validate_progress_message(ValidateProgressEvent::ValidatingLanguage {
                language_id: compose,
            }),
            "Validating compose registry…"
        );
    }

    #[test]
    fn cli_progress_disabled_when_stderr_is_not_a_tty() {
        // Integration tests run with piped stderr; unit tests inherit the test harness.
        let progress = super::CliProgress::new();
        if TerminalContext::detect().is_human() {
            assert!(progress.is_enabled());
        } else {
            assert!(!progress.is_enabled());
        }
    }
}
