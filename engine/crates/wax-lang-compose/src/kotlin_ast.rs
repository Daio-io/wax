//! Shared Kotlin tree-sitter helpers for Compose extraction.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation};

use crate::kotlin_recovery::{
    ByteRange, NormalizedKotlinSource, ParsePass, SyntaxFamily, SyntaxProblem, SyntaxRegion,
    normalize_kotlin_for_parse, recover_parse_passes,
};

/// Parsed Kotlin source and syntax trees.
///
/// `source` is always the original file text. `primary.tree` is usually
/// parsed from a byte-preserving normalized buffer that works around known
/// tree-sitter Kotlin grammar gaps, but falls back to the original parse when
/// normalization would degrade a clean tree.
#[derive(Debug)]
pub(crate) struct ParsedKotlinFile {
    pub(crate) source: String,
    pub(crate) primary: ParsePass,
    #[allow(dead_code)]
    pub(crate) recovered: Vec<ParsePass>,
    #[allow(dead_code)]
    pub(crate) syntax_regions: Vec<SyntaxRegion>,
    pub(crate) unresolved_problems: Vec<SyntaxProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryParseSource {
    Original,
    Normalized,
}

impl ParsedKotlinFile {
    #[allow(dead_code)]
    pub(crate) fn passes(&self) -> impl Iterator<Item = &ParsePass> {
        std::iter::once(&self.primary).chain(&self.recovered)
    }

    pub(crate) fn primary_tree(&self) -> &tree_sitter::Tree {
        &self.primary.tree
    }

    pub(crate) fn is_partial(&self) -> bool {
        !self.unresolved_problems.is_empty()
    }
}

/// Errors produced while reading or parsing Kotlin source.
#[derive(Debug)]
pub(crate) enum ParseKotlinFileError {
    Io {
        context: String,
        source: std::io::Error,
    },
    ParseFailed(PathBuf),
}

impl std::fmt::Display for ParseKotlinFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::ParseFailed(path) => {
                write!(f, "failed to parse Kotlin source {}", path.display())
            }
        }
    }
}

impl std::error::Error for ParseKotlinFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ParseFailed(_) => None,
        }
    }
}

/// Errors produced while initialising the Kotlin tree-sitter parser.
#[derive(Debug)]
pub(crate) enum KotlinAstError {
    /// The parser rejected the grammar language version.
    SetLanguage(String),
}

impl std::fmt::Display for KotlinAstError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetLanguage(reason) => write!(f, "failed to configure parser: {reason}"),
        }
    }
}

impl std::error::Error for KotlinAstError {}

pub(crate) fn new_parser() -> Result<tree_sitter::Parser, KotlinAstError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .map_err(|err| KotlinAstError::SetLanguage(err.to_string()))?;
    Ok(parser)
}

/// Returns the Kotlin `package` declaration from a parsed source file, when present.
pub(crate) fn package_name_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<String> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "package_header" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "qualified_identifier" {
                    return child.utf8_text(source).ok().map(str::to_owned);
                }
            }
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }

    None
}

pub(crate) fn collect_kotlin_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_kotlin_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "kt") {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn parse_kotlin_file_permissive(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let source = fs::read_to_string(path).map_err(|source| ParseKotlinFileError::Io {
        context: format!("read Kotlin source {}", path.display()),
        source,
    })?;
    let normalized = normalize_kotlin_for_parse(&source);
    parse_kotlin_source_permissive(parser, path, source, normalized)
}

fn parse_kotlin_source_permissive(
    parser: &mut tree_sitter::Parser,
    path: &Path,
    source: String,
    normalized: NormalizedKotlinSource,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let original_bytes = source.as_bytes();
    let normalized_tree = parser.parse(normalized.bytes.as_slice(), None);
    let should_validate_original = normalized.bytes.as_slice() != original_bytes
        && normalized_tree
            .as_ref()
            .is_none_or(|tree| tree.root_node().has_error());
    let original_tree = should_validate_original
        .then(|| parser.parse(original_bytes, None))
        .flatten();

    let (tree, primary_source) = select_primary_parse(normalized_tree, original_tree)
        .ok_or_else(|| ParseKotlinFileError::ParseFailed(path.to_path_buf()))?;
    // Regions describe transforms on the normalized buffer; drop them when the
    // original tree wins so they cannot mask extraction on the chosen tree.
    let (primary_bytes, syntax_regions) = match primary_source {
        PrimaryParseSource::Original => (original_bytes, Vec::new()),
        PrimaryParseSource::Normalized => (normalized.bytes.as_slice(), normalized.regions),
    };
    let recovery = recover_parse_passes(parser, primary_bytes, &tree);

    Ok(ParsedKotlinFile {
        unresolved_problems: recovery.unresolved_problems,
        source,
        primary: ParsePass {
            tree,
            clean: recovery.primary_clean,
            priority: 0,
        },
        recovered: recovery.recovered,
        syntax_regions,
    })
}

fn select_primary_parse(
    normalized: Option<tree_sitter::Tree>,
    original: Option<tree_sitter::Tree>,
) -> Option<(tree_sitter::Tree, PrimaryParseSource)> {
    match (normalized, original) {
        (Some(tree), _) if !tree.root_node().has_error() => {
            Some((tree, PrimaryParseSource::Normalized))
        }
        (_, Some(tree)) if !tree.root_node().has_error() => {
            Some((tree, PrimaryParseSource::Original))
        }
        (Some(tree), _) => Some((tree, PrimaryParseSource::Normalized)),
        (_, Some(tree)) => Some((tree, PrimaryParseSource::Original)),
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn parse_kotlin_file_strict(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let parsed = parse_kotlin_file_permissive(parser, path)?;
    if parsed.is_partial() {
        return Err(ParseKotlinFileError::ParseFailed(path.to_path_buf()));
    }

    Ok(parsed)
}

/// Diagnostic emitted when tree-sitter returns no syntax tree for a source file.
pub(crate) fn unparseable_file_diagnostic(relative_file: &str) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: "parse_failed".to_owned(),
        message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
        location: Some(SourceLocation {
            file: relative_file.to_owned(),
            line: 1,
            column: None,
        }),
    }
}

/// Diagnostic emitted when tree-sitter recovers a partial tree with syntax errors.
pub(crate) fn partial_tree_parse_diagnostic(
    problem: &SyntaxProblem,
    relative_file: &str,
) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: "parse_failed".to_owned(),
        message: format!(
            "tree-sitter could not fully parse {} syntax in {relative_file} near {}:{}; {}; component, token, local-definition, or hard-coded-style facts in the skipped region may be incomplete",
            syntax_family_name(problem.family),
            problem.line,
            problem.column,
            if problem.recovered_later_source {
                "skipped the uncertain region and continued scanning later source"
            } else {
                "file scanned with gaps"
            },
        ),
        location: Some(SourceLocation {
            file: relative_file.to_owned(),
            line: problem.line,
            column: Some(problem.column),
        }),
    }
}

fn syntax_family_name(family: SyntaxFamily) -> &'static str {
    match family {
        SyntaxFamily::SuspendLambda => "suspend lambda",
        SyntaxFamily::SoftKeywordFunctionName => "soft-keyword function name",
        SyntaxFamily::WhenGuard => "when guard",
        SyntaxFamily::AnnotatedFunctionType => "annotated function type",
        SyntaxFamily::ExplicitBackingField => "explicit backing field",
        SyntaxFamily::ContextParameter => "context parameter",
        SyntaxFamily::ContextReceiver => "context receiver",
        SyntaxFamily::AnnotatedTypeArgument => "annotated type argument",
        SyntaxFamily::Unknown => "unknown",
    }
}

pub(crate) fn syntax_problems_from_tree(root: tree_sitter::Node<'_>) -> Vec<SyntaxProblem> {
    collect_syntax_problem_nodes(root)
        .into_iter()
        .map(|node| {
            let start = node.start_position();
            SyntaxProblem {
                range: node_byte_range(node),
                line: u32::try_from(start.row.saturating_add(1)).unwrap_or(u32::MAX),
                column: u32::try_from(start.column.saturating_add(1)).unwrap_or(u32::MAX),
                family: SyntaxFamily::Unknown,
                recovered_later_source: false,
            }
        })
        .collect()
}

fn collect_syntax_problem_nodes(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut candidates = collect_leaf_problem_candidates(root);
    candidates.sort_by_key(|node| (node.start_byte(), node.end_byte()));

    let mut selected = Vec::with_capacity(candidates.len());
    let mut current_group = None;
    for candidate in candidates {
        let candidate_range = node_byte_range(candidate);
        let candidate_is_point = candidate_range.start == candidate_range.end;
        match current_group.take() {
            None => {
                current_group = Some((candidate_range.end, candidate_is_point, candidate));
            }
            Some((group_end, group_has_point_at_end, best))
                if problem_range_connects_to_group(
                    group_end,
                    group_has_point_at_end,
                    candidate_range,
                ) =>
            {
                let (group_end, group_has_point_at_end) = if candidate_range.end > group_end {
                    (candidate_range.end, candidate_is_point)
                } else {
                    (
                        group_end,
                        group_has_point_at_end
                            || (candidate_range.end == group_end && candidate_is_point),
                    )
                };
                let best = if syntax_problem_sort_key(candidate) < syntax_problem_sort_key(best) {
                    candidate
                } else {
                    best
                };
                current_group = Some((group_end, group_has_point_at_end, best));
            }
            Some((_, _, best)) => {
                selected.push(best);
                current_group = Some((candidate_range.end, candidate_is_point, candidate));
            }
        }
    }
    if let Some((_, _, best)) = current_group {
        selected.push(best);
    }
    selected
}

fn collect_leaf_problem_candidates(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut candidates = Vec::new();
    let mut has_problem_descendant = Vec::new();
    let mut stack = vec![(root, None)];

    while let Some((node, nearest_problem)) = stack.pop() {
        let nearest_problem = if node.is_error() || node.is_missing() {
            if let Some(index) = nearest_problem {
                has_problem_descendant[index] = true;
            }
            let index = candidates.len();
            candidates.push(node);
            has_problem_descendant.push(false);
            Some(index)
        } else {
            nearest_problem
        };

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push((child, nearest_problem));
            }
        }
    }

    candidates
        .into_iter()
        .zip(has_problem_descendant)
        .filter_map(|(candidate, has_descendant)| (!has_descendant).then_some(candidate))
        .collect()
}

fn syntax_problem_sort_key(node: tree_sitter::Node<'_>) -> (usize, bool, usize) {
    (
        node.end_byte().saturating_sub(node.start_byte()),
        !node.is_missing(),
        node.start_byte(),
    )
}

fn node_byte_range(node: tree_sitter::Node<'_>) -> ByteRange {
    ByteRange {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

fn problem_range_connects_to_group(
    group_end: usize,
    group_has_point_at_end: bool,
    candidate: ByteRange,
) -> bool {
    candidate.start < group_end
        || (candidate.start == group_end
            && (group_has_point_at_end || candidate.start == candidate.end))
}

pub(crate) fn annotation_type_name(
    annotation: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<String> {
    let mut cursor = annotation.walk();
    for child in annotation.named_children(&mut cursor) {
        match child.kind() {
            "user_type" => return last_type_name_segment(child, source),
            "type" => {
                let mut type_cursor = child.walk();
                for type_child in child.named_children(&mut type_cursor) {
                    if type_child.kind() == "user_type" {
                        return last_type_name_segment(type_child, source);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn last_type_name_segment(user_type: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = user_type.walk();
    let mut last_type_identifier = None;
    for type_child in user_type.named_children(&mut cursor) {
        if matches!(type_child.kind(), "identifier" | "type_identifier") {
            last_type_identifier = type_child.utf8_text(source).ok().map(str::to_owned);
        }
    }
    last_type_identifier
}

fn has_annotation_named(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut modifiers_cursor = child.walk();
            for modifier in child.named_children(&mut modifiers_cursor) {
                if modifier.kind() == "annotation"
                    && annotation_type_name(modifier, source).as_deref() == Some(expected)
                {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn has_composable_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    has_annotation_named(node, source, "Composable")
}

pub(crate) fn has_preview_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    has_annotation_named(node, source, "Preview")
}

pub(crate) fn function_name_from_decl(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = name_node.utf8_text(source).ok()?.to_owned();
        return Some((name, name_node.start_position()));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "simple_identifier" | "identifier") {
            let name = child.utf8_text(source).ok()?.to_owned();
            return Some((name, child.start_position()));
        }
    }
    None
}

/// Import bindings collected from Kotlin `import` declarations in one source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ImportBindings {
    /// Maps local symbol names to the package prefix they were imported from.
    pub symbol_packages: BTreeMap<String, String>,
    /// Imported declaration names keyed by local symbol name.
    pub symbol_names: BTreeMap<String, String>,
    /// Package prefixes imported with a wildcard (`import com.example.*`).
    pub wildcard_packages: Vec<String>,
}

impl ImportBindings {
    /// Returns the package prefix for a symbol used at a call site, when known.
    pub(crate) fn package_for_symbol(&self, symbol: &str) -> Option<&str> {
        if let Some(package) = self.symbol_packages.get(symbol) {
            return Some(package.as_str());
        }

        match self.wildcard_packages.len() {
            0 => None,
            1 => Some(self.wildcard_packages[0].as_str()),
            _ => None,
        }
    }
}

/// Collects import bindings from the top level of a Kotlin source file.
pub(crate) fn collect_import_bindings(
    root: tree_sitter::Node<'_>,
    source: &[u8],
) -> ImportBindings {
    let mut bindings = ImportBindings::default();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "import" {
            if let Some(import) = parse_import_directive(node, source) {
                match import {
                    ParsedImport::Named {
                        local_name,
                        package,
                        symbol,
                    } => {
                        bindings.symbol_packages.insert(local_name.clone(), package);
                        bindings.symbol_names.insert(local_name, symbol);
                    }
                    ParsedImport::Wildcard { package } => {
                        bindings.wildcard_packages.push(package);
                    }
                }
            }
            continue;
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }

    bindings.wildcard_packages.sort();
    bindings
}

enum ParsedImport {
    Named {
        local_name: String,
        package: String,
        symbol: String,
    },
    Wildcard {
        package: String,
    },
}

fn parse_import_directive(
    import_node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<ParsedImport> {
    let qualified = import_node
        .named_children(&mut import_node.walk())
        .find(|child| child.kind() == "qualified_identifier")?;

    let qualified_name = qualified.utf8_text(source).ok()?.to_owned();
    let mut alias = None;
    let mut is_wildcard = false;

    let mut cursor = import_node.walk();
    for child in import_node.children(&mut cursor) {
        if child.kind() == "as"
            && let Some(next) = child.next_sibling()
            && (next.kind() == "identifier" || next.kind() == "simple_identifier")
        {
            alias = next.utf8_text(source).ok().map(str::to_owned);
        }
        if child.kind() == "*" || child.utf8_text(source).ok().is_some_and(|text| text == "*") {
            is_wildcard = true;
        }
    }

    if is_wildcard {
        return Some(ParsedImport::Wildcard {
            package: qualified_name,
        });
    }

    let package = package_prefix_from_qualified(&qualified_name)?;
    let local_name = alias.unwrap_or_else(|| symbol_from_qualified(&qualified_name));
    Some(ParsedImport::Named {
        local_name,
        package,
        symbol: symbol_from_qualified(&qualified_name),
    })
}

fn package_prefix_from_qualified(qualified: &str) -> Option<String> {
    let (package, _) = qualified.rsplit_once('.')?;
    if package.is_empty() {
        None
    } else {
        Some(package.to_owned())
    }
}

fn symbol_from_qualified(qualified: &str) -> String {
    qualified
        .rsplit_once('.')
        .map_or(qualified, |(_, symbol)| symbol)
        .to_owned()
}

pub(crate) fn nearest_enclosing_composable(
    mut node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_declaration"
            && has_composable_annotation(parent, source)
            && let Some((name, pos)) = function_name_from_decl(parent, source)
            && name.starts_with(|c: char| c.is_ascii_uppercase())
        {
            return Some((name, pos));
        }
        node = parent;
    }
    None
}

pub(crate) fn is_within_preview_composable(mut node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_declaration"
            && has_composable_annotation(parent, source)
            && has_preview_annotation(parent, source)
        {
            return true;
        }
        node = parent;
    }

    false
}

/// True when `node` has an error/missing ancestor that still lies inside a clean range.
pub(crate) fn node_has_error_ancestor_within(
    mut node: tree_sitter::Node<'_>,
    clean: &[ByteRange],
) -> bool {
    while let Some(parent) = node.parent() {
        if !clean.iter().any(|range| range.contains_node(parent)) {
            break;
        }
        if parent.is_error() || parent.is_missing() {
            return true;
        }
        node = parent;
    }
    false
}

pub(crate) fn is_pascal_case_composable_symbol(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

pub(crate) fn is_non_ui_scaffolding_composable_symbol(symbol: &str) -> bool {
    // Compose provider/effect naming convention marks dependency wiring or side effects, not UI.
    symbol.starts_with("Provide") || symbol.ends_with("Effect")
}

pub(crate) fn call_simple_callee(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "simple_identifier" | "identifier") {
            let name = child.utf8_text(source).ok()?.to_owned();
            return Some((name, child.start_position()));
        }
        if child.kind() == "expression"
            && let Some(found) = simple_identifier_from_expression(child, source)
        {
            return Some(found);
        }
    }
    None
}

fn simple_identifier_from_expression(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    if matches!(node.kind(), "simple_identifier" | "identifier") {
        let name = node.utf8_text(source).ok()?.to_owned();
        return Some((name, node.start_position()));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = simple_identifier_from_expression(child, source) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kotlin_recovery::ComponentScopePolicy;

    fn parse_tree(source: &str) -> tree_sitter::Tree {
        let mut parser = new_parser().expect("parser");
        parser.parse(source.as_bytes(), None).expect("tree")
    }

    #[test]
    fn primary_parse_selection_is_monotonic() {
        let cases = [
            (
                Some("fun Normalized() {}\n"),
                Some("fun Original() {}\n"),
                PrimaryParseSource::Normalized,
                false,
            ),
            (
                Some("fun Normalized( {\n"),
                Some("fun Original() {}\n"),
                PrimaryParseSource::Original,
                false,
            ),
            (
                Some("fun Normalized() {}\n"),
                Some("fun Original( {\n"),
                PrimaryParseSource::Normalized,
                false,
            ),
            (
                Some("fun Normalized( {\n"),
                Some("fun Original( {\n"),
                PrimaryParseSource::Normalized,
                true,
            ),
            (
                None,
                Some("fun Original() {}\n"),
                PrimaryParseSource::Original,
                false,
            ),
        ];

        for (normalized, original, want_source, want_error) in cases {
            let (tree, source) =
                select_primary_parse(normalized.map(parse_tree), original.map(parse_tree))
                    .expect("selected");
            assert_eq!(source, want_source);
            assert_eq!(tree.root_node().has_error(), want_error);
        }

        assert!(select_primary_parse(None, None).is_none());
    }

    #[test]
    fn permissive_parse_prefers_clean_original_over_degraded_normalization() {
        let source = "fun Host() {}\n".to_owned();
        let host_body = ByteRange::new(11, 13).expect("body range");
        let normalized = NormalizedKotlinSource {
            bytes: b"fun Host(  {}\n".to_vec(),
            // Stale Exclude region from an unused rewrite — must be dropped on Original.
            regions: vec![SyntaxRegion {
                source: ByteRange::new(0, source.len()).expect("source range"),
                body: Some(host_body),
                family: SyntaxFamily::SuspendLambda,
                component_scope: ComponentScopePolicy::Exclude,
            }],
        };
        assert_eq!(source.len(), normalized.bytes.len());

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_source_permissive(
            &mut parser,
            Path::new("Host.kt"),
            source.clone(),
            normalized,
        )
        .expect("fallback parse");

        assert_eq!(parsed.source, source);
        assert!(!parsed.primary_tree().root_node().has_error());
        assert!(!parsed.is_partial());
        assert!(parsed.unresolved_problems.is_empty());
        assert!(
            parsed.syntax_regions.is_empty(),
            "Original fallback must not retain normalized rewrite regions"
        );
    }

    fn first_node_of_kind<'a>(
        root: tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == kind {
                return Some(node);
            }
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    stack.push(child);
                }
            }
        }
        None
    }

    #[test]
    fn collect_import_bindings_maps_named_and_wildcard_imports() {
        let mut parser = new_parser().expect("parser");
        let source = r#"
import com.acme.designsystem.Button
import com.foundation.ui.Icon
import com.example.widgets.*
import com.example.widgets.Widget as CustomWidget

@Composable
fun Screen() {}
"#;
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let bindings = collect_import_bindings(tree.root_node(), source.as_bytes());

        assert_eq!(
            bindings.symbol_packages.get("Button"),
            Some(&"com.acme.designsystem".to_owned())
        );
        assert_eq!(
            bindings.symbol_packages.get("Icon"),
            Some(&"com.foundation.ui".to_owned())
        );
        assert_eq!(
            bindings.symbol_packages.get("CustomWidget"),
            Some(&"com.example.widgets".to_owned())
        );
        assert_eq!(
            bindings.wildcard_packages,
            vec!["com.example.widgets".to_owned()]
        );
        assert_eq!(
            bindings.package_for_symbol("Button"),
            Some("com.acme.designsystem")
        );
        assert_eq!(
            bindings.package_for_symbol("Icon"),
            Some("com.foundation.ui")
        );
    }

    #[test]
    fn annotation_type_name_returns_last_segment_for_qualified_names() {
        let mut parser = new_parser().expect("parser");
        let source = "@androidx.compose.runtime.Composable\nfun QualifiedCard() {}";
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let root = tree.root_node();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "annotation" {
                assert_eq!(
                    annotation_type_name(node, source.as_bytes()).as_deref(),
                    Some("Composable")
                );
                return;
            }
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    stack.push(child);
                }
            }
        }

        panic!("annotation not found");
    }

    #[test]
    fn smallest_problem_prefers_nested_missing_or_error() {
        let mut parser = new_parser().expect("parser");
        let source = r#"
import androidx.compose.runtime.Composable

@Composable
fun Broken() {
    val content: @Composable ((String) -> Unit =
}
"#;
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let root = tree.root_node();
        let function = first_node_of_kind(root, "function_declaration")
            .unwrap_or_else(|| panic!("{}", root.to_sexp()));
        let problems = collect_syntax_problem_nodes(root);

        assert_eq!(problems.len(), 1, "expected one grouped syntax problem");
        let problem = problems[0];
        assert!(
            problem.start_byte() > function.start_byte(),
            "expected nested missing/error node instead of outer function_declaration"
        );
        assert!(problem.is_missing() || problem.is_error());
    }

    #[test]
    fn disjoint_failures_in_one_tree_produce_two_ordered_problems() {
        let mut parser = new_parser().expect("parser");
        let source = "fun First() { val one = }\nfun Second() { val two = }\n";
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let problems = collect_syntax_problem_nodes(tree.root_node());

        assert_eq!(problems.len(), 2);
        assert!(
            problems[0].start_byte() < problems[1].start_byte(),
            "disjoint problems should remain in source order"
        );
    }

    #[test]
    fn zero_width_problem_bridges_touching_problem_ranges() {
        let point = ByteRange { start: 10, end: 10 };
        let right = ByteRange { start: 10, end: 20 };

        assert!(problem_range_connects_to_group(10, false, point));
        assert!(problem_range_connects_to_group(10, true, right));
    }

    #[test]
    fn syntax_problem_collection_handles_deep_malformed_input_iteratively() {
        let mut parser = new_parser().expect("parser");
        let source = format!("fun Broken() = {}0\n", "[".repeat(8_192));
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let root = tree.root_node();

        assert!(root.has_error(), "test input should remain malformed");
        assert!(
            !collect_syntax_problem_nodes(root).is_empty(),
            "deep malformed input should produce a syntax problem"
        );
    }

    #[test]
    fn syntax_problem_collection_handles_many_independent_errors_within_budget() {
        let mut parser = new_parser().expect("parser");
        let source = (0..1_000)
            .map(|index| format!("fun Broken{index}() {{ val x = }}\n"))
            .collect::<String>();
        let tree = parser.parse(source.as_bytes(), None).expect("tree");

        let started = std::time::Instant::now();
        let problems = collect_syntax_problem_nodes(tree.root_node());
        let elapsed = started.elapsed();

        assert!(
            problems.len() >= 900,
            "expected independent parser problems, found {}",
            problems.len()
        );
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "syntax problem selection took {elapsed:?}"
        );
    }

    #[test]
    fn known_recovery_metadata_defaults_to_one_primary_pass() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("Screen.kt");
        fs::write(
            &source_file,
            "@Composable\nfun PrimaryButton() {}\nfun Helper() = Unit\n",
        )
        .expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed =
            parse_kotlin_file_permissive(&mut parser, &source_file).expect("permissive parse");

        assert_eq!(parsed.passes().count(), 1);
        assert_eq!(
            parsed.primary.clean,
            vec![ByteRange::new(0, parsed.source.len()).unwrap()]
        );
        assert_eq!(parsed.primary.priority, 0);
        assert!(parsed.recovered.is_empty());
        assert!(parsed.syntax_regions.is_empty());
        assert!(parsed.unresolved_problems.is_empty());
        assert!(!parsed.is_partial());
    }

    #[test]
    fn strict_parse_reports_syntax_errors() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let broken_file = tempdir.path().join("Broken.kt");
        fs::write(&broken_file, "@Composable\nfun Broken(").expect("write broken source");

        let mut parser = new_parser().expect("parser");
        let err = parse_kotlin_file_strict(&mut parser, &broken_file)
            .expect_err("strict parse should fail");

        assert!(matches!(err, ParseKotlinFileError::ParseFailed(path) if path == broken_file));
    }

    #[test]
    fn permissive_parse_keeps_partial_trees() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let broken_file = tempdir.path().join("Broken.kt");
        fs::write(
            &broken_file,
            "@Composable\nfun PrimaryButton() {}\nfun Broken(\n@Composable\nfun SecondaryButton() {}",
        )
        .expect("write broken source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &broken_file)
            .expect("permissive parse should keep partial trees");

        assert!(parsed.primary_tree().root_node().has_error());
        assert!(parsed.is_partial());
        assert!(!parsed.unresolved_problems.is_empty());
    }

    #[test]
    fn permissive_parse_normalizes_parenthesized_composable_parameter_without_errors() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("MainApp.kt");
        fs::write(
            &source_file,
            r#"
import androidx.compose.runtime.Composable

interface NavArgument
interface NavDecoration

private object CapsuleDecor : NavDecoration {
    @Composable
    override fun <T : NavArgument> DecoratedContent(
        args: List<T>,
        modifier: Modifier,
        content: @Composable ((T) -> Unit),
    ) {
        content.invoke(args.first())
    }
}
"#,
        )
        .expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &source_file)
            .expect("permissive parse should succeed");

        assert!(
            !parsed.primary_tree().root_node().has_error(),
            "valid parenthesized annotated function type should parse without tree-sitter errors"
        );
        assert!(
            parsed
                .source
                .contains("content: @Composable ((T) -> Unit),"),
            "parsed source must remain the original text"
        );
    }

    #[test]
    fn normalization_preserves_literals_and_comments_while_rewriting_known_syntax() {
        let source = r#"
val label = "@Composable ((T) -> Unit)"
// @Composable ((T) -> Unit)
fun Screen(
    content: @Composable ((T) -> Unit),
) {}
"#;

        let normalized = normalize_kotlin_for_parse(source);

        assert_eq!(
            normalized.bytes,
            br#"
val label = "@Composable ((T) -> Unit)"
// @Composable ((T) -> Unit)
fun Screen(
    content: @Composable  (T) -> Unit ,
) {}
"#
        );
        assert_eq!(normalized.regions.len(), 1);
    }

    #[test]
    fn normalization_ignores_parens_and_arrows_inside_literals() {
        let source = r#"
fun Screen(
    content: @Composable ((@Label(")") T) -> Unit),
    maybe: @Composable ((@Label("->") T)),
) {}
"#;

        let normalized = normalize_kotlin_for_parse(source);

        assert_eq!(
            normalized.bytes,
            br#"
fun Screen(
    content: @Composable  (@Label(")") T) -> Unit ,
    maybe: @Composable ((@Label("->") T)),
) {}
"#
        );
    }

    #[test]
    fn permissive_parse_normalizes_supported_annotated_function_type_positions() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("FunctionTypes.kt");
        let source = r#"
annotation class Composable
interface Scope
class Item

fun parameter(content: @Composable (() -> Unit)) {}
val handler: @Composable ((T) -> Unit) = {}
fun factory(): @Composable ((T) -> Unit) = {}
class Screen(
    val content: @Composable ((T) -> Unit),
)
val nullable: @Composable (() -> Unit)? = null
val receiver: @Composable (Scope.(Item) -> Unit) = {}
"#;
        fs::write(&source_file, source).expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed =
            parse_kotlin_file_permissive(&mut parser, &source_file).expect("permissive parse");

        assert!(
            !parsed.primary_tree().root_node().has_error(),
            "{}",
            parsed.primary_tree().root_node().to_sexp()
        );
        assert_eq!(
            parsed
                .syntax_regions
                .iter()
                .filter(|region| region.family == SyntaxFamily::AnnotatedFunctionType)
                .count(),
            6
        );
    }

    #[test]
    fn permissive_parse_handles_empty_source_without_panicking() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("Empty.kt");
        fs::write(&source_file, "").expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &source_file)
            .expect("empty source should parse");

        assert_eq!(parsed.source, "");
        assert_eq!(parsed.passes().count(), 1);
    }

    #[test]
    fn permissive_parse_handles_unclosed_block_comment_without_panicking() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("BlockComment.kt");
        fs::write(&source_file, "/* unterminated").expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &source_file)
            .expect("unterminated block comment should still produce a parsed file");

        assert!(parsed.primary_tree().root_node().has_error() || parsed.is_partial());
    }

    #[test]
    fn permissive_parse_handles_unclosed_triple_quoted_string_without_panicking() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("TripleQuoted.kt");
        fs::write(&source_file, "val text = \"\"\"unterminated").expect("write source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &source_file)
            .expect("unterminated triple quoted string should still produce a parsed file");

        assert!(parsed.primary_tree().root_node().has_error() || parsed.is_partial());
    }

    #[test]
    fn strict_parse_handles_unbalanced_braces_without_panicking() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("Braces.kt");
        fs::write(&source_file, "@Composable\nfun Broken() {\n").expect("write source");

        let mut parser = new_parser().expect("parser");
        let err = parse_kotlin_file_strict(&mut parser, &source_file)
            .expect_err("strict parse should reject partial trees");

        assert!(matches!(err, ParseKotlinFileError::ParseFailed(path) if path == source_file));
    }

    #[test]
    fn strict_parse_handles_partial_tree_without_panicking() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let source_file = tempdir.path().join("Partial.kt");
        fs::write(&source_file, "fun Broken(\n").expect("write source");

        let mut parser = new_parser().expect("parser");
        let err = parse_kotlin_file_strict(&mut parser, &source_file)
            .expect_err("strict parse should reject a parser partial tree");

        assert!(matches!(err, ParseKotlinFileError::ParseFailed(path) if path == source_file));
    }
}
