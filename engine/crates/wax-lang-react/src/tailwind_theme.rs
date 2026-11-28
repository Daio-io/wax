//! Tailwind v4 theme declarations and utility namespaces.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::path::{Path, PathBuf};

use tree_sitter::Node;
use wax_contract::{
    Diagnostic, DiagnosticSeverity, RegistryDocument, RegistryExtractionStatus,
    RegistrySourceEvidence, RegistryToken, StyleContext, TokenCategory, is_css_custom_property,
};
use wax_lang_api::{
    DISCOVERED_LITERAL_VALUE, TokenMatch, discovered_token_marker, refresh_discovered_token_value,
};

use crate::css_token_extract::{css_parser, first_error, source_location};
use crate::diagnostics::PARSE_FAILED;
use crate::discover::{ReactDiscoverError, repo_relative_path};
use crate::palette_discovery::{apply_token_evidence, invalid_token};
use crate::swc_parse::ReactParseError;

#[derive(Clone, Copy)]
pub(crate) struct ThemeNamespace {
    pub prefix: &'static str,
    pub category: TokenCategory,
}

const NAMESPACES: &[(&str, TokenCategory)] = {
    use TokenCategory::{AspectRatio, Color, Elevation, Radius, Spacing, Typography, Unknown};
    &[
        ("--color-", Color),
        ("--spacing-", Spacing),
        ("--container-", Spacing),
        ("--radius-", Radius),
        ("--shadow-", Elevation),
        ("--inset-shadow-", Elevation),
        ("--drop-shadow-", Elevation),
        ("--text-shadow-", Elevation),
        ("--text-", Typography),
        ("--font-", Typography),
        ("--font-weight-", Typography),
        ("--tracking-", Typography),
        ("--leading-", Typography),
        ("--blur-", Unknown),
        ("--perspective-", Unknown),
        ("--aspect-", AspectRatio),
        ("--ease-", Unknown),
        ("--animate-", Unknown),
        ("--breakpoint-", Unknown),
    ]
};

/// Tailwind v4 theme namespace for a custom property, by longest prefix.
pub(crate) fn theme_namespace(property: &str) -> Option<ThemeNamespace> {
    if property == "--spacing" {
        return Some(ThemeNamespace {
            prefix: "--spacing",
            category: TokenCategory::Spacing,
        });
    }
    if !is_css_custom_property(property) || property[2..].contains("--") {
        return None;
    }
    let namespace = NAMESPACES
        .iter()
        .filter(|(prefix, _)| property.starts_with(prefix))
        .map(|&(prefix, category)| ThemeNamespace { prefix, category })
        .max_by_key(|namespace| namespace.prefix.len())?;
    (property.len() > namespace.prefix.len()).then_some(namespace)
}

const UTILITIES: &[(&str, &[&str])] = &[
    ("bg", &["color"]),
    ("border", &["color"]),
    ("border-x", &["color"]),
    ("border-y", &["color"]),
    ("border-t", &["color"]),
    ("border-r", &["color"]),
    ("border-b", &["color"]),
    ("border-l", &["color"]),
    ("border-s", &["color"]),
    ("border-e", &["color"]),
    ("outline", &["color"]),
    ("ring", &["color"]),
    ("ring-offset", &["color"]),
    ("inset-ring", &["color"]),
    ("divide", &["color"]),
    ("fill", &["color"]),
    ("stroke", &["color"]),
    ("decoration", &["color"]),
    ("accent", &["color"]),
    ("caret", &["color"]),
    ("placeholder", &["color"]),
    ("from", &["color"]),
    ("via", &["color"]),
    ("to", &["color"]),
    ("text", &["text", "color"]),
    ("shadow", &["shadow", "color"]),
    ("inset-shadow", &["inset-shadow", "color"]),
    ("drop-shadow", &["drop-shadow", "color"]),
    ("text-shadow", &["text-shadow", "color"]),
    ("font", &["font-weight", "font"]),
    ("tracking", &["tracking"]),
    ("leading", &["leading"]),
    ("rounded", &["radius"]),
    ("rounded-t", &["radius"]),
    ("rounded-r", &["radius"]),
    ("rounded-b", &["radius"]),
    ("rounded-l", &["radius"]),
    ("rounded-s", &["radius"]),
    ("rounded-e", &["radius"]),
    ("rounded-tl", &["radius"]),
    ("rounded-tr", &["radius"]),
    ("rounded-br", &["radius"]),
    ("rounded-bl", &["radius"]),
    ("rounded-ss", &["radius"]),
    ("rounded-se", &["radius"]),
    ("rounded-es", &["radius"]),
    ("rounded-ee", &["radius"]),
    ("p", &["spacing"]),
    ("px", &["spacing"]),
    ("py", &["spacing"]),
    ("pt", &["spacing"]),
    ("pr", &["spacing"]),
    ("pb", &["spacing"]),
    ("pl", &["spacing"]),
    ("ps", &["spacing"]),
    ("pe", &["spacing"]),
    ("m", &["spacing"]),
    ("mx", &["spacing"]),
    ("my", &["spacing"]),
    ("mt", &["spacing"]),
    ("mr", &["spacing"]),
    ("mb", &["spacing"]),
    ("ml", &["spacing"]),
    ("ms", &["spacing"]),
    ("me", &["spacing"]),
    ("gap", &["spacing"]),
    ("gap-x", &["spacing"]),
    ("gap-y", &["spacing"]),
    ("space-x", &["spacing"]),
    ("space-y", &["spacing"]),
    ("h", &["spacing"]),
    ("min-h", &["spacing"]),
    ("max-h", &["spacing"]),
    ("size", &["spacing"]),
    ("inset", &["spacing"]),
    ("inset-x", &["spacing"]),
    ("inset-y", &["spacing"]),
    ("top", &["spacing"]),
    ("right", &["spacing"]),
    ("bottom", &["spacing"]),
    ("left", &["spacing"]),
    ("start", &["spacing"]),
    ("end", &["spacing"]),
    ("translate-x", &["spacing"]),
    ("translate-y", &["spacing"]),
    ("scroll-m", &["spacing"]),
    ("scroll-mx", &["spacing"]),
    ("scroll-my", &["spacing"]),
    ("scroll-mt", &["spacing"]),
    ("scroll-mr", &["spacing"]),
    ("scroll-mb", &["spacing"]),
    ("scroll-ml", &["spacing"]),
    ("scroll-ms", &["spacing"]),
    ("scroll-me", &["spacing"]),
    ("scroll-p", &["spacing"]),
    ("scroll-px", &["spacing"]),
    ("scroll-py", &["spacing"]),
    ("scroll-pt", &["spacing"]),
    ("scroll-pr", &["spacing"]),
    ("scroll-pb", &["spacing"]),
    ("scroll-pl", &["spacing"]),
    ("scroll-ps", &["spacing"]),
    ("scroll-pe", &["spacing"]),
    ("indent", &["spacing"]),
    ("basis", &["spacing"]),
    ("w", &["container", "spacing"]),
    ("min-w", &["container", "spacing"]),
    ("max-w", &["container", "spacing"]),
    ("blur", &["blur"]),
    ("perspective", &["perspective"]),
    ("aspect", &["aspect"]),
    ("ease", &["ease"]),
    ("animate", &["animate"]),
];

#[derive(Clone, Copy)]
enum ArbitraryShape {
    Color,
    Length,
    Other,
}

fn arbitrary_metadata(
    prefix: &str,
    shape: ArbitraryShape,
    value: &str,
) -> Option<(TokenCategory, StyleContext)> {
    use ArbitraryShape::{Color, Length, Other};
    use StyleContext as Context;
    use TokenCategory as Category;

    Some(match (prefix, shape) {
        (
            "bg" | "border" | "border-x" | "border-y" | "border-t" | "border-r" | "border-b"
            | "border-l" | "border-s" | "border-e" | "outline" | "ring" | "ring-offset"
            | "inset-ring" | "divide" | "fill" | "stroke" | "decoration" | "accent" | "caret"
            | "placeholder" | "from" | "via" | "to" | "text" | "shadow" | "inset-shadow"
            | "drop-shadow" | "text-shadow",
            Color,
        ) => (Category::Color, Context::Color),
        ("text" | "leading" | "tracking", Length) => (Category::Typography, Context::Typography),
        ("font", Length) if is_finite_number(value) => (Category::Typography, Context::Typography),
        (
            "p" | "px" | "py" | "pt" | "pr" | "pb" | "pl" | "ps" | "pe" | "scroll-p" | "scroll-px"
            | "scroll-py" | "scroll-pt" | "scroll-pr" | "scroll-pb" | "scroll-pl" | "scroll-ps"
            | "scroll-pe",
            Length,
        ) => (Category::Spacing, Context::Padding),
        (
            "m" | "mx" | "my" | "mt" | "mr" | "mb" | "ml" | "ms" | "me" | "scroll-m" | "scroll-mx"
            | "scroll-my" | "scroll-mt" | "scroll-mr" | "scroll-mb" | "scroll-ml" | "scroll-ms"
            | "scroll-me",
            Length,
        ) => (Category::Spacing, Context::Margin),
        ("gap" | "gap-x" | "gap-y" | "space-x" | "space-y", Length) => {
            (Category::Spacing, Context::Gap)
        }
        ("aspect", Length | Other) => (Category::AspectRatio, Context::AspectRatio),
        ("w" | "min-w" | "max-w", Length) => (Category::Spacing, Context::Width),
        ("h" | "min-h" | "max-h", Length) => (Category::Spacing, Context::Height),
        ("size", Length) => (Category::Spacing, Context::Size),
        (
            "inset" | "inset-x" | "inset-y" | "top" | "right" | "bottom" | "left" | "start" | "end"
            | "translate-x" | "translate-y" | "indent" | "basis",
            Length,
        ) => (Category::Spacing, Context::Unknown),
        (
            "rounded" | "rounded-t" | "rounded-r" | "rounded-b" | "rounded-l" | "rounded-s"
            | "rounded-e" | "rounded-tl" | "rounded-tr" | "rounded-br" | "rounded-bl"
            | "rounded-ss" | "rounded-se" | "rounded-es" | "rounded-ee",
            Length,
        ) => (Category::Radius, Context::Radius),
        ("shadow" | "inset-shadow" | "drop-shadow" | "text-shadow", Length | Other) => {
            (Category::Elevation, Context::Elevation)
        }
        _ => return None,
    })
}

/// Decodes a literal arbitrary value for utilities with a hard-coded style context.
pub(crate) fn arbitrary_style(utility: &str) -> Option<(String, TokenCategory, StyleContext)> {
    let (prefix, raw) = utility.split_once("-[")?;
    let raw = raw.strip_suffix(']')?;
    let (negative, prefix) = match prefix.strip_prefix('-') {
        Some(prefix) => (true, prefix),
        None => (false, prefix),
    };
    let lowercase = raw.to_ascii_lowercase();
    if ["var(", "theme(", "env(", "calc(", "url(", "--"]
        .iter()
        .any(|pattern| lowercase.contains(pattern))
    {
        return None;
    }

    let hint = lowercase.split_once(':').filter(|(hint, _)| {
        hint.starts_with(|ch: char| ch.is_ascii_alphabetic())
            && hint
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    });
    let (shape, raw) = match hint {
        Some((hint, _)) => (
            match hint {
                "color" => ArbitraryShape::Color,
                "length" | "number" | "percentage" => ArbitraryShape::Length,
                _ => ArbitraryShape::Other,
            },
            &raw[hint.len() + 1..],
        ),
        None => (infer_arbitrary_shape(&lowercase), raw),
    };
    if raw.is_empty() {
        return None;
    }
    let (category, context) = arbitrary_metadata(prefix, shape, raw)?;

    let mut value = String::with_capacity(raw.len() + usize::from(negative));
    if negative {
        value.push('-');
    }
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.next_if_eq(&'_').is_some() => value.push('_'),
            '_' => value.push(' '),
            _ => value.push(ch),
        }
    }
    if category == TokenCategory::AspectRatio
        && wax_lang_api::aspect_ratio::normalize(&value).is_none()
    {
        return None;
    }
    Some((value, category, context))
}

fn infer_arbitrary_shape(value: &str) -> ArbitraryShape {
    if value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) || value.split_once('(').is_some_and(|(name, _)| {
        matches!(
            name,
            "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color"
        )
    }) {
        return ArbitraryShape::Color;
    }

    const CSS_LENGTH_UNITS: &[&str] = &[
        "%", "px", "cm", "mm", "q", "in", "pc", "pt", "cap", "ch", "em", "ex", "ic", "lh", "rcap",
        "rch", "rem", "rex", "ric", "rlh", "vw", "vh", "vi", "vb", "vmin", "vmax", "svw", "svh",
        "svi", "svb", "svmin", "svmax", "lvw", "lvh", "lvi", "lvb", "lvmin", "lvmax", "dvw", "dvh",
        "dvi", "dvb", "dvmin", "dvmax", "cqw", "cqh", "cqi", "cqb", "cqmin", "cqmax",
    ];
    let number = value.trim_end_matches(|ch: char| ch.is_ascii_lowercase() || ch == '%');
    if is_finite_number(number)
        && (number.len() == value.len() || CSS_LENGTH_UNITS.contains(&&value[number.len()..]))
    {
        ArbitraryShape::Length
    } else {
        ArbitraryShape::Other
    }
}

fn is_finite_number(value: &str) -> bool {
    value.parse::<f64>().is_ok_and(f64::is_finite)
}

/// Registered tokens a Tailwind utility and its optional `/` modifier reference.
pub(crate) fn utility_tokens<'a>(
    utility: &str,
    modifier: Option<&str>,
    css_tokens: &'a BTreeMap<String, TokenMatch>,
) -> Vec<&'a TokenMatch> {
    if css_tokens.is_empty() {
        return Vec::new();
    }
    let Some((prefix, value, namespaces)) = UTILITIES
        .iter()
        .filter_map(|&(prefix, namespaces)| {
            let value = utility.strip_prefix(prefix)?.strip_prefix('-')?;
            (!value.is_empty()).then_some((prefix, value, namespaces))
        })
        .max_by_key(|(prefix, _, _)| prefix.len())
    else {
        return Vec::new();
    };
    let is_number = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    if namespaces.contains(&"spacing") && modifier.is_some_and(is_number) && is_number(value) {
        // `w-1/2` is the fraction `1/2`, not `--spacing-1` with a modifier.
        return Vec::new();
    }
    let (namespace, base) = if value.starts_with(['[', '(']) {
        (
            None,
            arbitrary_property(value).and_then(|property| css_tokens.get(property)),
        )
    } else {
        // An explicitly registered numeric token such as `--spacing-4` wins;
        // deriving `p-4` from the base `--spacing` scale is out of scope.
        namespaces
            .iter()
            .find_map(|&namespace| {
                css_tokens
                    .get(&format!("--{namespace}-{value}"))
                    .map(|token| (Some(namespace), Some(token)))
            })
            .unwrap_or((None, None))
    };
    let modifier = modifier.and_then(|modifier| {
        if modifier.starts_with(['[', '(']) {
            css_tokens.get(arbitrary_property(modifier)?)
        } else if prefix == "text" && namespace != Some("color") && !value.starts_with(['[', '(']) {
            // A named font-size modifier selects a line height.
            css_tokens.get(&format!("--leading-{modifier}"))
        } else {
            None
        }
    });
    let mut tokens = Vec::with_capacity(2);
    tokens.extend(base);
    tokens.extend(
        modifier.filter(|modifier| base.is_none_or(|base| base.token_id != modifier.token_id)),
    );
    tokens
}

fn arbitrary_property(value: &str) -> Option<&str> {
    let property = if let Some(inner) = value.strip_prefix('(') {
        without_type_hint(inner.strip_suffix(')')?)
    } else {
        let inner = value.strip_prefix('[')?.strip_suffix(']')?;
        let call = without_type_hint(inner).strip_prefix("var(")?;
        let close = top_level_delimiters(call)
            .find(|&(_, byte)| byte == b')')?
            .0;
        if close + 1 != call.len() {
            return None;
        }
        call[..close].split(',').next()?
    };
    is_css_custom_property(property).then_some(property)
}

fn without_type_hint(value: &str) -> &str {
    match value.split_once(':') {
        Some((hint, rest))
            if hint.starts_with(|ch: char| ch.is_ascii_alphabetic())
                && hint
                    .bytes()
                    .all(|byte| byte.is_ascii_alphabetic() || byte == b'-') =>
        {
            rest
        }
        _ => value,
    }
}

/// Finds delimiters outside bracketed, parenthesized, and quoted content.
pub(crate) fn top_level_delimiters(value: &str) -> impl Iterator<Item = (usize, u8)> + '_ {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    value.bytes().enumerate().filter(move |&(_, byte)| {
        if escaped {
            escaped = false;
            return false;
        }
        if byte == b'\\' {
            escaped = true;
            return false;
        }
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
            return false;
        }

        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'[' | b'(' => depth += 1,
            b']' | b')' if depth > 0 => depth -= 1,
            b':' | b'/' | b')' => return depth == 0,
            _ => {}
        }
        false
    })
}

pub(crate) fn discover_theme_tokens(
    document: &mut RegistryDocument,
    seeds: &[RegistryToken],
    repo_root: &Path,
    css_files: &[PathBuf],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ReactDiscoverError> {
    let parser_error = |error| ReactDiscoverError::Io {
        context: "parse React discovery CSS".to_owned(),
        source: std::io::Error::other(error),
    };
    let mut parser = css_parser().map_err(parser_error)?;
    let mut roots = BTreeMap::new();
    let mut themes = BTreeMap::new();
    let mut damaged = BTreeSet::new();
    let diagnostic_start = diagnostics.len();
    let mut has_theme = false;
    for file in css_files {
        let source = std::fs::read_to_string(file).map_err(|source| ReactDiscoverError::Io {
            context: format!("read CSS source file {}", file.display()),
            source,
        })?;
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| parser_error(ReactParseError::CssNoTree))?;
        let path = repo_relative_path(repo_root, file);
        let root = tree.root_node();
        if let Some(error) = first_error(root, |error| is_theme_reset(error, &source)) {
            let point = error.start_position();
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                code: PARSE_FAILED.to_owned(),
                message: "failed to parse CSS file cleanly; affected declarations skipped"
                    .to_owned(),
                location: Some(source_location(&path, point)),
            });
            damaged.insert(path.clone());
        }
        for rule in root.named_children(&mut root.walk()) {
            let is_theme = is_theme_rule(rule, &source);
            has_theme |= is_theme
                || (rule.is_error()
                    && rule.named_children(&mut rule.walk()).any(|child| {
                        child.kind() == "at_keyword" && &source[child.byte_range()] == "@theme"
                    }));
            let Some(block) = rule
                .named_children(&mut rule.walk())
                .find(|node| node.kind() == "block")
            else {
                continue;
            };
            let is_root = rule.kind() == "rule_set"
                && rule
                    .named_child(0)
                    .is_some_and(|selector| &source[selector.byte_range()] == ":root");
            if !is_root && !is_theme {
                continue;
            }
            for node in block.named_children(&mut block.walk()) {
                let Some((property, value)) = declaration(node, &source) else {
                    continue;
                };
                if is_root && is_css_custom_property(property) {
                    roots.insert(property.to_owned(), value);
                } else if is_theme && let Some(namespace) = theme_namespace(property) {
                    match themes.entry(property.to_owned()) {
                        Entry::Vacant(entry) => {
                            let line = |row| u32::try_from(row + 1).unwrap_or(u32::MAX);
                            let evidence = RegistrySourceEvidence {
                                path: path.clone(),
                                start_line: Some(line(node.start_position().row)),
                                end_line: Some(line(node.end_position().row)),
                                extra: Default::default(),
                            };
                            entry.insert((value, namespace.category, evidence));
                        }
                        Entry::Occupied(entry) if entry.get().0.text != value.text => {
                            diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Warning,
                                code: "tailwind_theme_duplicate".to_owned(),
                                message: format!("conflicting Tailwind theme declarations for '{property}'; keeping the first"),
                                location: Some(source_location(&path, node.start_position())),
                            });
                        }
                        Entry::Occupied(_) => {}
                    }
                }
            }
        }
    }
    let mut matched = BTreeMap::new();
    for (index, seed) in seeds
        .iter()
        .enumerate()
        .filter(|(_, seed)| seed.package.is_none())
    {
        let Some(property) = wax_contract::css_custom_property_from_key(&seed.key) else {
            continue;
        };
        if themes.contains_key(property) {
            if matched.insert(property, index).is_some() {
                return Err(invalid_token(format!(
                    "multiple seed tokens match var({property})"
                )));
            }
        } else if seed.id.starts_with("css.")
            && let Some(prior) = &seed.details
            && prior.source_evidence.is_some()
        {
            has_theme = true;
            let token = &mut document.tokens[index];
            let mut details = prior.clone();
            details.status = if prior
                .source_evidence
                .as_ref()
                .is_some_and(|evidence| damaged.contains(&evidence.path))
            {
                RegistryExtractionStatus::Stale
            } else {
                RegistryExtractionStatus::NotObserved
            };
            refresh_discovered_token_value(
                &mut token.value,
                discovered_token_marker(Some(prior)),
                Some(&mut details),
                None,
            );
            token.details = Some(details);
        }
    }

    if !has_theme {
        // Non-Tailwind stylesheets had no effect on registry discovery.
        diagnostics.truncate(diagnostic_start);
    }

    for (property, (value, category, evidence)) in &themes {
        let resolved = resolve_value(value, &roots, &themes);
        if let Some(&index) = matched.get(property.as_str()) {
            let token = &mut document.tokens[index];
            apply_token_evidence(token, evidence.clone());
            if token.css_variants.is_empty() {
                refresh_discovered_token_value(
                    &mut token.value,
                    discovered_token_marker(seeds[index].details.as_ref()),
                    token.details.as_mut(),
                    Some(resolved),
                );
            } else {
                token.value = None;
                if let Some(details) = &mut token.details {
                    details.extra.remove(DISCOVERED_LITERAL_VALUE);
                }
            }
            continue;
        }
        let mut token = RegistryToken {
            id: format!("css.{}", &property[2..]),
            key: format!("var({property})"),
            package: None,
            category: *category,
            aliases: Vec::new(),
            value: Some(resolved.to_owned()),
            css_variants: Vec::new(),
            metadata: None,
            details: None,
            extra: Default::default(),
        };
        apply_token_evidence(&mut token, evidence.clone());
        if let Some(details) = &mut token.details {
            details
                .extra
                .insert(DISCOVERED_LITERAL_VALUE.to_owned(), resolved.into());
        }
        document.tokens.push(token);
    }
    Ok(())
}

fn is_theme_rule(rule: Node<'_>, source: &str) -> bool {
    if rule.kind() != "at_rule" {
        return false;
    }
    let mut cursor = rule.walk();
    let mut header = rule
        .named_children(&mut cursor)
        .filter(|node| !matches!(node.kind(), "block" | "comment"))
        .map(|node| &source[node.byte_range()]);
    header.next() == Some("@theme")
        && matches!(
            header.next(),
            None | Some("inline" | "static" | "default" | "reference")
        )
        && header.next().is_none()
}

fn is_theme_reset(error: Node<'_>, source: &str) -> bool {
    // tree-sitter-css reads the wildcard in Tailwind's reset syntax as an ERROR.
    error.is_error()
        && &source[error.byte_range()] == "*"
        && error.parent().is_some_and(|declaration| {
            declaration.kind() == "declaration"
                && declaration
                    .parent()
                    .and_then(|block| block.parent())
                    .is_some_and(|rule| is_theme_rule(rule, source))
                && source[declaration.start_byte()..error.start_byte()].starts_with("--")
                && source[error.end_byte()..declaration.end_byte()]
                    .trim()
                    .strip_prefix(':')
                    .is_some_and(|value| value.trim_end_matches(';').trim() == "initial")
        })
}

struct CssValue {
    text: String,
    reference: Option<VarReference>,
}

struct VarReference {
    property: String,
    fallback: Option<String>,
}

type ThemeDeclarations = BTreeMap<String, (CssValue, TokenCategory, RegistrySourceEvidence)>;

fn resolve_value<'a>(
    initial: &'a CssValue,
    roots: &'a BTreeMap<String, CssValue>,
    themes: &'a ThemeDeclarations,
) -> &'a str {
    let mut current = initial;
    let mut visited = Vec::new();
    for _ in 0..8 {
        let Some(reference) = &current.reference else {
            break;
        };
        let property = reference.property.as_str();
        if visited.contains(&property) {
            return &initial.text;
        }
        visited.push(property);
        let Some(next) = roots
            .get(property)
            .or_else(|| themes.get(property).map(|entry| &entry.0))
        else {
            return reference.fallback.as_deref().unwrap_or(&current.text);
        };
        current = next;
    }
    &current.text
}

fn declaration<'a>(node: Node<'_>, source: &'a str) -> Option<(&'a str, CssValue)> {
    if node.kind() != "declaration" || node.has_error() {
        return None;
    }
    let property = node.named_child(0)?;
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    let colon = children.find(|child| child.kind() == ":")?;
    let mut values = children
        .by_ref()
        .filter(|child| child.kind() != "comment")
        .peekable();
    let first = values.next()?;
    let reference = match values.peek().map(Node::kind) {
        None | Some("important" | ";") => var_reference(first, source),
        _ => None,
    };
    let end = node
        .children(&mut node.walk())
        .find(|child| matches!(child.kind(), "important" | ";"))
        .map_or(node.end_byte(), |child| child.start_byte());
    let text = source[colon.end_byte()..end].trim().to_owned();
    Some((&source[property.byte_range()], CssValue { text, reference }))
}

fn var_reference(call: Node<'_>, source: &str) -> Option<VarReference> {
    if call.kind() != "call_expression"
        || !source[call.named_child(0)?.byte_range()].eq_ignore_ascii_case("var")
    {
        return None;
    }
    let args = call.named_child(1)?;
    let mut cursor = args.walk();
    let mut args_children = args
        .children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "(" | ")" | "comment"));
    let property = &source[args_children.next()?.byte_range()];
    if !is_css_custom_property(property) {
        return None;
    }
    let fallback = match args_children.next() {
        None => None,
        Some(comma) if comma.kind() == "," => Some(
            source[comma.end_byte()..args.end_byte() - 1]
                .trim()
                .to_owned(),
        ),
        Some(_) => return None,
    };
    Some(VarReference {
        property: property.to_owned(),
        fallback,
    })
}
