//! Optional progress callbacks for long-running engine operations.

use std::sync::Arc;
use wax_contract::LanguageId;

/// Progress events emitted during `wax scan` orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanProgressEvent {
    /// Loading config, lockfile, and resolving registries.
    Preparing,
    /// Downloading and installing a missing language pack.
    Installing {
        /// Language pack id.
        language_id: LanguageId,
        /// Locked version being installed.
        version: String,
    },
    /// Running one language pack scan subprocess (serial scans only).
    Scanning {
        /// Language pack id.
        language_id: LanguageId,
    },
    /// One language pack scan finished successfully (serial scans only).
    ScanComplete {
        /// Language pack id.
        language_id: LanguageId,
    },
    /// Parallel language scans in flight; `completed` is finished count out of `total`.
    LanguagesScanning {
        /// Languages that have finished scanning successfully so far.
        completed: usize,
        /// Total enabled languages being scanned.
        total: usize,
    },
    /// Writing merged and per-language scan output files.
    WritingOutputs,
}

/// Progress events emitted during `wax validate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateProgressEvent {
    /// Loading config and lockfile.
    LoadingConfig,
    /// Validating one enabled language registry.
    ValidatingLanguage {
        /// Language pack id.
        language_id: LanguageId,
    },
}

/// Optional scan progress sink. Default is a no-op.
#[derive(Clone, Default)]
pub struct ScanProgress {
    callback: Option<Arc<dyn Fn(ScanProgressEvent) + Send + Sync>>,
}

impl std::fmt::Debug for ScanProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.callback.is_some() {
            f.write_str("ScanProgress { callback: Some(_) }")
        } else {
            f.write_str("ScanProgress { callback: None }")
        }
    }
}

impl ScanProgress {
    /// Creates a progress sink that invokes `callback` for each event.
    pub fn new(callback: impl Fn(ScanProgressEvent) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    /// Emits `event` when a callback is configured.
    ///
    /// # Panics
    ///
    /// Panics if the callback supplied to [`ScanProgress::new`] panics while
    /// handling `event`.
    pub fn emit(&self, event: ScanProgressEvent) {
        if let Some(callback) = &self.callback {
            callback(event);
        }
    }
}

/// Optional validate progress sink. Default is a no-op.
#[derive(Clone, Default)]
pub struct ValidateProgress {
    callback: Option<Arc<dyn Fn(ValidateProgressEvent) + Send + Sync>>,
}

impl std::fmt::Debug for ValidateProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.callback.is_some() {
            f.write_str("ValidateProgress { callback: Some(_) }")
        } else {
            f.write_str("ValidateProgress { callback: None }")
        }
    }
}

impl ValidateProgress {
    /// Creates a progress sink that invokes `callback` for each event.
    pub fn new(callback: impl Fn(ValidateProgressEvent) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    /// Emits `event` when a callback is configured.
    ///
    /// # Panics
    ///
    /// Panics if the callback supplied to [`ValidateProgress::new`] panics while
    /// handling `event`.
    pub fn emit(&self, event: ValidateProgressEvent) {
        if let Some(callback) = &self.callback {
            callback(event);
        }
    }
}
