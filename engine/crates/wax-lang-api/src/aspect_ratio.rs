//! Shared parsing and normalization for literal aspect ratios.

/// Parses a positive unitless number or quotient and rounds it to four decimal places.
pub fn normalize(value: &str) -> Option<f64> {
    normalize_with(value, str::trim)
}

/// Parses an aspect ratio with optional Kotlin `f` or `F` suffixes on the numbers.
pub fn normalize_kotlin(value: &str) -> Option<f64> {
    normalize_with(value, |number| {
        let number = number.trim();
        number.strip_suffix(['f', 'F']).unwrap_or(number)
    })
}

fn normalize_with(value: &str, number_text: fn(&str) -> &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/').unwrap_or((value, "1"));
    let numerator = number_text(numerator).parse::<f64>().ok()?;
    let denominator = number_text(denominator).parse::<f64>().ok()?;
    if !numerator.is_finite() || !denominator.is_finite() || numerator <= 0.0 || denominator <= 0.0
    {
        return None;
    }
    let quotient = numerator / denominator;
    if !quotient.is_finite() || quotient <= 0.0 {
        return None;
    }
    let scaled = quotient * 10_000.0;
    // At this magnitude the quotient's representable steps already exceed 0.0001.
    Some(if scaled.is_finite() {
        scaled.round() / 10_000.0
    } else {
        quotient
    })
}
