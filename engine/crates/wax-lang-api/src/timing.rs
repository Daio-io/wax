//! Timing helpers for scan instrumentation.

use std::time::Duration;

use wax_contract::MAX_PARSE_EXTRACT_MS;

/// Converts configured scan elapsed time into the scan-contract millisecond value.
///
/// Returns zero when no files were scanned. Non-empty scans are clamped to the
/// contract range so fast scans are not represented as "not measured" and long
/// scans remain valid scan output.
#[must_use]
pub fn parse_extract_millis(duration: Duration, files_scanned: u32) -> u64 {
    if files_scanned == 0 {
        return 0;
    }

    duration
        .as_millis()
        .clamp(1, u128::from(MAX_PARSE_EXTRACT_MS)) as u64
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

    #[test]
    fn contract_max_is_preserved() {
        let duration = Duration::from_millis(MAX_PARSE_EXTRACT_MS);

        assert_eq!(parse_extract_millis(duration, 1), MAX_PARSE_EXTRACT_MS);
    }

    #[test]
    fn duration_above_contract_max_is_clamped() {
        let duration = Duration::from_millis(MAX_PARSE_EXTRACT_MS + 1);

        assert_eq!(parse_extract_millis(duration, 1), MAX_PARSE_EXTRACT_MS);
    }
}
