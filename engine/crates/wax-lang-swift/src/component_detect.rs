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
            "struct_declaration" => {
                if let Some(component) =
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

    let name = type_declaration_name(node, source)?;
    if !is_pascal_case(&name) {
        return None;
    }

    let declaration_text = node_text(node, source);
    if !type_declaration_is_view(&declaration_text) {
        return None;
    }
    if !declaration_text.contains("body") || !declaration_text.contains("some View") {
        return None;
    }

    let pos = node.start_position();
    Some(DetectedComponent {
        symbol: name,
        line: pos.row as u32 + 1,
        column: pos.column as u32 + 1,
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

    let name = function_declaration_name(node, source)?;
    if !is_pascal_case(&name) {
        return None;
    }

    let declaration_text = node_text(node, source);
    if !declaration_text.contains("-> some View") {
        return None;
    }

    let pos = node.start_position();
    Some(DetectedComponent {
        symbol: name,
        line: pos.row as u32 + 1,
        column: pos.column as u32 + 1,
    })
}

fn is_private_for_discovery(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let declaration_text = node_text(node, source);
    declaration_text.starts_with("private ")
        || declaration_text.starts_with("fileprivate ")
        || declaration_text.contains(" private ")
        || declaration_text.contains(" fileprivate ")
}

fn is_struct_declaration(node: tree_sitter::Node<'_>) -> bool {
    node.child_by_field_name("declaration_kind")
        .is_some_and(|kind| kind.kind() == "struct")
}

fn type_declaration_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    identifier_from_type_name(name_node, source)
}

fn function_declaration_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    if name_node.kind() == "simple_identifier" {
        return Some(node_text(name_node, source));
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

fn type_declaration_is_view(declaration_text: &str) -> bool {
    declaration_text.contains(": View")
        || declaration_text.contains(": some View")
        || declaration_text.contains(", View")
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
        let tree = parse(source);
        let symbols = collect_component_declarations(tree.root_node(), source.as_bytes(), false);

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
        let tree = parse(source);
        let symbols = collect_component_declarations(tree.root_node(), source.as_bytes(), true);

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
}
