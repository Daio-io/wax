//! Recovery metadata for permissive Kotlin parsing.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

#[allow(dead_code)]
pub(crate) const MAX_RECOVERY_ATTEMPTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ByteRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl ByteRange {
    pub(crate) fn new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    #[allow(dead_code)]
    pub(crate) fn contains(self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }

    pub(crate) fn contains_node(self, node: tree_sitter::Node<'_>) -> bool {
        self.start <= node.start_byte() && node.end_byte() <= self.end
    }

    fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SyntaxFamily {
    SuspendLambda,
    WhenGuard,
    AnnotatedFunctionType,
    ExplicitBackingField,
    ContextParameter,
    ContextReceiver,
    AnnotatedTypeArgument,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentScopePolicy {
    Inherit,
    ComposableLambda,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxRegion {
    pub(crate) source: ByteRange,
    pub(crate) body: Option<ByteRange>,
    pub(crate) family: SyntaxFamily,
    pub(crate) component_scope: ComponentScopePolicy,
}

/// True when `node` sits in a known syntax region's source span but outside its body.
///
/// Type annotations and masked prefixes are excluded from token/style walks, while
/// preserved bodies (for example explicit-field initializers) remain extractable.
pub(crate) fn node_in_type_annotation_range(
    node: tree_sitter::Node<'_>,
    regions: &[SyntaxRegion],
) -> bool {
    regions.iter().any(|region| {
        region.source.contains_node(node)
            && !region.body.is_some_and(|body| body.contains_node(node))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxProblem {
    pub(crate) range: ByteRange,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) family: SyntaxFamily,
    pub(crate) recovered_later_source: bool,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ParsePass {
    pub(crate) tree: tree_sitter::Tree,
    pub(crate) clean: Vec<ByteRange>,
    pub(crate) priority: u16,
}

/// Additional parse passes recovered after broad syntax gaps.
#[derive(Debug)]
pub(crate) struct ParseRecovery {
    pub(crate) primary_clean: Vec<ByteRange>,
    pub(crate) recovered: Vec<ParsePass>,
    pub(crate) unresolved_problems: Vec<SyntaxProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedKotlinSource {
    pub(crate) bytes: Vec<u8>,
    pub(crate) regions: Vec<SyntaxRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelimiterKind {
    Paren,
    Brace,
    Bracket,
    Angle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BalancedDelimiter {
    open: usize,
    close: usize,
    kind: DelimiterKind,
}

#[derive(Debug)]
struct LexedKotlin<'a> {
    bytes: &'a [u8],
    token_starts: Vec<usize>,
    matching_delimiters: BTreeMap<usize, usize>,
    delimiters: Vec<BalancedDelimiter>,
    unbalanced_delimiters: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryTransform {
    source: ByteRange,
    body: Option<ByteRange>,
    family: SyntaxFamily,
    component_scope: ComponentScopePolicy,
    mask_ranges: Vec<ByteRange>,
}

pub(crate) fn merge_clean_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    ranges.sort();

    let mut merged: Vec<ByteRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.start <= previous.end => {
                previous.end = previous.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }

    merged
}

pub(crate) fn normalize_kotlin_for_parse(source: &str) -> NormalizedKotlinSource {
    let lexed = lex_kotlin(source);
    let mut transforms = Vec::new();
    collect_suspend_lambda_transforms(&lexed, &mut transforms);
    collect_when_guard_transforms(&lexed, &mut transforms);
    collect_annotated_function_type_transforms(&lexed, &mut transforms);
    collect_explicit_backing_field_transforms(&lexed, &mut transforms);
    collect_context_transforms(&lexed, &mut transforms);
    collect_annotated_type_argument_transforms(&lexed, &mut transforms);

    let mut selected = select_non_overlapping_transforms(transforms);
    let mut bytes = source.as_bytes().to_vec();

    selected.sort_by_key(|transform| Reverse(transform.source.start));
    for transform in &selected {
        for range in &transform.mask_ranges {
            mask_preserving_lines(&mut bytes, *range);
        }
    }

    let mut regions = selected
        .into_iter()
        .map(|transform| SyntaxRegion {
            source: transform.source,
            body: transform.body,
            family: transform.family,
            component_scope: transform.component_scope,
        })
        .collect::<Vec<_>>();
    regions.sort_by_key(|region| (region.source.start, region.source.end, region.family));

    NormalizedKotlinSource { bytes, regions }
}

fn lex_kotlin(source: &str) -> LexedKotlin<'_> {
    let bytes = source.as_bytes();
    let mut token_starts = Vec::new();
    let mut matching_delimiters = BTreeMap::new();
    let mut delimiters = Vec::new();
    let mut delimiter_stack = Vec::new();
    let mut unbalanced_delimiters = Vec::new();

    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(bytes, index + 2);
            }
            b'"' if bytes.get(index + 1) == Some(&b'"') && bytes.get(index + 2) == Some(&b'"') => {
                index = skip_triple_quoted_string(bytes, index + 3);
            }
            b'"' => {
                index = skip_quoted_literal(bytes, index + 1, b'"');
            }
            b'\'' => {
                index = skip_quoted_literal(bytes, index + 1, b'\'');
            }
            b'(' => {
                delimiter_stack.push((index, DelimiterKind::Paren));
                index += 1;
            }
            b')' => {
                close_delimiter(
                    &mut delimiter_stack,
                    &mut matching_delimiters,
                    &mut delimiters,
                    &mut unbalanced_delimiters,
                    index,
                    DelimiterKind::Paren,
                    true,
                );
                index += 1;
            }
            b'{' => {
                delimiter_stack.push((index, DelimiterKind::Brace));
                index += 1;
            }
            b'}' => {
                close_delimiter(
                    &mut delimiter_stack,
                    &mut matching_delimiters,
                    &mut delimiters,
                    &mut unbalanced_delimiters,
                    index,
                    DelimiterKind::Brace,
                    true,
                );
                index += 1;
            }
            b'[' => {
                delimiter_stack.push((index, DelimiterKind::Bracket));
                index += 1;
            }
            b']' => {
                close_delimiter(
                    &mut delimiter_stack,
                    &mut matching_delimiters,
                    &mut delimiters,
                    &mut unbalanced_delimiters,
                    index,
                    DelimiterKind::Bracket,
                    true,
                );
                index += 1;
            }
            b'<' if is_probable_angle_open(bytes, index) => {
                delimiter_stack.push((index, DelimiterKind::Angle));
                index += 1;
            }
            b'>' if bytes.get(index.wrapping_sub(1)) != Some(&b'-') => {
                close_delimiter(
                    &mut delimiter_stack,
                    &mut matching_delimiters,
                    &mut delimiters,
                    &mut unbalanced_delimiters,
                    index,
                    DelimiterKind::Angle,
                    false,
                );
                index += 1;
            }
            b'@' => {
                token_starts.push(index);
                index += 1;
            }
            byte if is_identifier_start_byte(byte) => {
                token_starts.push(index);
                index += 1;
                while index < bytes.len() && is_identifier_byte(bytes[index]) {
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }

    unbalanced_delimiters.extend(delimiter_stack.into_iter().map(|(open, _)| open));
    unbalanced_delimiters.sort_unstable();
    delimiters.sort_by_key(|delimiter| (delimiter.open, delimiter.close));

    LexedKotlin {
        bytes,
        token_starts,
        matching_delimiters,
        delimiters,
        unbalanced_delimiters,
    }
}

pub(crate) fn recover_parse_passes(
    parser: &mut tree_sitter::Parser,
    normalized: &[u8],
    primary: &tree_sitter::Tree,
) -> ParseRecovery {
    let mut unresolved_problems = crate::kotlin_ast::syntax_problems_from_tree(primary.root_node());
    let primary_ranges = unresolved_problems
        .iter()
        .map(|problem| problem.range)
        .collect::<Vec<_>>();
    let boundaries = safe_recovery_boundaries(normalized);
    let mut working_problems = unresolved_problems.clone();
    let mut recovered = Vec::new();
    let mut tried = BTreeSet::new();
    let mut prior_offset = 0usize;
    let mut attempts = 0usize;
    let mut recovered_later_source = false;

    'recovery: while attempts < MAX_RECOVERY_ATTEMPTS {
        let Some(problem) = working_problems
            .iter()
            .find(|problem| problem.range.start >= prior_offset)
            .cloned()
        else {
            break;
        };

        let mut accepted = false;
        for &boundary in boundaries
            .iter()
            .filter(|boundary| **boundary > problem.range.start)
        {
            if !tried.insert((problem.range.start, boundary)) {
                continue;
            }
            if attempts >= MAX_RECOVERY_ATTEMPTS {
                break 'recovery;
            }
            attempts += 1;

            let mut masked = normalized.to_vec();
            let Some(mask_range) = ByteRange::new(problem.range.start, boundary) else {
                continue;
            };
            mask_preserving_lines(&mut masked, mask_range);
            let Some(tree) = parser.parse(masked.as_slice(), None) else {
                continue;
            };
            let next_problems = crate::kotlin_ast::syntax_problems_from_tree(tree.root_node());
            let next_problem_start = next_problems
                .iter()
                .find(|next| next.range.start >= boundary)
                .map_or(normalized.len(), |next| next.range.start);
            let Some(clean) = ByteRange::new(boundary, next_problem_start) else {
                continue;
            };
            // Accept only when the clean suffix advances past the prior offset and
            // contains real syntax; failed boundaries try the next later offset.
            let next_progresses = next_problems
                .iter()
                .find(|next| next.range.start >= boundary)
                .is_none_or(|next| next.range.start > prior_offset);
            if clean.start >= clean.end
                || !next_progresses
                || !contains_named_declaration_or_statement(tree.root_node(), clean)
            {
                continue;
            }

            recovered_later_source = true;
            recovered.push(ParsePass {
                tree,
                clean: vec![clean],
                priority: (recovered.len() + 1) as u16,
            });
            working_problems = next_problems;
            prior_offset = next_problem_start;
            accepted = true;
            if next_problem_start >= normalized.len() {
                break 'recovery;
            }
            break;
        }

        if !accepted {
            break;
        }
    }

    if recovered_later_source {
        for problem in &mut unresolved_problems {
            problem.recovered_later_source = true;
        }
    }

    ParseRecovery {
        primary_clean: complement_problem_ranges(normalized.len(), &primary_ranges),
        recovered,
        unresolved_problems,
    }
}

fn safe_recovery_boundaries(source: &[u8]) -> Vec<usize> {
    let Ok(source) = std::str::from_utf8(source) else {
        return Vec::new();
    };
    let lexed = lex_kotlin(source);
    if !lexed.unbalanced_delimiters.is_empty() {
        return Vec::new();
    }

    let mut boundaries = BTreeSet::new();
    for &token in &lexed.token_starts {
        let brace_depth = lexed
            .delimiters
            .iter()
            .filter(|delimiter| {
                delimiter.kind == DelimiterKind::Brace
                    && delimiter.open < token
                    && token < delimiter.close
            })
            .count();
        if brace_depth <= 1
            && [
                b"class".as_slice(),
                b"data",
                b"enum",
                b"fun",
                b"interface",
                b"object",
                b"typealias",
                b"val",
                b"var",
            ]
            .iter()
            .any(|keyword| starts_with_keyword(lexed.bytes, token, keyword))
        {
            boundaries.insert(token);
        }
        if let Some(statement_start) =
            statement_start_after_line_or_semicolon(lexed.bytes, token, brace_depth)
        {
            boundaries.insert(statement_start);
        }
    }
    boundaries.into_iter().collect()
}

fn statement_start_after_line_or_semicolon(
    bytes: &[u8],
    token: usize,
    brace_depth: usize,
) -> Option<usize> {
    if brace_depth > 1 {
        return None;
    }
    let line_start = bytes[..token]
        .iter()
        .rev()
        .position(|byte| *byte == b'\n')
        .map_or(0, |offset| token - offset);
    let prefix = &bytes[..token];
    let after_semicolon = prefix
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b';');
    let at_line_start = bytes[line_start..token]
        .iter()
        .all(|byte| byte.is_ascii_whitespace());
    if after_semicolon || at_line_start {
        Some(
            bytes[..token]
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(token, |newline| newline + 1),
        )
    } else {
        None
    }
}

fn contains_named_declaration_or_statement(root: tree_sitter::Node<'_>, clean: ByteRange) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_named()
            && clean.contains_node(node)
            && (node.kind().ends_with("_declaration") || node.kind().ends_with("_statement"))
        {
            return true;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    false
}

fn complement_problem_ranges(source_len: usize, problems: &[ByteRange]) -> Vec<ByteRange> {
    let mut clean = Vec::new();
    let mut start = 0;
    for problem in merge_clean_ranges(problems.to_vec()) {
        if start < problem.start {
            clean.push(ByteRange {
                start,
                end: problem.start,
            });
        }
        start = start.max(problem.end);
    }
    if start < source_len {
        clean.push(ByteRange {
            start,
            end: source_len,
        });
    }
    clean
}

fn close_delimiter(
    stack: &mut Vec<(usize, DelimiterKind)>,
    matching_delimiters: &mut BTreeMap<usize, usize>,
    delimiters: &mut Vec<BalancedDelimiter>,
    unbalanced_delimiters: &mut Vec<usize>,
    close: usize,
    expected: DelimiterKind,
    record_unmatched: bool,
) {
    if let Some((open, kind)) = stack.last().copied()
        && kind == expected
    {
        let _ = stack.pop();
        matching_delimiters.insert(open, close);
        delimiters.push(BalancedDelimiter { open, close, kind });
    } else if record_unmatched || stack.iter().any(|(_, kind)| *kind == expected) {
        unbalanced_delimiters.push(close);
    }
}

fn collect_suspend_lambda_transforms(lexed: &LexedKotlin<'_>, out: &mut Vec<RecoveryTransform>) {
    for &token_start in &lexed.token_starts {
        if !starts_with_keyword(lexed.bytes, token_start, b"suspend") {
            continue;
        }

        let keyword_end = token_start + "suspend".len();
        let Some(block_start) = next_significant_index(lexed.bytes, keyword_end) else {
            continue;
        };
        if lexed.bytes.get(block_start) != Some(&b'{') {
            continue;
        }
        let Some(block_end) = lexed.matching_delimiters.get(&block_start).copied() else {
            continue;
        };
        let Some(source) = ByteRange::new(token_start, block_end + 1) else {
            continue;
        };
        let Some(body) = ByteRange::new(block_start, block_end + 1) else {
            continue;
        };
        let Some(mask) = ByteRange::new(token_start, keyword_end) else {
            continue;
        };
        out.push(RecoveryTransform {
            source,
            body: Some(body),
            family: SyntaxFamily::SuspendLambda,
            component_scope: ComponentScopePolicy::Exclude,
            mask_ranges: vec![mask],
        });
    }
}

fn collect_when_guard_transforms(lexed: &LexedKotlin<'_>, out: &mut Vec<RecoveryTransform>) {
    for &token_start in &lexed.token_starts {
        if !starts_with_keyword(lexed.bytes, token_start, b"when") {
            continue;
        }

        let Some(condition_open) = next_significant_index(lexed.bytes, token_start + "when".len())
        else {
            continue;
        };
        if lexed.bytes.get(condition_open) != Some(&b'(') {
            continue;
        }
        let Some(condition_close) = lexed.matching_delimiters.get(&condition_open).copied() else {
            continue;
        };
        let Some(body_open) = next_significant_index(lexed.bytes, condition_close + 1) else {
            continue;
        };
        if lexed.bytes.get(body_open) != Some(&b'{') {
            continue;
        }
        let Some(body_close) = lexed.matching_delimiters.get(&body_open).copied() else {
            continue;
        };

        for &inner_start in &lexed.token_starts {
            if inner_start <= body_open || inner_start >= body_close {
                continue;
            }
            if !starts_with_keyword(lexed.bytes, inner_start, b"if")
                || !is_top_level_in_range(lexed, inner_start, body_open + 1, body_close)
            {
                continue;
            }

            let entry_start = first_non_whitespace_on_line(lexed.bytes, inner_start);
            if entry_start >= inner_start {
                continue;
            }

            let Some(arrow) = find_top_level_arrow(lexed, inner_start, body_close) else {
                continue;
            };
            let Some(body_start) = next_significant_index(lexed.bytes, arrow + 2) else {
                continue;
            };
            let body_end = expression_end(lexed, body_start, body_close);
            let entry_end = line_content_end(lexed.bytes, body_end);
            let Some(source) = ByteRange::new(entry_start, entry_end) else {
                continue;
            };
            let Some(body) = ByteRange::new(body_start, body_end) else {
                continue;
            };
            let Some(mask) = ByteRange::new(inner_start, arrow) else {
                continue;
            };
            out.push(RecoveryTransform {
                source,
                body: Some(body),
                family: SyntaxFamily::WhenGuard,
                component_scope: ComponentScopePolicy::Inherit,
                mask_ranges: vec![mask],
            });
        }
    }
}

fn collect_annotated_function_type_transforms(
    lexed: &LexedKotlin<'_>,
    out: &mut Vec<RecoveryTransform>,
) {
    for &token_start in &lexed.token_starts {
        if !starts_with_keyword(lexed.bytes, token_start, b"Composable")
            || token_start == 0
            || lexed.bytes[token_start - 1] != b'@'
        {
            continue;
        }

        let annotation_start = token_start - 1;
        let Some(type_open) = next_significant_index(lexed.bytes, token_start + "Composable".len())
        else {
            continue;
        };
        if lexed.bytes.get(type_open) != Some(&b'(') {
            continue;
        }
        let Some(type_close) = lexed.matching_delimiters.get(&type_open).copied() else {
            continue;
        };
        if find_top_level_arrow(lexed, type_open + 1, type_close).is_none() {
            continue;
        }

        let source_end = if next_significant_index(lexed.bytes, type_close + 1)
            .is_some_and(|index| lexed.bytes[index] == b'?')
        {
            next_significant_index(lexed.bytes, type_close + 1)
                .map_or(type_close + 1, |index| index + 1)
        } else {
            type_close + 1
        };
        let initializer_body = lambda_initializer_body(lexed, source_end);
        let Some(source) = ByteRange::new(annotation_start, source_end) else {
            continue;
        };
        let Some(open_mask) = ByteRange::new(type_open, type_open + 1) else {
            continue;
        };
        let Some(close_mask) = ByteRange::new(type_close, type_close + 1) else {
            continue;
        };
        out.push(RecoveryTransform {
            source,
            body: initializer_body,
            family: SyntaxFamily::AnnotatedFunctionType,
            component_scope: if initializer_body.is_some() {
                ComponentScopePolicy::ComposableLambda
            } else {
                ComponentScopePolicy::Inherit
            },
            mask_ranges: vec![open_mask, close_mask],
        });
    }
}

fn collect_explicit_backing_field_transforms(
    lexed: &LexedKotlin<'_>,
    out: &mut Vec<RecoveryTransform>,
) {
    for &token_start in &lexed.token_starts {
        if !starts_with_keyword(lexed.bytes, token_start, b"field")
            || first_non_whitespace_on_line(lexed.bytes, token_start) != token_start
        {
            continue;
        }

        let Some(class_body) = enclosing_class_or_object_body(lexed, token_start) else {
            continue;
        };
        if preceding_property_start(lexed, class_body, token_start).is_none() {
            continue;
        }

        let statement_end = line_end(lexed.bytes, token_start);
        let Some(equal_index) = find_top_level_byte(lexed, token_start, statement_end, b'=') else {
            continue;
        };
        let Some(initializer_start) = next_significant_index(lexed.bytes, equal_index + 1) else {
            continue;
        };
        if initializer_start >= statement_end
            || !has_safe_initializer_boundary(lexed, initializer_start, statement_end)
        {
            continue;
        }
        let initializer_end = expression_end(lexed, initializer_start, statement_end);
        let field_end = line_content_end(lexed.bytes, initializer_end);
        let Some(source) = ByteRange::new(token_start, field_end) else {
            continue;
        };
        let Some(body) = ByteRange::new(initializer_start, initializer_end) else {
            continue;
        };
        let Some(mask) = ByteRange::new(token_start, equal_index) else {
            continue;
        };
        out.push(RecoveryTransform {
            source,
            body: Some(body),
            family: SyntaxFamily::ExplicitBackingField,
            component_scope: ComponentScopePolicy::Exclude,
            mask_ranges: vec![mask],
        });
    }
}

fn collect_context_transforms(lexed: &LexedKotlin<'_>, out: &mut Vec<RecoveryTransform>) {
    for &token_start in &lexed.token_starts {
        if !starts_with_keyword(lexed.bytes, token_start, b"context") {
            continue;
        }

        let Some(open) = next_significant_index(lexed.bytes, token_start + "context".len()) else {
            continue;
        };
        if lexed.bytes.get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = lexed.matching_delimiters.get(&open).copied() else {
            continue;
        };
        let items = split_top_level_items(lexed, open + 1, close);
        if items.is_empty() {
            continue;
        }

        let mut mask_ranges = Vec::new();
        let mut saw_parameter = false;
        let mut valid = true;
        for item in items {
            if let Some(colon) = find_top_level_byte(lexed, item.start, item.end, b':') {
                let Some(name) = context_parameter_name(lexed.bytes, item, colon) else {
                    valid = false;
                    break;
                };
                if !has_context_type(lexed.bytes, colon + 1, item.end) {
                    valid = false;
                    break;
                }

                saw_parameter = true;
                mask_ranges.push(name);
                let Some(colon_mask) = ByteRange::new(colon, colon + 1) else {
                    valid = false;
                    break;
                };
                mask_ranges.push(colon_mask);
            } else if !has_context_type(lexed.bytes, item.start, item.end) {
                valid = false;
                break;
            }
        }
        if !valid {
            continue;
        }

        let Some(source) = ByteRange::new(token_start, close + 1) else {
            continue;
        };
        out.push(RecoveryTransform {
            source,
            body: None,
            family: if saw_parameter {
                SyntaxFamily::ContextParameter
            } else {
                SyntaxFamily::ContextReceiver
            },
            component_scope: ComponentScopePolicy::Inherit,
            mask_ranges,
        });
    }
}

fn collect_annotated_type_argument_transforms(
    lexed: &LexedKotlin<'_>,
    out: &mut Vec<RecoveryTransform>,
) {
    for &token_start in &lexed.token_starts {
        if lexed.bytes.get(token_start) != Some(&b'@') {
            continue;
        }
        let annotation_start = token_start;
        let name_start = annotation_start + 1;
        if name_start >= lexed.bytes.len() || !is_annotation_token_byte(lexed.bytes[name_start]) {
            continue;
        }
        let Some(angle_range) =
            smallest_enclosing_delimiter(lexed, annotation_start, DelimiterKind::Angle)
        else {
            continue;
        };
        let Some(annotation_end) = annotation_token_end(lexed.bytes, annotation_start) else {
            continue;
        };
        let next = next_significant_index(lexed.bytes, annotation_end);
        let mask_end = match next {
            Some(open) if lexed.bytes[open] == b'(' => {
                let Some(close) = lexed.matching_delimiters.get(&open).copied() else {
                    continue;
                };
                if close >= angle_range.end - 1 {
                    continue;
                }
                close + 1
            }
            _ => annotation_end,
        };
        let Some(type_start) = next_significant_index(lexed.bytes, mask_end) else {
            continue;
        };
        if type_start >= angle_range.end - 1
            || !(lexed.bytes[type_start] == b'@'
                || lexed.bytes[type_start] == b'('
                || is_identifier_start_byte(lexed.bytes[type_start]))
        {
            continue;
        }
        let trailing_comma = find_top_level_byte(lexed, type_start, angle_range.end - 1, b',')
            .filter(|comma| {
                next_significant_index(lexed.bytes, *comma + 1) == Some(angle_range.end - 1)
            });
        let source_end = trailing_comma.map_or(mask_end, |comma| comma + 1);
        let Some(annotation_mask) = ByteRange::new(annotation_start, mask_end) else {
            continue;
        };
        let mut mask_ranges = vec![annotation_mask];
        if let Some(comma) = trailing_comma {
            let Some(comma_mask) = ByteRange::new(comma, comma + 1) else {
                continue;
            };
            mask_ranges.push(comma_mask);
        }
        let Some(source) = ByteRange::new(annotation_start, source_end) else {
            continue;
        };
        out.push(RecoveryTransform {
            source,
            body: None,
            family: SyntaxFamily::AnnotatedTypeArgument,
            component_scope: ComponentScopePolicy::Exclude,
            mask_ranges,
        });
    }
}

fn select_non_overlapping_transforms(
    mut transforms: Vec<RecoveryTransform>,
) -> Vec<RecoveryTransform> {
    transforms.sort_by_key(|transform| {
        (
            transform.source.len(),
            transform.source.start,
            transform.source.end,
            transform.family,
        )
    });

    let mut selected: Vec<RecoveryTransform> = Vec::new();
    'candidate: for transform in transforms {
        for existing in &selected {
            if transform.source.start < existing.source.end
                && existing.source.start < transform.source.end
            {
                continue 'candidate;
            }
        }
        selected.push(transform);
    }

    selected
}

fn mask_preserving_lines(bytes: &mut [u8], range: ByteRange) {
    for byte in &mut bytes[range.start..range.end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn is_probable_angle_open(bytes: &[u8], index: usize) -> bool {
    let Some(previous) = previous_non_whitespace_byte(bytes, index) else {
        return false;
    };
    let Some(next) =
        next_significant_index(bytes, index + 1).and_then(|next| bytes.get(next).copied())
    else {
        return false;
    };
    (is_identifier_byte(previous) || matches!(previous, b'>' | b')' | b'?' | b'.'))
        && (next == b'@' || is_identifier_start_byte(next))
}

fn starts_with_keyword(bytes: &[u8], start: usize, keyword: &[u8]) -> bool {
    bytes
        .get(start..start + keyword.len())
        .is_some_and(|found| found == keyword)
        && bytes
            .get(start + keyword.len())
            .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn annotation_token_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start.checked_add(1)?;
    while index < bytes.len() && is_annotation_token_byte(bytes[index]) {
        index += 1;
    }

    (index > start + 1).then_some(index)
}

fn next_significant_index(bytes: &[u8], mut index: usize) -> Option<usize> {
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(bytes, index + 2);
            }
            _ => return Some(index),
        }
    }
    None
}

fn previous_non_whitespace_byte(bytes: &[u8], index: usize) -> Option<u8> {
    let mut cursor = index.checked_sub(1)?;
    loop {
        let byte = *bytes.get(cursor)?;
        if !byte.is_ascii_whitespace() {
            return Some(byte);
        }
        cursor = cursor.checked_sub(1)?;
    }
}

fn find_top_level_arrow(lexed: &LexedKotlin<'_>, start: usize, end: usize) -> Option<usize> {
    let mut index = start;
    while index + 1 < end {
        if let Some(close) = lexed.matching_delimiters.get(&index).copied()
            && close < end
        {
            index = close + 1;
            continue;
        }
        if lexed.bytes[index] == b'-' && lexed.bytes.get(index + 1) == Some(&b'>') {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn find_top_level_byte(
    lexed: &LexedKotlin<'_>,
    start: usize,
    end: usize,
    needle: u8,
) -> Option<usize> {
    let mut index = start;
    while index < end {
        if let Some(close) = lexed.matching_delimiters.get(&index).copied()
            && close < end
        {
            index = close + 1;
            continue;
        }
        if lexed.bytes[index] == needle {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn is_top_level_in_range(
    lexed: &LexedKotlin<'_>,
    index: usize,
    range_start: usize,
    range_end: usize,
) -> bool {
    let mut cursor = range_start;
    while cursor < index {
        if let Some(close) = lexed.matching_delimiters.get(&cursor).copied()
            && close < range_end
        {
            if index > cursor && index < close {
                return false;
            }
            cursor = close + 1;
            continue;
        }
        cursor += 1;
    }
    true
}

fn split_top_level_items(lexed: &LexedKotlin<'_>, start: usize, end: usize) -> Vec<ByteRange> {
    let mut items = Vec::new();
    let mut item_start = skip_ascii_whitespace(lexed.bytes, start);
    let mut index = item_start;
    while index < end {
        if let Some(close) = lexed.matching_delimiters.get(&index).copied()
            && close < end
        {
            index = close + 1;
            continue;
        }
        if lexed.bytes[index] == b',' {
            let item_end = trim_trailing_whitespace(lexed.bytes, item_start, index);
            if item_start < item_end
                && let Some(item) = ByteRange::new(item_start, item_end)
            {
                items.push(item);
            }
            item_start = skip_ascii_whitespace(lexed.bytes, index + 1);
        }
        index += 1;
    }
    let item_end = trim_trailing_whitespace(lexed.bytes, item_start, end);
    if item_start < item_end
        && let Some(item) = ByteRange::new(item_start, item_end)
    {
        items.push(item);
    }
    items
}

fn context_parameter_name(bytes: &[u8], item: ByteRange, colon: usize) -> Option<ByteRange> {
    let start = next_significant_index(bytes, item.start)?;
    if start >= colon || !is_identifier_start_byte(*bytes.get(start)?) {
        return None;
    }

    let mut end = start + 1;
    while end < colon && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (next_significant_index(bytes, end) == Some(colon))
        .then(|| ByteRange::new(start, end))
        .flatten()
}

fn has_context_type(bytes: &[u8], start: usize, end: usize) -> bool {
    next_significant_index(bytes, start).is_some_and(|type_start| {
        type_start < end
            && (bytes[type_start] == b'@'
                || bytes[type_start] == b'('
                || is_identifier_start_byte(bytes[type_start]))
    })
}

fn smallest_enclosing_delimiter(
    lexed: &LexedKotlin<'_>,
    index: usize,
    kind: DelimiterKind,
) -> Option<ByteRange> {
    lexed
        .delimiters
        .iter()
        .filter(|delimiter| {
            delimiter.kind == kind && delimiter.open < index && index < delimiter.close
        })
        .min_by_key(|delimiter| delimiter.close.saturating_sub(delimiter.open))
        .and_then(|delimiter| ByteRange::new(delimiter.open, delimiter.close + 1))
}

fn enclosing_class_or_object_body(lexed: &LexedKotlin<'_>, index: usize) -> Option<ByteRange> {
    let body = smallest_enclosing_delimiter(lexed, index, DelimiterKind::Brace)?;
    if !is_top_level_in_range(lexed, index, body.start + 1, body.end - 1) {
        return None;
    }

    let search_start = smallest_enclosing_delimiter(lexed, body.start, DelimiterKind::Brace)
        .map_or(0, |parent| parent.start + 1);
    lexed
        .token_starts
        .iter()
        .copied()
        .filter(|token| search_start <= *token && *token < body.start)
        .filter(|token| is_top_level_in_range(lexed, *token, search_start, body.start))
        .rev()
        .find(|token| {
            starts_with_keyword(lexed.bytes, *token, b"class")
                || starts_with_keyword(lexed.bytes, *token, b"object")
        })
        .map(|_| body)
}

fn preceding_property_start(
    lexed: &LexedKotlin<'_>,
    class_body: ByteRange,
    field_start: usize,
) -> Option<usize> {
    let property_start = lexed
        .token_starts
        .iter()
        .copied()
        .filter(|token| class_body.start < *token && *token < field_start)
        .filter(|token| {
            is_top_level_in_range(lexed, *token, class_body.start + 1, class_body.end - 1)
        })
        .rev()
        .find(|token| {
            starts_with_keyword(lexed.bytes, *token, b"val")
                || starts_with_keyword(lexed.bytes, *token, b"var")
        })?;
    if !lexed.bytes[property_start..field_start].contains(&b'\n')
        || find_top_level_byte(lexed, property_start, field_start, b'=').is_some()
        || find_top_level_byte(lexed, property_start, field_start, b';').is_some()
        || lexed
            .unbalanced_delimiters
            .iter()
            .any(|delimiter| property_start <= *delimiter && *delimiter < field_start)
    {
        return None;
    }

    Some(property_start)
}

fn has_safe_initializer_boundary(
    lexed: &LexedKotlin<'_>,
    initializer_start: usize,
    statement_end: usize,
) -> bool {
    if lexed
        .unbalanced_delimiters
        .iter()
        .any(|delimiter| initializer_start <= *delimiter && *delimiter < statement_end)
        || find_top_level_byte(lexed, initializer_start, statement_end, b';').is_some()
    {
        return false;
    }

    !lexed.delimiters.iter().any(|delimiter| {
        (initializer_start <= delimiter.open
            && delimiter.open < statement_end
            && statement_end <= delimiter.close)
            || (initializer_start <= delimiter.close
                && delimiter.close < statement_end
                && delimiter.open < initializer_start)
    })
}

fn lambda_initializer_body(lexed: &LexedKotlin<'_>, source_end: usize) -> Option<ByteRange> {
    let line_limit = line_end(lexed.bytes, source_end);
    let equal_index = find_top_level_byte(lexed, source_end, line_limit, b'=')?;
    let body_start = next_significant_index(lexed.bytes, equal_index + 1)?;
    if lexed.bytes.get(body_start) != Some(&b'{') {
        return None;
    }
    let body_end = lexed.matching_delimiters.get(&body_start).copied()?;
    ByteRange::new(body_start, body_end + 1)
}

fn expression_end(lexed: &LexedKotlin<'_>, start: usize, limit: usize) -> usize {
    if let Some(close) = lexed.matching_delimiters.get(&start).copied()
        && close < limit
    {
        return close + 1;
    }

    let mut index = start;
    while index < limit {
        if let Some(close) = lexed.matching_delimiters.get(&index).copied()
            && close < limit
        {
            index = close + 1;
            continue;
        }
        if lexed.bytes[index] == b'\n' {
            break;
        }
        index += 1;
    }
    trim_trailing_whitespace(lexed.bytes, start, index)
}

fn line_end(bytes: &[u8], index: usize) -> usize {
    let mut cursor = index;
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}

fn line_content_end(bytes: &[u8], index: usize) -> usize {
    let end = line_end(bytes, index);
    trim_trailing_whitespace(bytes, 0, end)
}

fn first_non_whitespace_on_line(bytes: &[u8], index: usize) -> usize {
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] != b'\n' {
        cursor -= 1;
    }
    skip_ascii_whitespace(bytes, cursor)
}

fn trim_trailing_whitespace(bytes: &[u8], start: usize, mut end: usize) -> usize {
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    end
}

fn is_annotation_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':')
}

fn is_identifier_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn skip_line_comment(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn skip_block_comment(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1_u32;
    while index < bytes.len() {
        match (bytes.get(index), bytes.get(index + 1)) {
            (Some(b'/'), Some(b'*')) => {
                depth = depth.saturating_add(1);
                index += 2;
            }
            (Some(b'*'), Some(b'/')) => {
                depth = depth.saturating_sub(1);
                index += 2;
                if depth == 0 {
                    break;
                }
            }
            _ => index += 1,
        }
    }
    index
}

fn skip_triple_quoted_string(bytes: &[u8], mut index: usize) -> usize {
    while index + 2 < bytes.len() {
        if bytes[index] == b'"' && bytes[index + 1] == b'"' && bytes[index + 2] == b'"' {
            return index + 3;
        }
        index += 1;
    }
    bytes.len()
}

fn skip_quoted_literal(bytes: &[u8], mut index: usize, delimiter: u8) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            byte if byte == delimiter => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::{
        ComponentScopePolicy, MAX_RECOVERY_ATTEMPTS, NormalizedKotlinSource, SyntaxFamily,
        normalize_kotlin_for_parse, recover_parse_passes, safe_recovery_boundaries,
    };
    use crate::kotlin_ast::{new_parser, parse_kotlin_file_permissive};

    // (file, source, after_name)
    const FIXTURES: &[(&str, &str, &str)] = &[
        (
            "SuspendLambda.kt",
            include_str!("../tests/fixtures/kotlin-syntax/app/src/main/kotlin/SuspendLambda.kt"),
            "AfterSuspendLambda",
        ),
        (
            "WhenGuard.kt",
            include_str!("../tests/fixtures/kotlin-syntax/app/src/main/kotlin/WhenGuard.kt"),
            "AfterWhenGuard",
        ),
        (
            "AnnotatedFunctionType.kt",
            include_str!(
                "../tests/fixtures/kotlin-syntax/app/src/main/kotlin/AnnotatedFunctionType.kt"
            ),
            "AfterAnnotatedFunctionType",
        ),
        (
            "ExplicitBackingField.kt",
            include_str!(
                "../tests/fixtures/kotlin-syntax/app/src/main/kotlin/ExplicitBackingField.kt"
            ),
            "AfterExplicitBackingField",
        ),
        (
            "ContextParameter.kt",
            include_str!("../tests/fixtures/kotlin-syntax/app/src/main/kotlin/ContextParameter.kt"),
            "AfterContextParameter",
        ),
        (
            "ContextReceiver.kt",
            include_str!("../tests/fixtures/kotlin-syntax/app/src/main/kotlin/ContextReceiver.kt"),
            "AfterContextReceiver",
        ),
        (
            "WhenTrailingComma.kt",
            include_str!(
                "../tests/fixtures/kotlin-syntax/app/src/main/kotlin/WhenTrailingComma.kt"
            ),
            "AfterWhenTrailingComma",
        ),
        (
            "AnnotatedTypeArgument.kt",
            include_str!(
                "../tests/fixtures/kotlin-syntax/app/src/main/kotlin/AnnotatedTypeArgument.kt"
            ),
            "AfterAnnotatedTypeArgument",
        ),
    ];

    fn fixture_source(file: &str) -> &'static str {
        FIXTURES
            .iter()
            .find_map(|(name, source, _)| (*name == file).then_some(*source))
            .unwrap_or_else(|| panic!("missing fixture {file}"))
    }

    #[test]
    fn known_valid_syntax_fixtures_are_byte_preserving_and_sorted() {
        for &(file, source, _) in FIXTURES {
            let normalized = normalize_kotlin_for_parse(source);
            let tree = parse(&normalized);

            assert_eq!(normalized.bytes.len(), source.len(), "{file}");
            assert_eq!(
                newline_offsets(&normalized.bytes),
                newline_offsets(source.as_bytes()),
                "{file}"
            );
            assert!(
                !tree.root_node().has_error(),
                "{file} should parse cleanly after normalization:\n{}\n{}",
                String::from_utf8_lossy(&normalized.bytes),
                tree.root_node().to_sexp()
            );
            assert!(
                normalized
                    .regions
                    .windows(2)
                    .all(|pair| pair[0].source.start <= pair[1].source.start),
                "{file} regions should stay ordered"
            );
        }
    }

    #[test]
    fn permissive_parse_retains_original_source_and_after_function_locations() {
        let tempdir = tempfile::tempdir().expect("tempdir");

        for &(file, source, after_name) in FIXTURES {
            let path = tempdir.path().join(file);
            std::fs::write(&path, source).expect("write fixture");

            let mut parser = new_parser().expect("parser");
            let parsed =
                parse_kotlin_file_permissive(&mut parser, &path).expect("permissive parse");

            assert_eq!(parsed.source, source, "{file}");

            let after_name_start = source.find(after_name).expect("after function name");
            let expected_position = byte_line_column(source.as_bytes(), after_name_start);
            let after_node = find_function_name_node(
                parsed.primary_tree().root_node(),
                parsed.source.as_bytes(),
                after_name,
            )
            .unwrap_or_else(|| panic!("missing after function {file}"));

            assert_eq!(after_node.start_byte(), after_name_start, "{file}");
            assert_eq!(
                (
                    after_node.start_position().row + 1,
                    after_node.start_position().column + 1
                ),
                expected_position,
                "{file}"
            );
        }
    }

    #[test]
    fn trailing_comma_fixture_requires_no_known_region() {
        let normalized = normalize_kotlin_for_parse(fixture_source("WhenTrailingComma.kt"));

        assert!(
            normalized.regions.is_empty(),
            "grammar already handles trailing-comma when entries"
        );
    }

    #[test]
    fn annotated_type_argument_preserves_its_type_and_masks_its_trailing_comma() {
        let source = fixture_source("AnnotatedTypeArgument.kt");
        let normalized = normalize_kotlin_for_parse(source);
        let annotation_start = source
            .find("@Serializable(with = ItemSerializer::class)")
            .expect("type-use annotation");
        let type_start = source
            .find("    Item,\n>")
            .expect("annotated type argument")
            + 4;
        let type_end = type_start + "Item".len();
        let trailing_comma = type_end;

        assert_eq!(
            &normalized.bytes[type_start..type_end],
            &source.as_bytes()[type_start..type_end]
        );
        assert_eq!(normalized.bytes[trailing_comma], b' ');
        assert_eq!(normalized.regions.len(), 1);
        assert_eq!(
            normalized.regions[0],
            super::SyntaxRegion {
                source: super::ByteRange {
                    start: annotation_start,
                    end: trailing_comma + 1,
                },
                body: None,
                family: SyntaxFamily::AnnotatedTypeArgument,
                component_scope: ComponentScopePolicy::Exclude,
            }
        );
    }

    #[test]
    fn explicit_backing_field_requires_class_property_and_balanced_initializer() {
        for source in [
            "field = MutableStateFlow(emptyList())\n",
            "class Holder {\n    field = MutableStateFlow(emptyList())\n}\n",
            "class Holder {\n    val state: StateFlow<Item>\n        field = MutableStateFlow(\n}\n",
        ] {
            let normalized = normalize_kotlin_for_parse(source);

            assert_eq!(normalized.bytes, source.as_bytes(), "{source}");
            assert!(normalized.regions.is_empty(), "{source}");
        }
    }

    #[test]
    fn known_syntax_text_inside_literals_and_comments_is_unchanged() {
        let source = r#"
val text = "suspend { FetchRepository() } @Composable (() -> Unit)"
val character = '@'
// context(itemScope: ItemScope)
/* when (item) { is Item if enabled -> Unit } */
"#;

        let normalized = normalize_kotlin_for_parse(source);

        assert_eq!(normalized.bytes, source.as_bytes());
        assert!(normalized.regions.is_empty());
    }

    #[test]
    fn context_parameter_requires_a_plain_identifier_and_type() {
        for source in [
            "context(: ItemScope)\nfun Screen() = Unit\n",
            "context(itemScope:)\nfun Screen() = Unit\n",
            "context(itemScope.value: ItemScope)\nfun Screen() = Unit\n",
        ] {
            let normalized = normalize_kotlin_for_parse(source);

            assert_eq!(normalized.bytes, source.as_bytes(), "{source}");
            assert!(normalized.regions.is_empty(), "{source}");
        }
    }

    #[test]
    fn context_parameter_masks_only_the_identifier_and_colon() {
        let source = "context(itemScope /* preserved */ : ItemScope)\nfun Screen() = Unit\n";

        let normalized = normalize_kotlin_for_parse(source);
        let comment_start = source.find("/* preserved */").expect("comment");
        let comment_end = comment_start + "/* preserved */".len();

        assert_eq!(
            &normalized.bytes[comment_start..comment_end],
            &source.as_bytes()[comment_start..comment_end]
        );
        assert_eq!(normalized.regions.len(), 1);
        assert_eq!(normalized.regions[0].family, SyntaxFamily::ContextParameter);
    }

    #[test]
    fn annotated_type_argument_requires_a_following_type() {
        let source = "val items: List<@Serializable(with = ItemSerializer::class)>\n";

        let normalized = normalize_kotlin_for_parse(source);

        assert_eq!(normalized.bytes, source.as_bytes());
        assert!(normalized.regions.is_empty());
    }

    #[test]
    fn context_receiver_fixture_records_region_without_masking() {
        let normalized = normalize_kotlin_for_parse(fixture_source("ContextReceiver.kt"));

        assert!(
            normalized
                .regions
                .iter()
                .any(|region| region.family == SyntaxFamily::ContextReceiver),
            "legacy context receivers should still be recorded as known syntax"
        );
    }

    fn parse(normalized: &NormalizedKotlinSource) -> tree_sitter::Tree {
        let mut parser = new_parser().expect("parser");
        parser
            .parse(normalized.bytes.as_slice(), None)
            .expect("normalized parse")
    }

    fn newline_offsets(bytes: &[u8]) -> Vec<usize> {
        bytes
            .iter()
            .enumerate()
            .filter_map(|(index, byte)| (*byte == b'\n').then_some(index))
            .collect()
    }

    fn byte_line_column(bytes: &[u8], target: usize) -> (usize, usize) {
        let mut row = 1_usize;
        let mut column = 1_usize;
        for &byte in bytes.iter().take(target) {
            if byte == b'\n' {
                row += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (row, column)
    }

    fn find_function_name_node<'a>(
        root: tree_sitter::Node<'a>,
        source: &[u8],
        expected: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "function_declaration"
                && let Some(name) = node.child_by_field_name("name")
                && name.utf8_text(source).ok() == Some(expected)
            {
                return Some(name);
            }

            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        None
    }

    #[test]
    fn recovery_attempts_are_bounded_and_monotonic() {
        let mut source = String::from("import androidx.compose.runtime.Composable\n\n");
        for index in 0..80 {
            source.push_str(&format!("fun Broken{index}() = ()\n\n"));
            source.push_str("@Composable\n");
            source.push_str(&format!(
                "fun After{index}() {{ PrimaryButton(onClick = {{}}) }}\n\n"
            ));
        }

        let normalized = normalize_kotlin_for_parse(&source);
        let mut parser = new_parser().expect("parser");
        let primary = parser
            .parse(normalized.bytes.as_slice(), None)
            .expect("primary tree");
        let recovery = recover_parse_passes(&mut parser, &normalized.bytes, &primary);

        let mut prior_clean_start = 0usize;
        for pass in &recovery.recovered {
            let clean_start = pass
                .clean
                .iter()
                .map(|range| range.start)
                .min()
                .expect("recovered pass has clean ranges");
            assert!(
                clean_start > prior_clean_start,
                "accepted recovery passes must advance clean start ({clean_start} <= {prior_clean_start})"
            );
            prior_clean_start = clean_start;
        }
        assert!(
            recovery.recovered.len() <= MAX_RECOVERY_ATTEMPTS,
            "recovered passes exceed attempt cap: {}",
            recovery.recovered.len()
        );
        assert!(
            safe_recovery_boundaries(b"fun Broken() {").is_empty(),
            "unbalanced delimiters must not emit recovery boundaries"
        );
    }
}
