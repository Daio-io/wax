//! Deterministic hard-coded style token inference.

use std::collections::BTreeMap;

use wax_contract::{
    DesignSystemToken, HardcodedStyleInference, HardcodedStyleSite, LanguageId, ScanFacts,
    ScanFactsError, StyleContext, StyleContextCounts, TokenCategory, TokenCategoryCounts,
    TokenInferenceClassification, TokenInferenceConfidence, TokenInferenceCounts,
    TokenInferenceEvidence, TokenInferenceReport, TokenMatchKind, TokenReplacementSuggestion,
};

use crate::config::waxrc::TokenInferenceConfig;

#[derive(Debug, Clone, PartialEq)]
enum NormalizedValue {
    Numeric {
        scalar: f64,
        unit: Option<String>,
        allow_near: bool,
    },
    Color([u8; 4]),
    ExactText(String),
    Unsupported,
}

struct InferredSite<'a> {
    row: HardcodedStyleInference,
    source: &'a HardcodedStyleSite,
}

/// Builds one inference row per raw hard-coded site using configured matching rules.
///
/// # Errors
///
/// Returns [`ScanFactsError::ContractViolation`] when observation counts exceed `u32`.
pub fn build_token_inference(
    languages: &BTreeMap<LanguageId, ScanFacts>,
    config: &TokenInferenceConfig,
) -> Result<TokenInferenceReport, ScanFactsError> {
    let mut inferred_sites = Vec::new();

    for (language_id, facts) in languages {
        for site in &facts.hardcoded_style_sites {
            inferred_sites.push(InferredSite {
                row: infer_site(
                    language_id,
                    site,
                    &facts.design_system_tokens,
                    config.numeric_tolerance,
                ),
                source: site,
            });
        }
    }

    inferred_sites.sort_by(|left, right| {
        (
            left.row.language.as_str(),
            left.source.location.file.as_str(),
            left.source.location.line,
            left.source.location.column.unwrap_or(0),
            left.row.site_id.as_str(),
        )
            .cmp(&(
                right.row.language.as_str(),
                right.source.location.file.as_str(),
                right.source.location.line,
                right.source.location.column.unwrap_or(0),
                right.row.site_id.as_str(),
            ))
    });

    let counts = derive_counts(&inferred_sites)?;
    let sites = inferred_sites.into_iter().map(|site| site.row).collect();
    Ok(TokenInferenceReport {
        numeric_tolerance: config.numeric_tolerance,
        counts,
        sites,
    })
}

fn derive_counts(sites: &[InferredSite<'_>]) -> Result<TokenInferenceCounts, ScanFactsError> {
    let hardcoded_observation_count =
        u32::try_from(sites.len()).map_err(|_| ScanFactsError::ContractViolation {
            field: "token_inference.counts.hardcoded_observation_count".to_owned(),
            message: "hardcoded observation count exceeds u32 maximum".to_owned(),
        })?;
    let mut counts = TokenInferenceCounts {
        hardcoded_observation_count,
        ..TokenInferenceCounts::default()
    };

    for site in sites {
        match site.row.classification {
            TokenInferenceClassification::Exact => {
                counts.exact_replacement_candidate_count =
                    counts.exact_replacement_candidate_count.saturating_add(1);
                bump_candidate_breakdown(site, &mut counts);
            }
            TokenInferenceClassification::Near => {
                counts.near_replacement_candidate_count =
                    counts.near_replacement_candidate_count.saturating_add(1);
                bump_candidate_breakdown(site, &mut counts);
            }
            TokenInferenceClassification::Unmatched => {
                counts.unmatched_observation_count =
                    counts.unmatched_observation_count.saturating_add(1);
            }
            TokenInferenceClassification::Unassessed => {
                counts.unassessed_observation_count =
                    counts.unassessed_observation_count.saturating_add(1);
            }
        }
    }

    counts.assessed_observation_count = counts
        .exact_replacement_candidate_count
        .saturating_add(counts.near_replacement_candidate_count)
        .saturating_add(counts.unmatched_observation_count);
    Ok(counts)
}

fn bump_candidate_breakdown(site: &InferredSite<'_>, counts: &mut TokenInferenceCounts) {
    match site.row.confidence {
        Some(TokenInferenceConfidence::VeryHigh) => {
            counts.candidates_by_confidence.very_high =
                counts.candidates_by_confidence.very_high.saturating_add(1);
        }
        Some(TokenInferenceConfidence::High) => {
            counts.candidates_by_confidence.high =
                counts.candidates_by_confidence.high.saturating_add(1);
        }
        Some(TokenInferenceConfidence::Medium) => {
            counts.candidates_by_confidence.medium =
                counts.candidates_by_confidence.medium.saturating_add(1);
        }
        Some(TokenInferenceConfidence::Low) => {
            counts.candidates_by_confidence.low =
                counts.candidates_by_confidence.low.saturating_add(1);
        }
        None => {}
    }

    bump_category(&mut counts.candidates_by_category, site.source.category);
    bump_context(&mut counts.candidates_by_context, site.source.context);
}

fn bump_category(counts: &mut TokenCategoryCounts, category: TokenCategory) {
    match category {
        TokenCategory::Color => counts.color = counts.color.saturating_add(1),
        TokenCategory::Spacing => counts.spacing = counts.spacing.saturating_add(1),
        TokenCategory::Typography => counts.typography = counts.typography.saturating_add(1),
        TokenCategory::Radius => counts.radius = counts.radius.saturating_add(1),
        TokenCategory::Elevation => counts.elevation = counts.elevation.saturating_add(1),
        TokenCategory::Unknown => counts.unknown = counts.unknown.saturating_add(1),
    }
}

fn bump_context(counts: &mut StyleContextCounts, context: StyleContext) {
    match context {
        StyleContext::Padding => counts.padding = counts.padding.saturating_add(1),
        StyleContext::Margin => counts.margin = counts.margin.saturating_add(1),
        StyleContext::Gap => counts.gap = counts.gap.saturating_add(1),
        StyleContext::Width => counts.width = counts.width.saturating_add(1),
        StyleContext::Height => counts.height = counts.height.saturating_add(1),
        StyleContext::Size => counts.size = counts.size.saturating_add(1),
        StyleContext::Radius => counts.radius = counts.radius.saturating_add(1),
        StyleContext::Color => counts.color = counts.color.saturating_add(1),
        StyleContext::Typography => counts.typography = counts.typography.saturating_add(1),
        StyleContext::Elevation => counts.elevation = counts.elevation.saturating_add(1),
        StyleContext::Unknown => counts.unknown = counts.unknown.saturating_add(1),
    }
}

fn infer_site(
    language_id: &LanguageId,
    site: &HardcodedStyleSite,
    tokens: &[DesignSystemToken],
    tolerance: f64,
) -> HardcodedStyleInference {
    let same_category: Vec<&DesignSystemToken> = tokens
        .iter()
        .filter(|token| token.category == site.category)
        .collect();

    if same_category.is_empty() {
        return unassessed(
            language_id,
            site,
            vec![TokenInferenceEvidence::MissingCanonicalValues],
        );
    }

    let observed = normalize_observed(language_id, site);
    if matches!(observed, NormalizedValue::Unsupported) {
        return unassessed(
            language_id,
            site,
            vec![TokenInferenceEvidence::UnsupportedCanonicalFormat],
        );
    }

    let mut usable = Vec::new();
    let mut missing_canonical = false;
    let mut unsupported_canonical = false;

    for token in &same_category {
        match &token.value {
            None => {
                missing_canonical = true;
            }
            Some(value) => {
                let normalized =
                    normalize_canonical(language_id, site.category, site.context, value);
                match normalized {
                    NormalizedValue::Unsupported => {
                        unsupported_canonical = true;
                    }
                    other => usable.push((*token, other, value.as_str())),
                }
            }
        }
    }

    let mut exact_matches = Vec::new();
    for (token, normalized, canonical_value) in &usable {
        if values_exact_match(&observed, normalized) {
            exact_matches.push((*token, normalized.clone(), *canonical_value));
        }
    }

    if !exact_matches.is_empty() {
        exact_matches.sort_by(|left, right| left.0.id.cmp(&right.0.id));
        let multiple = exact_matches.len() > 1;
        let suggestions = exact_matches
            .iter()
            .map(|(token, normalized, canonical_value)| {
                suggestion_for(token, canonical_value, TokenMatchKind::Exact, normalized)
            })
            .collect();
        let mut evidence = vec![
            TokenInferenceEvidence::ExactValue,
            context_evidence(site.context),
        ];
        if multiple {
            evidence.push(TokenInferenceEvidence::MultipleEqualMatches);
        }
        evidence.sort();
        return HardcodedStyleInference {
            language: language_id.clone(),
            site_id: site.id.clone(),
            classification: TokenInferenceClassification::Exact,
            confidence: Some(confidence_for(
                TokenInferenceClassification::Exact,
                site.context,
                multiple,
            )),
            suggestions,
            evidence,
        };
    }

    if usable.is_empty() {
        let mut evidence = Vec::new();
        if missing_canonical {
            evidence.push(TokenInferenceEvidence::MissingCanonicalValues);
        }
        if unsupported_canonical {
            evidence.push(TokenInferenceEvidence::UnsupportedCanonicalFormat);
        }
        if missing_canonical || unsupported_canonical {
            evidence.push(TokenInferenceEvidence::IncompleteCanonicalCoverage);
        }
        evidence.sort();
        evidence.dedup();
        return unassessed(language_id, site, evidence);
    }

    if !matches!(
        observed,
        NormalizedValue::Numeric {
            allow_near: true,
            ..
        }
    ) {
        return unmatched(
            language_id,
            site,
            vec![TokenInferenceEvidence::OutsideNumericTolerance],
        );
    }

    let mut near_candidates = Vec::new();
    let mut saw_incompatible = false;
    for (token, normalized, canonical_value) in &usable {
        match numeric_distance(&observed, normalized) {
            Some(distance) if distance > 0.0 && distance <= tolerance => {
                near_candidates.push((*token, normalized.clone(), *canonical_value, distance));
            }
            Some(_) => {}
            None => {
                if matches!(normalized, NormalizedValue::Numeric { .. }) {
                    saw_incompatible = true;
                }
            }
        }
    }

    if near_candidates.is_empty() {
        let mut evidence = vec![TokenInferenceEvidence::OutsideNumericTolerance];
        if saw_incompatible {
            evidence.push(TokenInferenceEvidence::IncompatibleUnits);
        }
        evidence.sort();
        return unmatched(language_id, site, evidence);
    }

    near_candidates.sort_by(|left, right| {
        left.3
            .total_cmp(&right.3)
            .then_with(|| left.0.id.cmp(&right.0.id))
    });
    let best_distance = near_candidates[0].3;
    near_candidates.retain(|candidate| candidate.3 == best_distance);
    let multiple = near_candidates.len() > 1;
    let suggestions = near_candidates
        .iter()
        .map(|(token, normalized, canonical_value, distance)| {
            let mut suggestion =
                suggestion_for(token, canonical_value, TokenMatchKind::Near, normalized);
            suggestion.distance = Some(*distance);
            suggestion
        })
        .collect();
    let mut evidence = vec![
        TokenInferenceEvidence::WithinNumericTolerance,
        context_evidence(site.context),
    ];
    if multiple {
        evidence.push(TokenInferenceEvidence::MultipleEqualMatches);
    }
    evidence.sort();
    HardcodedStyleInference {
        language: language_id.clone(),
        site_id: site.id.clone(),
        classification: TokenInferenceClassification::Near,
        confidence: Some(confidence_for(
            TokenInferenceClassification::Near,
            site.context,
            multiple,
        )),
        suggestions,
        evidence,
    }
}

fn unassessed(
    language_id: &LanguageId,
    site: &HardcodedStyleSite,
    evidence: Vec<TokenInferenceEvidence>,
) -> HardcodedStyleInference {
    HardcodedStyleInference {
        language: language_id.clone(),
        site_id: site.id.clone(),
        classification: TokenInferenceClassification::Unassessed,
        confidence: None,
        suggestions: Vec::new(),
        evidence,
    }
}

fn unmatched(
    language_id: &LanguageId,
    site: &HardcodedStyleSite,
    evidence: Vec<TokenInferenceEvidence>,
) -> HardcodedStyleInference {
    HardcodedStyleInference {
        language: language_id.clone(),
        site_id: site.id.clone(),
        classification: TokenInferenceClassification::Unmatched,
        confidence: None,
        suggestions: Vec::new(),
        evidence,
    }
}

fn suggestion_for(
    token: &DesignSystemToken,
    canonical_value: &str,
    match_kind: TokenMatchKind,
    normalized: &NormalizedValue,
) -> TokenReplacementSuggestion {
    let (distance, normalized_unit) = match (match_kind, normalized) {
        (TokenMatchKind::Exact, NormalizedValue::Numeric { unit, .. }) => (Some(0.0), unit.clone()),
        (TokenMatchKind::Exact, _) => (None, None),
        (TokenMatchKind::Near, NormalizedValue::Numeric { unit, .. }) => (None, unit.clone()),
        (TokenMatchKind::Near, _) => (None, None),
    };
    TokenReplacementSuggestion {
        token_id: token.id.clone(),
        token_key: token.key.clone(),
        canonical_value: canonical_value.to_owned(),
        match_kind,
        distance,
        normalized_unit,
    }
}

fn context_evidence(context: StyleContext) -> TokenInferenceEvidence {
    if is_clear_context(context) {
        TokenInferenceEvidence::ClearUsageContext
    } else {
        TokenInferenceEvidence::GenericDimensionContext
    }
}

fn is_clear_context(context: StyleContext) -> bool {
    matches!(
        context,
        StyleContext::Padding
            | StyleContext::Margin
            | StyleContext::Gap
            | StyleContext::Radius
            | StyleContext::Color
            | StyleContext::Typography
            | StyleContext::Elevation
    )
}

fn confidence_for(
    classification: TokenInferenceClassification,
    context: StyleContext,
    multiple_equal: bool,
) -> TokenInferenceConfidence {
    let clear = is_clear_context(context);
    let mut confidence = match (classification, clear) {
        (TokenInferenceClassification::Exact, true) => TokenInferenceConfidence::VeryHigh,
        (TokenInferenceClassification::Exact, false) => TokenInferenceConfidence::High,
        (TokenInferenceClassification::Near, true) => TokenInferenceConfidence::Medium,
        (TokenInferenceClassification::Near, false) => TokenInferenceConfidence::Low,
        _ => TokenInferenceConfidence::Low,
    };
    if multiple_equal {
        confidence = match confidence {
            TokenInferenceConfidence::VeryHigh => TokenInferenceConfidence::High,
            TokenInferenceConfidence::High => TokenInferenceConfidence::Medium,
            TokenInferenceConfidence::Medium => TokenInferenceConfidence::Low,
            TokenInferenceConfidence::Low => TokenInferenceConfidence::Low,
        };
    }
    confidence
}

fn values_exact_match(left: &NormalizedValue, right: &NormalizedValue) -> bool {
    match (left, right) {
        (
            NormalizedValue::Numeric {
                scalar: left_scalar,
                unit: left_unit,
                ..
            },
            NormalizedValue::Numeric {
                scalar: right_scalar,
                unit: right_unit,
                ..
            },
        ) => left_unit == right_unit && left_scalar == right_scalar,
        (NormalizedValue::Color(left), NormalizedValue::Color(right)) => left == right,
        (NormalizedValue::ExactText(left_text), NormalizedValue::ExactText(right_text)) => {
            left_text == right_text
        }
        _ => false,
    }
}

fn numeric_distance(left: &NormalizedValue, right: &NormalizedValue) -> Option<f64> {
    match (left, right) {
        (
            NormalizedValue::Numeric {
                scalar: left_scalar,
                unit: left_unit,
                allow_near: true,
            },
            NormalizedValue::Numeric {
                scalar: right_scalar,
                unit: right_unit,
                allow_near: true,
            },
        ) if left_unit == right_unit => Some((left_scalar - right_scalar).abs()),
        _ => None,
    }
}

fn normalize_observed(language: &LanguageId, site: &HardcodedStyleSite) -> NormalizedValue {
    normalize_value(language, site.category, site.context, &site.value)
}

fn normalize_canonical(
    language: &LanguageId,
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue {
    normalize_value(language, category, context, value)
}

fn normalize_value(
    language: &LanguageId,
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue {
    let trimmed = strip_wrapping_quotes(value.trim());
    if trimmed.is_empty() {
        return NormalizedValue::Unsupported;
    }

    if let Some(color) = parse_hash_color(trimmed) {
        return NormalizedValue::Color(color);
    }

    match language.as_str() {
        "compose" => normalize_compose(category, context, trimmed),
        "react" => normalize_react(category, context, trimmed),
        "swift" => normalize_swift(category, context, trimmed),
        _ => normalize_generic(category, context, trimmed),
    }
}

fn strip_wrapping_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn normalize_compose(
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue {
    if let Some(color) = normalize_compose_hex_color(category, context, value) {
        return NormalizedValue::Color(color);
    }

    if let Some(normalized) = parse_compose_numeric(value, allow_numeric_near(category, context)) {
        if matches!(normalized, NormalizedValue::Unsupported)
            && supports_exact_text_fallback(category, context)
        {
            return NormalizedValue::ExactText(value.to_owned());
        }
        return normalized;
    }

    NormalizedValue::ExactText(value.to_owned())
}

fn parse_compose_numeric(value: &str, allow_near: bool) -> Option<NormalizedValue> {
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    for (suffix, unit) in [(".dp", "dp"), ("dp", "dp"), (".sp", "sp"), ("sp", "sp")] {
        if let Some(number) = strip_suffix_ignore_ascii_case(&compact, suffix)
            && let Some(normalized) = normalize_numeric(number, Some(unit), allow_near)
        {
            return Some(normalized);
        }
    }

    if let Some(scalar) = parse_finite_scalar(&compact) {
        return Some(NormalizedValue::Numeric {
            scalar,
            unit: None,
            allow_near,
        });
    }

    if split_numeric_prefix(&compact).is_some() {
        Some(NormalizedValue::Unsupported)
    } else {
        None
    }
}

fn normalize_react(category: TokenCategory, context: StyleContext, value: &str) -> NormalizedValue {
    if let Some(normalized) = parse_css_length(
        value,
        allow_unitless_px(context),
        allow_numeric_near(category, context),
    ) {
        if matches!(normalized, NormalizedValue::Unsupported)
            && supports_exact_text_fallback(category, context)
        {
            return NormalizedValue::ExactText(value.to_owned());
        }
        return normalized;
    }

    NormalizedValue::ExactText(value.to_owned())
}

fn normalize_swift(category: TokenCategory, context: StyleContext, value: &str) -> NormalizedValue {
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Ok(scalar) = compact.parse::<f64>() {
        return if scalar.is_finite() {
            NormalizedValue::Numeric {
                scalar,
                unit: None,
                allow_near: allow_numeric_near(category, context),
            }
        } else {
            NormalizedValue::Unsupported
        };
    }

    NormalizedValue::ExactText(value.to_owned())
}

fn normalize_generic(
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue {
    if let Some(normalized) = parse_css_length(
        value,
        allow_unitless_px(context),
        allow_numeric_near(category, context),
    ) {
        if matches!(normalized, NormalizedValue::Unsupported)
            && supports_exact_text_fallback(category, context)
        {
            return NormalizedValue::ExactText(value.to_owned());
        }
        return normalized;
    }
    NormalizedValue::ExactText(value.to_owned())
}

fn allow_numeric_near(category: TokenCategory, context: StyleContext) -> bool {
    !matches!(category, TokenCategory::Color | TokenCategory::Elevation)
        && !matches!(context, StyleContext::Color | StyleContext::Elevation)
}

fn supports_exact_text_fallback(category: TokenCategory, context: StyleContext) -> bool {
    matches!(
        category,
        TokenCategory::Color | TokenCategory::Typography | TokenCategory::Elevation
    ) || matches!(
        context,
        StyleContext::Color | StyleContext::Typography | StyleContext::Elevation
    )
}

fn normalize_compose_hex_color(
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> Option<[u8; 4]> {
    if category != TokenCategory::Color && context != StyleContext::Color {
        return None;
    }
    let trimmed = value.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))?;
    if !matches!(hex.len(), 6 | 8) || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let (alpha, red_at) = if hex.len() == 8 {
        (parse_hex_byte(hex, 0)?, 2)
    } else {
        (u8::MAX, 0)
    };
    Some([
        parse_hex_byte(hex, red_at)?,
        parse_hex_byte(hex, red_at + 2)?,
        parse_hex_byte(hex, red_at + 4)?,
        alpha,
    ])
}

fn allow_unitless_px(context: StyleContext) -> bool {
    matches!(
        context,
        StyleContext::Padding
            | StyleContext::Margin
            | StyleContext::Gap
            | StyleContext::Width
            | StyleContext::Height
            | StyleContext::Size
            | StyleContext::Radius
    )
}

fn parse_css_length(
    value: &str,
    unitless_as_px: bool,
    allow_near: bool,
) -> Option<NormalizedValue> {
    let compact = value.trim();
    for unit in ["vmin", "vmax", "rem", "px", "em", "vh", "vw", "%"] {
        if let Some(number) = strip_suffix_ignore_ascii_case(compact, unit)
            && let Some(normalized) = normalize_numeric(number, Some(unit), allow_near)
        {
            return Some(normalized);
        }
    }

    if let Some(scalar) = parse_finite_scalar(compact) {
        return Some(NormalizedValue::Numeric {
            scalar,
            unit: unitless_as_px.then(|| "px".to_owned()),
            allow_near,
        });
    }

    if split_numeric_prefix(compact).is_some() {
        Some(NormalizedValue::Unsupported)
    } else {
        None
    }
}

fn normalize_numeric(
    number: &str,
    unit: Option<&str>,
    allow_near: bool,
) -> Option<NormalizedValue> {
    let scalar = number.parse::<f64>().ok()?;
    if !scalar.is_finite() {
        return Some(NormalizedValue::Unsupported);
    }
    Some(NormalizedValue::Numeric {
        scalar,
        unit: unit.map(str::to_owned),
        allow_near,
    })
}

fn parse_finite_scalar(value: &str) -> Option<f64> {
    let scalar = value.parse::<f64>().ok()?;
    scalar.is_finite().then_some(scalar)
}

fn strip_suffix_ignore_ascii_case<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let split_at = value.len().checked_sub(suffix.len())?;
    let prefix = value.get(..split_at)?;
    let candidate = value.get(split_at..)?;
    candidate.eq_ignore_ascii_case(suffix).then_some(prefix)
}

fn split_numeric_prefix(value: &str) -> Option<(&str, &str)> {
    value
        .char_indices()
        .rev()
        .map(|(index, _)| index)
        .filter(|index| *index > 0)
        .find_map(|index| {
            let (number, suffix) = value.split_at(index);
            (!suffix.is_empty() && parse_finite_scalar(number).is_some())
                .then_some((number, suffix))
        })
}

fn parse_hash_color(value: &str) -> Option<[u8; 4]> {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#')?;
    if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }

    if matches!(hex.len(), 3 | 4) {
        let red = parse_hex_nibble(hex, 0)?;
        let green = parse_hex_nibble(hex, 1)?;
        let blue = parse_hex_nibble(hex, 2)?;
        let alpha = if hex.len() == 4 {
            parse_hex_nibble(hex, 3)?
        } else {
            u8::MAX
        };
        return Some([red, green, blue, alpha]);
    }

    Some([
        parse_hex_byte(hex, 0)?,
        parse_hex_byte(hex, 2)?,
        parse_hex_byte(hex, 4)?,
        if hex.len() == 8 {
            parse_hex_byte(hex, 6)?
        } else {
            u8::MAX
        },
    ])
}

fn parse_hex_nibble(hex: &str, index: usize) -> Option<u8> {
    let value = hex.as_bytes().get(index).copied()?;
    let value = char::from(value).to_digit(16)? as u8;
    Some(value * 17)
}

fn parse_hex_byte(hex: &str, index: usize) -> Option<u8> {
    let end = index.checked_add(2)?;
    let pair = hex.get(index..end)?;
    u8::from_str_radix(pair, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wax_contract::{SourceLocation, TokenCategory};

    fn language(id: &str) -> LanguageId {
        LanguageId::try_from(id).unwrap()
    }

    fn site(
        language_name: &str,
        value: &str,
        category: TokenCategory,
        context: StyleContext,
    ) -> (LanguageId, HardcodedStyleSite) {
        let language_id = language(language_name);
        (
            language_id,
            HardcodedStyleSite {
                id: format!("hardcoded.{language_name}:src/Card:1:1:{category:?}"),
                location: SourceLocation {
                    file: "src/Card".to_owned(),
                    line: 1,
                    column: Some(1),
                },
                value: value.to_owned(),
                category,
                context,
                parent: None,
            },
        )
    }

    fn token(id: &str, category: TokenCategory, value: Option<&str>) -> DesignSystemToken {
        DesignSystemToken {
            id: id.to_owned(),
            key: id.to_owned(),
            category,
            aliases: Vec::new(),
            value: value.map(str::to_owned),
        }
    }

    fn facts(
        language_id: &LanguageId,
        tokens: Vec<DesignSystemToken>,
        sites: Vec<HardcodedStyleSite>,
    ) -> ScanFacts {
        let mut facts = ScanFacts {
            schema_version: 3,
            language: wax_contract::LanguageMetadata {
                id: language_id.clone(),
                version: "0.1.0".into(),
                ecosystem: "test".into(),
                parser_name: "test".into(),
                parser_version: "1.0.0".into(),
            },
            snapshot_id: "snap".into(),
            scanned_at: time::OffsetDateTime::UNIX_EPOCH,
            status: wax_contract::ScanStatus::Complete,
            design_system_components: Vec::new(),
            local_components: Vec::new(),
            usage_sites: Vec::new(),
            diagnostics: Vec::new(),
            metrics: wax_contract::Metrics {
                invocation_adoption_ratio: None,
                registry_resolution_ratio: None,
                parse_extract_ms: 0,
                files_scanned: 0,
            },
            counts: wax_contract::CountSummary::default(),
            symbol_usage_summary: Vec::new(),
            design_system_tokens: tokens,
            token_sites: Vec::new(),
            hardcoded_style_sites: sites,
            token_usage_summary: Vec::new(),
        };
        facts.recompute_counts().unwrap();
        facts
    }

    #[test]
    fn normalize_compose_dp_variants_are_exact_and_sp_is_incompatible() {
        let language_id = language("compose");
        let site_dp = HardcodedStyleSite {
            id: "s".into(),
            location: SourceLocation {
                file: "a.kt".into(),
                line: 1,
                column: None,
            },
            value: "4.dp".into(),
            category: TokenCategory::Spacing,
            context: StyleContext::Padding,
            parent: None,
        };
        let left = normalize_observed(&language_id, &site_dp);
        let right = normalize_canonical(
            &language_id,
            TokenCategory::Spacing,
            StyleContext::Padding,
            "4dp",
        );
        assert!(values_exact_match(&left, &right));
        assert_eq!(
            numeric_distance(
                &left,
                &normalize_canonical(
                    &language_id,
                    TokenCategory::Spacing,
                    StyleContext::Padding,
                    "4.sp",
                )
            ),
            None
        );
    }

    #[test]
    fn normalize_numeric_precision_variants_are_exact_across_languages_and_categories() {
        let category_cases = [
            (TokenCategory::Spacing, StyleContext::Padding),
            (TokenCategory::Typography, StyleContext::Typography),
            (TokenCategory::Radius, StyleContext::Radius),
            (TokenCategory::Elevation, StyleContext::Elevation),
            (TokenCategory::Unknown, StyleContext::Unknown),
        ];

        for language_name in ["compose", "react", "swift", "basic"] {
            for (category, context) in category_cases {
                let (observed, canonical) = match (language_name, category) {
                    ("compose", TokenCategory::Typography) => ("1.600e1.sp", "16.sp"),
                    ("compose", TokenCategory::Unknown) => ("16.00", "16"),
                    ("compose", _) => ("16.00.dp", "16.dp"),
                    ("react" | "basic", TokenCategory::Typography) => ("1.600e1rem", "16rem"),
                    ("react" | "basic", TokenCategory::Unknown) => ("16.00", "16"),
                    ("react" | "basic", _) => ("16.00px", "16px"),
                    ("swift", _) => ("16.00", "16"),
                    _ => unreachable!(),
                };
                let language_id = language(language_name);
                let observed = normalize_value(&language_id, category, context, observed);
                let canonical = normalize_value(&language_id, category, context, canonical);
                assert!(
                    values_exact_match(&observed, &canonical),
                    "expected {language_name} {category:?} {observed:?} to equal {canonical:?}"
                );
            }
        }
    }

    #[test]
    fn normalize_equivalent_hex_colors_across_languages() {
        let cases = [
            ("compose", "#fff", "#ffffff"),
            ("compose", "0x112233", "0xFF112233"),
            ("compose", "0x80ff0000", "#ff000080"),
            ("react", "#fff", "#ffffff"),
            ("react", "#0f08", "#00ff0088"),
            ("swift", "#fff", "#ffffff"),
            ("basic", "#0f08", "#00ff0088"),
        ];

        for (language_name, observed, canonical) in cases {
            let language_id = language(language_name);
            let observed = normalize_value(
                &language_id,
                TokenCategory::Color,
                StyleContext::Color,
                observed,
            );
            let canonical = normalize_value(
                &language_id,
                TokenCategory::Color,
                StyleContext::Color,
                canonical,
            );
            assert!(
                values_exact_match(&observed, &canonical),
                "expected {language_name} {observed:?} to equal {canonical:?}"
            );
        }
    }

    #[test]
    fn normalize_non_ascii_values_remain_exact_text() {
        for language_name in ["compose", "react", "swift", "basic"] {
            let language_id = language(language_name);
            let normalized = normalize_value(
                &language_id,
                TokenCategory::Unknown,
                StyleContext::Unknown,
                "éx",
            );
            assert_eq!(normalized, NormalizedValue::ExactText("éx".to_owned()));
        }
    }

    #[test]
    fn normalize_unit_suffix_without_numeric_receiver_remains_exact_text() {
        for (language_name, value) in [
            ("compose", "namedDp"),
            ("react", "system"),
            ("basic", "system"),
        ] {
            let language_id = language(language_name);
            let normalized = normalize_value(
                &language_id,
                TokenCategory::Unknown,
                StyleContext::Unknown,
                value,
            );
            assert_eq!(normalized, NormalizedValue::ExactText(value.to_owned()));
        }
    }

    #[test]
    fn normalize_swift_non_finite_numeric_value_is_unsupported() {
        let language_id = language("swift");
        assert_eq!(
            normalize_value(
                &language_id,
                TokenCategory::Spacing,
                StyleContext::Gap,
                "NaN",
            ),
            NormalizedValue::Unsupported
        );
    }

    #[test]
    fn normalize_react_width_unitless_matches_px_and_rejects_rem() {
        let language_id = language("react");
        let observed = normalize_observed(
            &language_id,
            &HardcodedStyleSite {
                id: "s".into(),
                location: SourceLocation {
                    file: "a.tsx".into(),
                    line: 1,
                    column: None,
                },
                value: "4".into(),
                category: TokenCategory::Spacing,
                context: StyleContext::Width,
                parent: None,
            },
        );
        assert!(values_exact_match(
            &observed,
            &normalize_canonical(
                &language_id,
                TokenCategory::Spacing,
                StyleContext::Width,
                "4px",
            )
        ));
        assert_eq!(
            numeric_distance(
                &observed,
                &normalize_canonical(
                    &language_id,
                    TokenCategory::Spacing,
                    StyleContext::Width,
                    "1rem",
                )
            ),
            None
        );
    }

    #[test]
    fn normalize_react_color_is_typed_case_insensitive_and_without_distance() {
        let language_id = language("react");
        let observed = normalize_observed(
            &language_id,
            &HardcodedStyleSite {
                id: "s".into(),
                location: SourceLocation {
                    file: "a.tsx".into(),
                    line: 1,
                    column: None,
                },
                value: "\"#FFF\"".into(),
                category: TokenCategory::Color,
                context: StyleContext::Color,
                parent: None,
            },
        );
        let canonical = normalize_canonical(
            &language_id,
            TokenCategory::Color,
            StyleContext::Color,
            "#fff",
        );
        assert!(values_exact_match(&observed, &canonical));
        assert!(matches!(observed, NormalizedValue::Color(_)));
    }

    #[test]
    fn normalize_swift_gap_reports_numeric_distance() {
        let language_id = language("swift");
        let left = normalize_observed(
            &language_id,
            &HardcodedStyleSite {
                id: "s".into(),
                location: SourceLocation {
                    file: "a.swift".into(),
                    line: 1,
                    column: None,
                },
                value: "3".into(),
                category: TokenCategory::Spacing,
                context: StyleContext::Gap,
                parent: None,
            },
        );
        let right =
            normalize_canonical(&language_id, TokenCategory::Spacing, StyleContext::Gap, "4");
        assert_eq!(numeric_distance(&left, &right), Some(1.0));
    }

    fn classify(
        language: &str,
        value: &str,
        context: StyleContext,
        tokens: Vec<DesignSystemToken>,
        tolerance: f64,
    ) -> HardcodedStyleInference {
        classify_category(
            language,
            value,
            TokenCategory::Spacing,
            context,
            tokens,
            tolerance,
        )
    }

    fn classify_category(
        language: &str,
        value: &str,
        category: TokenCategory,
        context: StyleContext,
        tokens: Vec<DesignSystemToken>,
        tolerance: f64,
    ) -> HardcodedStyleInference {
        let (language_id, site) = site(language, value, category, context);
        let facts = facts(&language_id, tokens, vec![site]);
        let languages = BTreeMap::from([(language_id, facts)]);
        let report = build_token_inference(
            &languages,
            &TokenInferenceConfig {
                numeric_tolerance: tolerance,
            },
        )
        .unwrap();
        report.sites.into_iter().next().unwrap()
    }

    #[test]
    fn classification_exact_padding_is_very_high() {
        let row = classify(
            "react",
            "4",
            StyleContext::Padding,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Exact);
        assert_eq!(row.confidence, Some(TokenInferenceConfidence::VeryHigh));
        assert_eq!(row.suggestions[0].distance, Some(0.0));
        assert!(row.evidence.contains(&TokenInferenceEvidence::ExactValue));
        assert!(
            row.evidence
                .contains(&TokenInferenceEvidence::ClearUsageContext)
        );
    }

    #[test]
    fn classification_missing_sibling_does_not_block_exact_match() {
        let row = classify(
            "react",
            "4",
            StyleContext::Padding,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("spacing.m", TokenCategory::Spacing, None),
            ],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Exact);
        assert_eq!(row.suggestions[0].token_id, "spacing.s");
    }

    #[test]
    fn classification_exact_width_is_high() {
        let row = classify(
            "react",
            "4",
            StyleContext::Width,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Exact);
        assert_eq!(row.confidence, Some(TokenInferenceConfidence::High));
        assert!(
            row.evidence
                .contains(&TokenInferenceEvidence::GenericDimensionContext)
        );
    }

    #[test]
    fn classification_near_gap_is_medium() {
        let row = classify(
            "react",
            "3",
            StyleContext::Gap,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Near);
        assert_eq!(row.confidence, Some(TokenInferenceConfidence::Medium));
        assert_eq!(row.suggestions[0].distance, Some(1.0));
    }

    #[test]
    fn classification_near_width_is_low() {
        let row = classify(
            "react",
            "3",
            StyleContext::Width,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Near);
        assert_eq!(row.confidence, Some(TokenInferenceConfidence::Low));
    }

    #[test]
    fn classification_tolerance_zero_disables_near() {
        let row = classify(
            "react",
            "3",
            StyleContext::Gap,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            0.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
        assert!(row.confidence.is_none());
        assert!(row.suggestions.is_empty());
    }

    #[test]
    fn classification_far_value_is_unmatched_without_confidence() {
        let row = classify(
            "react",
            "200",
            StyleContext::Width,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
        assert!(row.confidence.is_none());
        assert!(row.suggestions.is_empty());
    }

    #[test]
    fn classification_missing_canonical_is_unassessed() {
        let row = classify(
            "react",
            "4",
            StyleContext::Padding,
            vec![token("spacing.s", TokenCategory::Spacing, None)],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unassessed);
        assert!(
            row.evidence
                .contains(&TokenInferenceEvidence::MissingCanonicalValues)
        );
    }

    #[test]
    fn classification_missing_sibling_does_not_block_near_match() {
        let row = classify(
            "react",
            "3",
            StyleContext::Padding,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("spacing.m", TokenCategory::Spacing, None),
            ],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Near);
        assert_eq!(row.suggestions[0].token_id, "spacing.s");
        assert_eq!(row.suggestions[0].distance, Some(1.0));
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::MissingCanonicalValues)
        );
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::IncompleteCanonicalCoverage)
        );
    }

    #[test]
    fn classification_missing_sibling_does_not_block_unmatched() {
        let row = classify(
            "react",
            "200",
            StyleContext::Width,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("spacing.m", TokenCategory::Spacing, None),
            ],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
        assert!(row.suggestions.is_empty());
        assert!(row.confidence.is_none());
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::MissingCanonicalValues)
        );
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::IncompleteCanonicalCoverage)
        );
    }

    #[test]
    fn classification_unsupported_sibling_does_not_block_near_match() {
        let row = classify(
            "react",
            "3",
            StyleContext::Padding,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("spacing.legacy", TokenCategory::Spacing, Some("8pt")),
            ],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Near);
        assert_eq!(row.suggestions[0].token_id, "spacing.s");
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::UnsupportedCanonicalFormat)
        );
        assert!(
            !row.evidence
                .contains(&TokenInferenceEvidence::IncompleteCanonicalCoverage)
        );
    }

    #[test]
    fn classification_different_category_value_does_not_participate() {
        let row = classify(
            "react",
            "3",
            StyleContext::Padding,
            vec![token("radius.s", TokenCategory::Radius, Some("4px"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unassessed);
    }

    #[test]
    fn partial_coverage_counts_near_and_unmatched_as_assessed() {
        let (language_id, mut near_site) =
            site("react", "3", TokenCategory::Spacing, StyleContext::Gap);
        near_site.id = "hardcoded.react:src/Card:1:1:spacing-near".to_owned();
        let (_, mut unmatched_site) =
            site("react", "200", TokenCategory::Spacing, StyleContext::Width);
        unmatched_site.id = "hardcoded.react:src/Card:2:1:spacing-unmatched".to_owned();
        unmatched_site.location.line = 2;
        let facts = facts(
            &language_id,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("spacing.m", TokenCategory::Spacing, None),
            ],
            vec![near_site, unmatched_site],
        );
        let report = build_token_inference(
            &BTreeMap::from([(language_id, facts)]),
            &TokenInferenceConfig::default(),
        )
        .unwrap();

        assert_eq!(report.counts.hardcoded_observation_count, 2);
        assert_eq!(report.counts.assessed_observation_count, 2);
        assert_eq!(report.counts.near_replacement_candidate_count, 1);
        assert_eq!(report.counts.unmatched_observation_count, 1);
        assert_eq!(report.counts.unassessed_observation_count, 0);
    }

    #[test]
    fn classification_equal_exact_matches_reduce_confidence_and_sort_ids() {
        let row = classify(
            "react",
            "4",
            StyleContext::Padding,
            vec![
                token("spacing.z", TokenCategory::Spacing, Some("4px")),
                token("spacing.a", TokenCategory::Spacing, Some("4px")),
            ],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Exact);
        assert_eq!(row.confidence, Some(TokenInferenceConfidence::High));
        assert_eq!(row.suggestions.len(), 2);
        assert_eq!(row.suggestions[0].token_id, "spacing.a");
        assert_eq!(row.suggestions[1].token_id, "spacing.z");
        assert!(
            row.evidence
                .contains(&TokenInferenceEvidence::MultipleEqualMatches)
        );
    }

    #[test]
    fn classification_no_same_category_tokens_is_unassessed() {
        let row = classify(
            "react",
            "4",
            StyleContext::Padding,
            vec![token("color.primary", TokenCategory::Color, Some("#fff"))],
            2.0,
        );
        assert_eq!(row.classification, TokenInferenceClassification::Unassessed);
    }

    #[test]
    fn classification_compose_native_argb_color_is_exact() {
        let row = classify_category(
            "compose",
            "0xFF336699",
            TokenCategory::Color,
            StyleContext::Color,
            vec![token(
                "color.primary",
                TokenCategory::Color,
                Some("0xFF336699"),
            )],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Exact);
    }

    #[test]
    fn classification_numeric_color_is_never_near() {
        let row = classify_category(
            "compose",
            "254",
            TokenCategory::Color,
            StyleContext::Color,
            vec![token("color.primary", TokenCategory::Color, Some("255"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_numeric_elevation_is_never_near() {
        let row = classify_category(
            "swift",
            "3",
            TokenCategory::Elevation,
            StyleContext::Elevation,
            vec![token("elevation.low", TokenCategory::Elevation, Some("4"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_react_composite_shadow_is_exact_text() {
        let row = classify_category(
            "react",
            "\"0 1px 2px #000\"",
            TokenCategory::Elevation,
            StyleContext::Elevation,
            vec![token(
                "elevation.low",
                TokenCategory::Elevation,
                Some("0 1px 2px #000"),
            )],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Exact);
    }

    #[test]
    fn classification_react_composite_shadow_does_not_collapse_to_scalar() {
        let row = classify_category(
            "react",
            "\"0 0 0\"",
            TokenCategory::Elevation,
            StyleContext::Elevation,
            vec![token("elevation.zero", TokenCategory::Elevation, Some("0"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_react_unknown_unitless_value_does_not_assume_pixels() {
        let row = classify(
            "react",
            "4",
            StyleContext::Unknown,
            vec![token("spacing.s", TokenCategory::Spacing, Some("4px"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_swift_named_color_preserves_case() {
        let row = classify_category(
            "swift",
            "\"Brand\"",
            TokenCategory::Color,
            StyleContext::Color,
            vec![token("color.brand", TokenCategory::Color, Some("brand"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_swift_three_letter_named_color_is_not_hex() {
        let row = classify_category(
            "swift",
            "\"Bad\"",
            TokenCategory::Color,
            StyleContext::Color,
            vec![token("color.bad", TokenCategory::Color, Some("bad"))],
            2.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_distinct_sub_epsilon_values_are_not_exact() {
        let row = classify(
            "swift",
            "0",
            StyleContext::Gap,
            vec![token(
                "spacing.tiny",
                TokenCategory::Spacing,
                Some("0.00000000000000000001"),
            )],
            0.0,
        );

        assert_eq!(row.classification, TokenInferenceClassification::Unmatched);
    }

    #[test]
    fn classification_near_ties_require_identical_distances() {
        let row = classify(
            "swift",
            "0",
            StyleContext::Gap,
            vec![
                token("spacing.negative", TokenCategory::Spacing, Some("-1")),
                token("spacing.positive", TokenCategory::Spacing, Some("1")),
                token(
                    "spacing.almost",
                    TokenCategory::Spacing,
                    Some("1.0000000000000002"),
                ),
            ],
            2.0,
        );

        assert_eq!(
            row.suggestions
                .iter()
                .map(|suggestion| suggestion.token_id.as_str())
                .collect::<Vec<_>>(),
            vec!["spacing.negative", "spacing.positive"]
        );
    }

    #[test]
    fn inference_sorts_sources_and_counts_candidate_metadata() {
        let language_id = language("react");
        let late = HardcodedStyleSite {
            id: "hardcoded.react:z.tsx:2:1:spacing".into(),
            location: SourceLocation {
                file: "z.tsx".into(),
                line: 2,
                column: Some(1),
            },
            value: "4".into(),
            category: TokenCategory::Spacing,
            context: StyleContext::Width,
            parent: None,
        };
        let early = HardcodedStyleSite {
            id: "hardcoded.react:a.tsx:10:1:color".into(),
            location: SourceLocation {
                file: "a.tsx".into(),
                line: 10,
                column: Some(1),
            },
            value: "\"#FFF\"".into(),
            category: TokenCategory::Color,
            context: StyleContext::Color,
            parent: None,
        };
        let facts = facts(
            &language_id,
            vec![
                token("spacing.s", TokenCategory::Spacing, Some("4px")),
                token("color.primary", TokenCategory::Color, Some("#fff")),
            ],
            vec![late, early],
        );
        let report = build_token_inference(
            &BTreeMap::from([(language_id, facts)]),
            &TokenInferenceConfig::default(),
        )
        .unwrap();

        assert_eq!(
            (
                report
                    .sites
                    .iter()
                    .map(|site| site.site_id.as_str())
                    .collect::<Vec<_>>(),
                report.counts.candidates_by_category.color,
                report.counts.candidates_by_category.spacing,
                report.counts.candidates_by_context.color,
                report.counts.candidates_by_context.width,
            ),
            (
                vec![
                    "hardcoded.react:a.tsx:10:1:color",
                    "hardcoded.react:z.tsx:2:1:spacing",
                ],
                1,
                1,
                1,
                1,
            )
        );
    }
}
