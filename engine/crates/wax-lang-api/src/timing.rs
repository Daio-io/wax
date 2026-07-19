//! Timing helpers for scan instrumentation.

use std::time::Duration;

/// Converts configured scan elapsed time into the scan-contract millisecond value.
///
/// Returns zero when no files were scanned. Non-empty scans are rounded up to
/// one millisecond so fast scans are not represented as "not measured".
#[must_use]
pub fn parse_extract_millis(duration: Duration, files_scanned: u32) -> u64 {
    if files_scanned == 0 {
        return 0;
    }

    u64::try_from(duration.as_millis().max(1)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_files_reports_zero() {
        assert_eq!(parse_extract_millis(Duration::from_secs(1), 0), 0);
    }

    #[test]
    fn sub_millisecond_work_reports_one() {
        assert_eq!(parse_extract_millis(Duration::from_nanos(1), 1), 1);
    }

    #[test]
    fn whole_milliseconds_are_preserved() {
        assert_eq!(parse_extract_millis(Duration::from_millis(42), 1), 42);
    }
}
