//! Hard-coded styling candidate extraction for React JSX `style` props.
//!
//! v1 scopes React hard-coded candidates to inline JSX object literals in
//! `style={{ ... }}` attributes (including TypeScript-wrapped objects). Broader
//! CSS-in-JS object tracking is follow-on work.

use swc_common::Spanned;
use swc_ecma_ast::{
    Expr, JSXAttrName, JSXAttrOrSpread, JSXAttrValue, JSXExpr, Lit, ObjectLit, Prop, PropName,
    PropOrSpread, Tpl,
};
use swc_ecma_visit::{Visit, VisitWith};
use wax_contract::{HardcodedStyleSite, StyleContext, TokenCategory};

use crate::component_scope::{ComponentDefinition, parent_for_span, peel_expr};
use crate::swc_parse::ParsedReactModule;

/// Collects conservative hard-coded style facts from JSX `style` object literals.
#[must_use]
pub fn collect_hardcoded_style_sites(
    parsed: &ParsedReactModule,
    components: &[ComponentDefinition],
) -> Vec<HardcodedStyleSite> {
    let mut visitor = StyleSiteVisitor {
        parsed,
        components,
        out: Vec::new(),
    };
    parsed.module.visit_with(&mut visitor);
    visitor.out
}

struct StyleSiteVisitor<'a> {
    parsed: &'a ParsedReactModule,
    components: &'a [ComponentDefinition],
    out: Vec<HardcodedStyleSite>,
}

impl Visit for StyleSiteVisitor<'_> {
    fn visit_jsx_opening_element(&mut self, node: &swc_ecma_ast::JSXOpeningElement) {
        for attr in &node.attrs {
            if let JSXAttrOrSpread::JSXAttr(attr) = attr
                && jsx_attr_name_is_style(&attr.name)
                && let Some(JSXAttrValue::JSXExprContainer(container)) = &attr.value
                && let JSXExpr::Expr(expr) = &container.expr
                && let Some(object) = unwrap_style_object(expr)
            {
                self.emit_from_object(object);
            }
        }
        node.visit_children_with(self);
    }
}

impl StyleSiteVisitor<'_> {
    fn emit_from_object(&mut self, object: &ObjectLit) {
        for prop in &object.props {
            let PropOrSpread::Prop(prop) = prop else {
                continue;
            };
            let Prop::KeyValue(key_value) = &**prop else {
                continue;
            };
            let Some(prop_name) = style_prop_name(&key_value.key) else {
                continue;
            };
            let Some((category, context)) = react_style_metadata(prop_name) else {
                continue;
            };
            if !is_hardcoded_style_value(&key_value.value) {
                continue;
            }
            let prop_span = key_value.span();
            let Some(location) = self.parsed.source_location_from_span(prop_span) else {
                continue;
            };
            let Some(value) = self.parsed.source_slice_from_span(key_value.value.span()) else {
                continue;
            };
            self.out.push(HardcodedStyleSite {
                id: format!(
                    "hardcoded.react:{}:{}:{}:{category:?}",
                    location.file,
                    location.line,
                    location.column.unwrap_or(0)
                ),
                location,
                value,
                category,
                context,
                parent: parent_for_span(self.parsed, self.components, prop_span),
            });
        }
    }
}

fn jsx_attr_name_is_style(name: &JSXAttrName) -> bool {
    match name {
        JSXAttrName::Ident(ident) => ident.sym.as_ref() == "style",
        JSXAttrName::JSXNamespacedName(_) => false,
    }
}

fn unwrap_style_object(expr: &Expr) -> Option<&ObjectLit> {
    match peel_expr(expr) {
        Expr::Object(object) => Some(object),
        _ => None,
    }
}

fn style_prop_name(name: &PropName) -> Option<&str> {
    match name {
        PropName::Ident(ident) => Some(ident.sym.as_ref()),
        PropName::Str(s) => s.value.as_str(),
        PropName::Computed(computed) => match peel_expr(&computed.expr) {
            Expr::Lit(Lit::Str(s)) => s.value.as_str(),
            _ => None,
        },
        PropName::Num(_) | PropName::BigInt(_) => None,
    }
}

fn react_style_metadata(prop_name: &str) -> Option<(TokenCategory, StyleContext)> {
    match prop_name {
        "color" | "backgroundColor" | "borderColor" => {
            Some((TokenCategory::Color, StyleContext::Color))
        }
        "padding" | "paddingTop" | "paddingRight" | "paddingBottom" | "paddingLeft"
        | "paddingInline" | "paddingBlock" | "paddingInlineStart" | "paddingInlineEnd"
        | "paddingBlockStart" | "paddingBlockEnd" => {
            Some((TokenCategory::Spacing, StyleContext::Padding))
        }
        "margin" | "marginTop" | "marginRight" | "marginBottom" | "marginLeft" | "marginInline"
        | "marginBlock" | "marginInlineStart" | "marginInlineEnd" | "marginBlockStart"
        | "marginBlockEnd" => Some((TokenCategory::Spacing, StyleContext::Margin)),
        "gap" | "rowGap" | "columnGap" => Some((TokenCategory::Spacing, StyleContext::Gap)),
        "width" => Some((TokenCategory::Spacing, StyleContext::Width)),
        "height" => Some((TokenCategory::Spacing, StyleContext::Height)),
        "fontSize" | "fontWeight" | "lineHeight" => {
            Some((TokenCategory::Typography, StyleContext::Typography))
        }
        "borderRadius" => Some((TokenCategory::Radius, StyleContext::Radius)),
        "boxShadow" | "shadow" => Some((TokenCategory::Elevation, StyleContext::Elevation)),
        _ => None,
    }
}

fn is_hardcoded_style_value(expr: &Expr) -> bool {
    match peel_expr(expr) {
        Expr::Lit(Lit::Str(_) | Lit::Num(_)) => true,
        Expr::Tpl(Tpl { exprs, .. }) => exprs.is_empty(),
        Expr::Unary(unary) if matches!(unary.op, swc_ecma_ast::UnaryOp::Minus) => {
            matches!(&*unary.arg, Expr::Lit(Lit::Num(_)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::collect_hardcoded_style_sites;
    use crate::component_scope::collect_component_definitions;
    use crate::swc_parse::{ParsedReactModule, ReactParseOutcome, parse_react_source_file};
    use std::path::Path;
    use wax_contract::TokenCategory;

    struct Hold {
        _root: tempfile::TempDir,
        parsed: ParsedReactModule,
    }

    fn parse(source: &str) -> Hold {
        let root = tempfile::tempdir().expect("tempdir");
        let relative = Path::new("src/File.tsx");
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        let parsed = match parse_react_source_file(root.path(), relative).unwrap() {
            ReactParseOutcome::Parsed(parsed) => parsed,
            ReactParseOutcome::Failed(diag) => panic!("{diag:?}"),
        };
        Hold {
            _root: root,
            parsed,
        }
    }

    #[test]
    fn emits_ts_wrapped_quoted_and_computed_keys() {
        let hold = parse(
            r##"
            export const Styled = () => (
                <div style={{ padding: 8 } as React.CSSProperties} />
            );
            export const Quoted = () => <div style={{ "color": "#fff" }} />;
            export const Computed = () => <div style={{ ["color"]: "#abc" }} />;
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_hardcoded_style_sites(&hold.parsed, &components);
        assert!(
            sites
                .iter()
                .any(|site| site.category == TokenCategory::Spacing && site.value == "8")
        );
        assert!(
            sites
                .iter()
                .any(|site| site.category == TokenCategory::Color && site.value == "\"#fff\"")
        );
        assert!(
            sites
                .iter()
                .any(|site| site.category == TokenCategory::Color && site.value == "\"#abc\"")
        );
        assert!(sites.iter().any(|site| {
            site.parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "Styled")
        }));
    }

    #[test]
    fn does_not_emit_css_in_js_object_variable_in_v1() {
        let hold = parse(
            r##"
            const styles = { color: "#fff" };
            function Card() {
                return <div style={styles} />;
            }
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_hardcoded_style_sites(&hold.parsed, &components);
        assert!(
            sites.is_empty(),
            "v1 only scans inline style object literals, got {sites:?}"
        );
    }

    #[test]
    fn forward_ref_styles_get_parent() {
        let hold = parse(
            r##"
            import { forwardRef } from "react";
            export const Card = forwardRef(function Card() {
                return <div style={{ color: "#fff" }} />;
            });
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_hardcoded_style_sites(&hold.parsed, &components);
        assert_eq!(sites.len(), 1);
        assert_eq!(
            sites[0]
                .parent
                .as_ref()
                .map(|parent| parent.symbol.as_str()),
            Some("Card")
        );
    }

    #[test]
    fn default_exported_forward_ref_styles_get_parent() {
        let hold = parse(
            r##"
            import { forwardRef } from "react";
            export default forwardRef(function Card() {
                return <div style={{ color: "#fff" }} />;
            });
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_hardcoded_style_sites(&hold.parsed, &components);
        assert_eq!(sites.len(), 1);
        assert_eq!(
            sites[0]
                .parent
                .as_ref()
                .map(|parent| parent.symbol.as_str()),
            Some("Card")
        );
    }
}
