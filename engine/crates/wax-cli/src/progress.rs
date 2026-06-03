//! Terminal progress indicators for interactive CLI use.

use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;
use wax_core::progress::{ScanProgressEvent, ValidateProgressEvent};

/// Spinner shown on stderr when the terminal supports it.
pub struct CliProgress {
    bar: Option<ProgressBar>,
}

impl CliProgress {
    /// Starts a spinner when stderr is an interactive terminal.
    pub fn new() -> Self {
        if !std::io::stderr().is_terminal() {
            return Self { bar: None };
        }

        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .expect("progress template should be valid"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar: Some(bar) }
    }

    /// Updates the spinner message.
    pub fn set_message(&self, message: impl Into<std::borrow::Cow<'static, str>>) {
        if let Some(bar) = &self.bar {
            bar.set_message(message);
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

/// Builds a [`wax_core::progress::ScanProgress`] sink backed by a CLI spinner.
pub fn scan_progress_sink(progress: Arc<CliProgress>) -> wax_core::progress::ScanProgress {
    wax_core::progress::ScanProgress::new(move |event| {
        progress.set_message(scan_progress_message(event));
    })
}

/// Builds a [`wax_core::progress::ValidateProgress`] sink backed by a CLI spinner.
pub fn validate_progress_sink(progress: Arc<CliProgress>) -> wax_core::progress::ValidateProgress {
    wax_core::progress::ValidateProgress::new(move |event| {
        progress.set_message(validate_progress_message(event));
    })
}

fn scan_progress_message(event: ScanProgressEvent) -> std::borrow::Cow<'static, str> {
    match event {
        ScanProgressEvent::Preparing => "Preparing scan…".into(),
        ScanProgressEvent::Installing {
            language_id,
            version,
        } => format!("Installing language pack {language_id}@{version}…").into(),
        ScanProgressEvent::Scanning { language_id } => {
            format!("Scanning with {language_id}…").into()
        }
        ScanProgressEvent::ScanComplete { language_id } => {
            format!("Finished {language_id} scan…").into()
        }
        ScanProgressEvent::WritingOutputs => "Writing scan output…".into(),
    }
}

fn validate_progress_message(event: ValidateProgressEvent) -> std::borrow::Cow<'static, str> {
    match event {
        ValidateProgressEvent::LoadingConfig => "Loading wax configuration…".into(),
        ValidateProgressEvent::ValidatingLanguage { language_id } => {
            format!("Validating {language_id} registry…").into()
        }
    }
}
