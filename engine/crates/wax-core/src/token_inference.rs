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
    Numeric { scalar: f64, unit: Option<String> },
    ExactText(String),
    Unsupported,
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
    let mut sites = Vec::new();

    for (language_id, facts) in languages {
        for site in &facts.hardcoded_style_sites {
            sites.push(infer_site(
                language_id,
                site,
                &facts.design_system_tokens,
                config.numeric_tolerance,
            ));
        }
    }

    sites.sort_by(|left, right| {
        let left_site = lookup_site(languages, left);
        let right_site = lookup_site(languages, right);
        (
            left.language.as_str(),
            left_site
                .map(|site| site.location.file.as_str())
                .unwrap_or(""),
            left_site.map(|site| site.location.line).unwrap_or(0),
            left_site.and_then(|site| site.location.column).unwrap_or(0),
            left.site_id.as_str(),
        )
            .cmp(&(
                right.language.as_str(),
                right_site
                    .map(|site| site.location.file.as_str())
                    .unwrap_or(""),
                right_site.map(|site| site.location.line).unwrap_or(0),
                right_site
                    .and_then(|site| site.location.column)
                    .unwrap_or(0),
                right.site_id.as_str(),
            ))
    });

    let counts = derive_counts(languages, &sites)?;
    Ok(TokenInferenceReport {
        numeric_tolerance: config.numeric_tolerance,
        counts,
        sites,
    })
}

fn lookup_site<'a>(
    languages: &'a BTreeMap<LanguageId, ScanFacts>,
    row: &HardcodedStyleInference,
) -> Option<&'a HardcodedStyleSite> {
    languages
        .get(&row.language)?
        .hardcoded_style_sites
        .iter()
        .find(|site| site.id == row.site_id)
}

fn derive_counts(
    languages: &BTreeMap<LanguageId, ScanFacts>,
    sites: &[HardcodedStyleInference],
) -> Result<TokenInferenceCounts, ScanFactsError> {
    let hardcoded_observation_count =
        u32::try_from(sites.len()).map_err(|_| ScanFactsError::ContractViolation {
            field: "token_inference.counts.hardcoded_observation_count".to_owned(),
            message: "hardcoded observation count exceeds u32 maximum".to_owned(),
        })?;
    let mut counts = TokenInferenceCounts {
        hardcoded_observation_count,
        ..TokenInferenceCounts::default()
    };

    for row in sites {
        match row.classification {
            TokenInferenceClassification::Exact => {
                counts.exact_replacement_candidate_count =
                    counts.exact_replacement_candidate_count.saturating_add(1);
                bump_candidate_breakdown(languages, row, &mut counts);
            }
            TokenInferenceClassification::Near => {
                counts.near_replacement_candidate_count =
                    counts.near_replacement_candidate_count.saturating_add(1);
                bump_candidate_breakdown(languages, row, &mut counts);
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

fn bump_candidate_breakdown(
    languages: &BTreeMap<LanguageId, ScanFacts>,
    row: &HardcodedStyleInference,
    counts: &mut TokenInferenceCounts,
) {
    match row.confidence {
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

    if let Some(site) = lookup_site(languages, row) {
        bump_category(&mut counts.candidates_by_category, site.category);
        bump_context(&mut counts.candidates_by_context, site.context);
    }
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
    let mut incomplete_coverage = false;
    let mut unsupported_canonical = false;

    for token in &same_category {
        match &token.value {
            None => {
                missing_canonical = true;
                incomplete_coverage = true;
            }
            Some(value) => {
                let normalized =
                    normalize_canonical(language_id, site.category, site.context, value);
                match normalized {
                    NormalizedValue::Unsupported => {
                        unsupported_canonical = true;
                        incomplete_coverage = true;
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

    if incomplete_coverage {
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

    if !matches!(observed, NormalizedValue::Numeric { .. }) {
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
            .partial_cmp(&right.3)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.id.cmp(&right.0.id))
    });
    let best_distance = near_candidates[0].3;
    near_candidates.retain(|candidate| (candidate.3 - best_distance).abs() <= f64::EPSILON);
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
            },
            NormalizedValue::Numeric {
                scalar: right_scalar,
                unit: right_unit,
            },
        ) => left_unit == right_unit && (left_scalar - right_scalar).abs() <= f64::EPSILON,
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
            },
            NormalizedValue::Numeric {
                scalar: right_scalar,
                unit: right_unit,
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

    match language.as_str() {
        "compose" => normalize_compose(trimmed),
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

fn normalize_compose(value: &str) -> NormalizedValue {
    if let Some(normalized) = parse_compose_numeric(value) {
        return normalized;
    }
    if looks_like_hex_color(value) {
        return NormalizedValue::ExactText(normalize_hex_color(value));
    }
    NormalizedValue::ExactText(value.to_ascii_lowercase())
}

fn parse_compose_numeric(value: &str) -> Option<NormalizedValue> {
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let split_at = compact
        .char_indices()
        .find(|&(_, ch)| ch.is_ascii_alphabetic())
        .map(|(index, _)| index);
    match split_at {
        Some(index) => {
            let (number, unit) = compact.split_at(index);
            let scalar = number.parse::<f64>().ok()?;
            if !scalar.is_finite() {
                return Some(NormalizedValue::Unsupported);
            }
            let unit = unit.to_ascii_lowercase();
            if unit != "dp" && unit != "sp" {
                return Some(NormalizedValue::Unsupported);
            }
            Some(NormalizedValue::Numeric {
                scalar,
                unit: Some(unit),
            })
        }
        None => {
            let scalar = compact.parse::<f64>().ok()?;
            if !scalar.is_finite() {
                return Some(NormalizedValue::Unsupported);
            }
            Some(NormalizedValue::Numeric { scalar, unit: None })
        }
    }
}

fn normalize_react(category: TokenCategory, context: StyleContext, value: &str) -> NormalizedValue {
    if category == TokenCategory::Color
        || context == StyleContext::Color
        || looks_like_hex_color(value)
    {
        if looks_like_hex_color(value) {
            return NormalizedValue::ExactText(normalize_hex_color(value));
        }
        return NormalizedValue::ExactText(value.to_ascii_lowercase());
    }

    if let Some(normalized) = parse_css_length(value, allow_unitless_px(context)) {
        return normalized;
    }

    NormalizedValue::ExactText(value.to_ascii_lowercase())
}

fn normalize_swift(category: TokenCategory, context: StyleContext, value: &str) -> NormalizedValue {
    if category == TokenCategory::Color
        || context == StyleContext::Color
        || looks_like_hex_color(value)
    {
        if looks_like_hex_color(value) {
            return NormalizedValue::ExactText(normalize_hex_color(value));
        }
        return NormalizedValue::ExactText(value.to_ascii_lowercase());
    }

    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Ok(scalar) = compact.parse::<f64>() {
        if scalar.is_finite() {
            return NormalizedValue::Numeric { scalar, unit: None };
        }
        return NormalizedValue::Unsupported;
    }

    NormalizedValue::ExactText(value.to_ascii_lowercase())
}

fn normalize_generic(
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue {
    if category == TokenCategory::Color
        || context == StyleContext::Color
        || looks_like_hex_color(value)
    {
        if looks_like_hex_color(value) {
            return NormalizedValue::ExactText(normalize_hex_color(value));
        }
        return NormalizedValue::ExactText(value.to_ascii_lowercase());
    }
    if let Some(normalized) = parse_css_length(value, allow_unitless_px(context)) {
        return normalized;
    }
    NormalizedValue::ExactText(value.to_ascii_lowercase())
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
            | StyleContext::Unknown
    )
}

fn parse_css_length(value: &str, unitless_as_px: bool) -> Option<NormalizedValue> {
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let split_at = compact
        .char_indices()
        .find(|&(_, ch)| ch.is_ascii_alphabetic() || ch == '%')
        .map(|(index, _)| index);

    match split_at {
        Some(index) => {
            let (number, unit) = compact.split_at(index);
            let scalar = number.parse::<f64>().ok()?;
            if !scalar.is_finite() {
                return Some(NormalizedValue::Unsupported);
            }
            let unit = unit.to_ascii_lowercase();
            match unit.as_str() {
                "px" => Some(NormalizedValue::Numeric {
                    scalar,
                    unit: Some("px".to_owned()),
                }),
                "rem" | "em" | "vh" | "vw" | "vmin" | "vmax" | "%" => {
                    // Keep incompatible unit families distinct; never convert.
                    Some(NormalizedValue::Numeric {
                        scalar,
                        unit: Some(unit),
                    })
                }
                _ => Some(NormalizedValue::Unsupported),
            }
        }
        None => {
            let scalar = compact.parse::<f64>().ok()?;
            if !scalar.is_finite() {
                return Some(NormalizedValue::Unsupported);
            }
            if unitless_as_px {
                Some(NormalizedValue::Numeric {
                    scalar,
                    unit: Some("px".to_owned()),
                })
            } else {
                Some(NormalizedValue::Numeric { scalar, unit: None })
            }
        }
    }
}

fn looks_like_hex_color(value: &str) -> bool {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn normalize_hex_color(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        format!("#{}", hex.to_ascii_lowercase())
    } else {
        format!("#{}", trimmed.to_ascii_lowercase())
    }
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
    fn normalize_react_color_is_case_insensitive_exact_without_distance() {
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
        assert!(matches!(observed, NormalizedValue::ExactText(_)));
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
        let (language_id, site) = site(language, value, TokenCategory::Spacing, context);
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
            hardcoded_style_sites: vec![site],
            token_usage_summary: Vec::new(),
        };
        facts.recompute_counts().unwrap();
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
    fn classification_incomplete_coverage_without_exact_is_unassessed() {
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
        assert_eq!(row.classification, TokenInferenceClassification::Unassessed);
        assert!(
            row.evidence
                .contains(&TokenInferenceEvidence::IncompleteCanonicalCoverage)
        );
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
}
