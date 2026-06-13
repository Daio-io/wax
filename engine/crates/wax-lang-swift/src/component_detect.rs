//! Shared SwiftUI component detection helpers.

/// A SwiftUI component declaration discovered in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedComponent {
    /// Source symbol name.
    pub(crate) symbol: String,
    /// One-based source line.
    pub(crate) line: u32,
    /// One-based source column.
    pub(crate) column: u32,
}

/// Collects SwiftUI component declarations from a parsed Swift syntax tree.
pub(crate) fn collect_component_declarations(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    discovery_visibility: bool,
) -> Vec<DetectedComponent> {
    let mut components = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "class_declaration" => {
                if is_struct_declaration(node)
                    && let Some(component) =
                        component_from_type_declaration(node, source, discovery_visibility)
                {
                    components.push(component);
                }
            }
            "function_declaration" => {
                if let Some(component) =
                    component_from_function_declaration(node, source, discovery_visibility)
                {
                    components.push(component);
                }
            }
            _ => {}
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    components
}

fn component_from_type_declaration(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    discovery_visibility: bool,
) -> Option<DetectedComponent> {
    if discovery_visibility && is_private_for_discovery(node, source) {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    let name = type_declaration_name(name_node, source)?;
    if !is_pascal_case(&name) {
        return None;
    }

    if !type_inherits_view(node, source) {
        return None;
    }
    if !type_has_view_body(node, source) {
        return None;
    }

    let (line, column) = component_position(name_node);
    Some(DetectedComponent {
        symbol: name,
        line,
        column,
    })
}

fn component_from_function_declaration(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    discovery_visibility: bool,
) -> Option<DetectedComponent> {
    if discovery_visibility && is_private_for_discovery(node, source) {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    let name = function_declaration_name(name_node, source)?;
    if !is_pascal_case(&name) {
        return None;
    }

    if !function_returns_some_view(node, source) {
        return None;
    }

    let (line, column) = component_position(name_node);
    Some(DetectedComponent {
        symbol: name,
        line,
        column,
    })
}

fn is_private_for_discovery(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "modifiers" {
            continue;
        }
        let mut modifiers_cursor = child.walk();
        for modifier in child.named_children(&mut modifiers_cursor) {
            if modifier.kind() == "visibility_modifier"
                && let Ok(visibility) = modifier.utf8_text(source)
                && matches!(visibility, "private" | "fileprivate")
            {
                return true;
            }
        }
    }
    false
}

fn is_struct_declaration(node: tree_sitter::Node<'_>) -> bool {
    node.child_by_field_name("declaration_kind")
        .is_some_and(|kind| kind.kind() == "struct")
}

fn type_declaration_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    identifier_from_type_name(node, source)
}

fn function_declaration_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "simple_identifier" {
        return Some(node_text(node, source));
    }
    None
}

fn identifier_from_type_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "type_identifier" | "simple_identifier" => Some(node_text(node, source)),
        "user_type" => {
            for index in 0..node.child_count() {
                let child = node.child(index)?;
                if child.kind() == "type_identifier" {
                    return Some(node_text(child, source));
                }
            }
            None
        }
        _ => None,
    }
}

fn type_inherits_view(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if child.kind() == "class_body" {
            continue;
        }
        if child.kind() == "inheritance_specifier" && inheritance_specifier_is_view(child, source) {
            return true;
        }
    }
    false
}

fn inheritance_specifier_is_view(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "user_type"
            && type_identifier_from_user_type(child, source).as_deref() == Some("View")
        {
            return true;
        }
    }
    false
}

fn type_identifier_from_user_type(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    for index in 0..node.child_count() {
        let child = node.child(index)?;
        if child.kind() == "type_identifier" {
            return Some(node_text(child, source));
        }
    }
    None
}

fn type_has_view_body(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let Some(class_body) = node.child_by_field_name("body") else {
        return false;
    };
    for index in 0..class_body.child_count() {
        let Some(member) = class_body.child(index) else {
            continue;
        };
        if is_view_body_property(member, source) {
            return true;
        }
    }
    false
}

fn is_view_body_property(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if node.kind() != "property_declaration" {
        return false;
    }
    let Some(name_pattern) = node.child_by_field_name("name") else {
        return false;
    };
    if property_name(name_pattern, source).as_deref() != Some("body") {
        return false;
    }
    property_returns_some_view(node, source)
}

fn property_name(pattern_node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    if pattern_node.kind() == "simple_identifier" {
        return Some(node_text(pattern_node, source));
    }
    if let Some(name) = pattern_node.child_by_field_name("name")
        && name.kind() == "simple_identifier"
    {
        return Some(node_text(name, source));
    }
    for index in 0..pattern_node.child_count() {
        let child = pattern_node.child(index)?;
        if child.kind() == "simple_identifier" {
            return Some(node_text(child, source));
        }
    }
    None
}

fn property_returns_some_view(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(type_annotation) = node.child_by_field_name("type") {
        return opaque_type_is_some_view(type_annotation, source);
    }
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if (child.kind() == "type_annotation" || child.kind() == "opaque_type")
            && opaque_type_is_some_view(child, source)
        {
            return true;
        }
    }
    false
}

fn function_returns_some_view(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    node_has_opaque_view_type(node, source)
}

fn node_has_opaque_view_type(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if child.kind() == "opaque_type" && opaque_type_is_some_view(child, source) {
            return true;
        }
    }
    false
}

fn opaque_type_is_some_view(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if node.kind() == "type_annotation" {
        for index in 0..node.child_count() {
            let Some(child) = node.child(index) else {
                continue;
            };
            if child.kind() == "opaque_type" && opaque_type_is_some_view(child, source) {
                return true;
            }
        }
        return false;
    }
    if node.kind() != "opaque_type" {
        return false;
    }
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if child.kind() == "user_type"
            && type_identifier_from_user_type(child, source).as_deref() == Some("View")
        {
            return true;
        }
    }
    false
}

fn component_position(name_node: tree_sitter::Node<'_>) -> (u32, u32) {
    let pos = name_node.start_position();
    (pos.row as u32 + 1, pos.column as u32 + 1)
}

fn is_pascal_case(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn node_text(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swift_ast::new_parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = new_parser().expect("parser");
        parser.parse(source.as_bytes(), None).expect("parse")
    }

    fn symbols(source: &str, discovery_visibility: bool) -> Vec<DetectedComponent> {
        let tree = parse(source);
        collect_component_declarations(tree.root_node(), source.as_bytes(), discovery_visibility)
    }

    #[test]
    fn detects_view_struct_and_some_view_function() {
        let source = r#"
            struct ProfileCard: View {
                var body: some View { Text("Profile") }
            }

            public func PrimaryButton(title: String) -> some View {
                Button(title) {}
            }
        "#;
        let symbols = symbols(source, false);

        assert!(
            symbols
                .iter()
                .any(|component| component.symbol == "ProfileCard")
        );
        assert!(
            symbols
                .iter()
                .any(|component| component.symbol == "PrimaryButton")
        );
    }

    #[test]
    fn nested_non_view_wrapper_is_not_a_component() {
        let source = r#"
            struct Outer {
                struct Inner: View {
                    var body: some View { EmptyView() }
                }
            }
        "#;
        let symbols = symbols(source, false);

        assert!(symbols.iter().any(|component| component.symbol == "Inner"));
        assert!(!symbols.iter().any(|component| component.symbol == "Outer"));
    }

    #[test]
    fn discovery_skips_private_and_fileprivate_symbols() {
        let source = r#"
            private struct PrivateCard: View {
                var body: some View { Text("Private") }
            }
            fileprivate func FilePrivateButton() -> some View {
                Button("Nope") {}
            }
            internal struct PublicEnoughCard: View {
                var body: some View { Text("Card") }
            }
        "#;
        let symbols = symbols(source, true);

        assert!(
            !symbols
                .iter()
                .any(|component| component.symbol == "PrivateCard")
        );
        assert!(
            !symbols
                .iter()
                .any(|component| component.symbol == "FilePrivateButton")
        );
        assert!(
            symbols
                .iter()
                .any(|component| component.symbol == "PublicEnoughCard")
        );
    }

    #[test]
    fn viewmodel_inheritance_does_not_match_view() {
        let source = r#"
            struct CardViewModel: ViewModel {
                var bodyText: String = ""
            }
        "#;
        let symbols = symbols(source, false);

        assert!(
            !symbols
                .iter()
                .any(|component| component.symbol == "CardViewModel")
        );
    }

    #[test]
    fn body_text_property_does_not_count_as_swiftui_body() {
        let source = r#"
            struct Card: View {
                var bodyText: String = "label"
            }
        "#;
        let symbols = symbols(source, false);

        assert!(!symbols.iter().any(|component| component.symbol == "Card"));
    }

    #[test]
    fn discovery_ignores_private_mentions_in_comments_and_strings() {
        let source = r#"
            struct PublicCard: View {
                var body: some View {
                    Text("fileprivate helper")
                    // private implementation detail
                }
            }
        "#;
        let symbols = symbols(source, true);

        assert!(
            symbols
                .iter()
                .any(|component| component.symbol == "PublicCard")
        );
    }

    #[test]
    fn component_location_points_at_symbol_name() {
        let source = "struct ProfileCard: View {\n    var body: some View { Text(\"\") }\n}";
        let symbols = symbols(source, false);
        let card = symbols
            .iter()
            .find(|component| component.symbol == "ProfileCard")
            .expect("ProfileCard should be detected");

        assert_eq!(card.line, 1);
        assert!(
            card.column > 7,
            "column should start at symbol name, not struct keyword"
        );
    }
}
