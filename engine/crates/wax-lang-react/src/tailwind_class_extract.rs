//! Registered Tailwind token references in React class strings.

use std::collections::{BTreeMap, HashSet};

use swc_common::{BytePos, Span, Spanned};
use swc_ecma_ast::{
    BinaryOp, CallExpr, Expr, ExprOrSpread, JSXAttr, JSXAttrName, JSXAttrValue, JSXExpr, Lit,
    OptCall, OptChainBase, Prop, PropName, PropOrSpread, Tpl,
};
use swc_ecma_visit::{Visit, VisitWith};
use wax_contract::TokenSite;
use wax_lang_api::TokenMatch;

use crate::component_scope::{ComponentDefinition, parent_for_span, peel_expr};
use crate::swc_parse::ParsedReactModule;
use crate::tailwind_theme::{top_level_delimiters, utility_token};

pub(crate) fn collect_class_token_sites(
    parsed: &ParsedReactModule,
    css_tokens: &BTreeMap<String, TokenMatch>,
    components: &[ComponentDefinition],
) -> Vec<TokenSite> {
    let mut collector = ClassTokenCollector {
        parsed,
        css_tokens,
        components,
        seen: HashSet::new(),
        sites: Vec::new(),
    };
    parsed.module.visit_with(&mut collector);
    collector.sites
}

struct ClassTokenCollector<'a> {
    parsed: &'a ParsedReactModule,
    css_tokens: &'a BTreeMap<String, TokenMatch>,
    components: &'a [ComponentDefinition],
    seen: HashSet<Span>,
    sites: Vec<TokenSite>,
}

impl Visit for ClassTokenCollector<'_> {
    fn visit_jsx_attr(&mut self, attr: &JSXAttr) {
        if matches!(&attr.name, JSXAttrName::Ident(name) if matches!(name.sym.as_ref(), "className" | "class"))
        {
            match &attr.value {
                Some(JSXAttrValue::Str(literal)) => self.collect_string(literal.span),
                Some(JSXAttrValue::JSXExprContainer(container)) => {
                    if let JSXExpr::Expr(expr) = &container.expr {
                        self.collect_expr(expr, true);
                    }
                }
                _ => {}
            }
        }
        attr.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        if let Some(object_keys) = call
            .callee
            .as_expr()
            .and_then(|callee| helper_object_keys(callee))
        {
            self.collect_call_args(&call.args, object_keys);
        }
        call.visit_children_with(self);
    }

    fn visit_opt_call(&mut self, call: &OptCall) {
        if let Some(object_keys) = helper_object_keys(&call.callee) {
            self.collect_call_args(&call.args, object_keys);
        }
        call.visit_children_with(self);
    }
}

fn helper_object_keys(callee: &Expr) -> Option<bool> {
    let Expr::Ident(callee) = callee else {
        return None;
    };
    match callee.sym.as_ref() {
        "cn" | "clsx" | "classnames" | "classNames" | "cx" => Some(true),
        "twMerge" | "twJoin" | "cva" | "tv" => Some(false),
        _ => None,
    }
}

impl ClassTokenCollector<'_> {
    fn collect_expr(&mut self, expr: &Expr, object_keys: bool) {
        match peel_expr(expr) {
            Expr::Lit(Lit::Str(literal)) => self.collect_string(literal.span),
            Expr::Tpl(template) => self.collect_template(template),
            Expr::Cond(conditional) => {
                self.collect_expr(&conditional.cons, object_keys);
                self.collect_expr(&conditional.alt, object_keys);
            }
            Expr::Bin(binary) if binary.op == BinaryOp::Add => {
                self.collect_static_concat(expr);
            }
            Expr::Bin(binary)
                if matches!(
                    binary.op,
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
                ) =>
            {
                self.collect_expr(&binary.left, object_keys);
                self.collect_expr(&binary.right, object_keys);
            }
            Expr::Array(array) => {
                for element in array.elems.iter().flatten() {
                    self.collect_expr(&element.expr, object_keys);
                }
            }
            Expr::Object(object) => {
                for property in &object.props {
                    match property {
                        PropOrSpread::Spread(spread) => {
                            self.collect_expr(&spread.expr, object_keys)
                        }
                        PropOrSpread::Prop(property) => {
                            self.collect_property(property, object_keys)
                        }
                    }
                }
            }
            Expr::Call(call) => {
                let keys = call
                    .callee
                    .as_expr()
                    .and_then(|callee| helper_object_keys(callee));
                self.collect_call_args(&call.args, keys.unwrap_or(object_keys));
            }
            Expr::OptChain(chain) => {
                if let OptChainBase::Call(call) = chain.base.as_ref() {
                    let keys = helper_object_keys(&call.callee).unwrap_or(object_keys);
                    self.collect_call_args(&call.args, keys);
                }
            }
            _ => {}
        }
    }

    fn collect_property(&mut self, property: &Prop, object_keys: bool) {
        let key = match property {
            Prop::KeyValue(property) => {
                self.collect_expr(&property.value, object_keys);
                &property.key
            }
            Prop::Method(property) => &property.key,
            Prop::Getter(property) => &property.key,
            Prop::Setter(property) => &property.key,
            _ => return,
        };
        if object_keys {
            match key {
                PropName::Str(key) => self.collect_string(key.span),
                PropName::Computed(key) => self.collect_expr(&key.expr, object_keys),
                _ => {}
            }
        }
    }

    fn collect_call_args(&mut self, args: &[ExprOrSpread], object_keys: bool) {
        for arg in args {
            self.collect_expr(&arg.expr, object_keys);
        }
    }

    fn collect_string(&mut self, span: Span) {
        let Some(raw) = self.parsed.source_slice_from_span(span) else {
            return;
        };
        if let Some(text) = raw.get(1..raw.len().saturating_sub(1)) {
            self.collect_classes(span, span.lo() + BytePos(1), text);
        }
    }

    fn collect_static_concat(&mut self, expr: &Expr) {
        fn flatten<'a>(expr: &'a Expr, parts: &mut Vec<&'a Expr>) -> bool {
            let Expr::Bin(binary) = peel_expr(expr) else {
                parts.push(peel_expr(expr));
                return matches!(peel_expr(expr), Expr::Lit(Lit::Str(_)));
            };
            if binary.op != BinaryOp::Add {
                parts.push(peel_expr(expr));
                return false;
            }
            flatten(&binary.left, parts) && flatten(&binary.right, parts)
        }

        let mut parts = Vec::new();
        if !flatten(expr, &mut parts) {
            return;
        }
        let mut combined = String::new();
        for part in parts {
            let Expr::Lit(Lit::Str(literal)) = part else {
                return;
            };
            let Some(raw) = self.parsed.source_slice_from_span(literal.span) else {
                return;
            };
            let Some(text) = raw.get(1..raw.len().saturating_sub(1)) else {
                return;
            };
            combined.push_str(text);
        }
        // A synthetic span is only used to deduplicate and attribute the
        // resulting complete classes to the concatenation expression.
        let span = expr.span();
        self.collect_classes(span, span.lo(), &combined);
    }

    fn collect_template(&mut self, template: &Tpl) {
        let whitespace = |ch: char| ch.is_ascii_whitespace();
        for (index, quasi) in template.quasis.iter().enumerate() {
            // SWC's quasi spans already exclude delimiters. Ignore fragments
            // that touch an interpolation without intervening whitespace.
            let raw = quasi.raw.as_ref();
            let start = if index == 0 {
                0
            } else {
                raw.find(whitespace).unwrap_or(raw.len())
            };
            let end = if quasi.tail {
                raw.len()
            } else {
                raw.rfind(whitespace).unwrap_or(0)
            };
            if let Some(text) = raw.get(start..end) {
                self.collect_classes(quasi.span, quasi.span.lo() + BytePos(start as u32), text);
            }
        }
        for (index, expr) in template.exprs.iter().enumerate() {
            let left = template.quasis.get(index).is_some_and(|quasi| {
                quasi.raw.is_empty() || quasi.raw.chars().last().is_some_and(whitespace)
            });
            let right = template.quasis.get(index + 1).is_some_and(|quasi| {
                quasi.raw.is_empty() || quasi.raw.chars().next().is_some_and(whitespace)
            });
            if left && right {
                self.collect_expr(expr, false);
            }
        }
    }

    fn collect_classes(&mut self, span: Span, start: BytePos, text: &str) {
        if !self.seen.insert(span) {
            return;
        }
        let mut offset = 0;
        for class in text.split(|ch: char| ch.is_ascii_whitespace()) {
            let class_offset = offset;
            offset += class.len() + 1;
            if class.is_empty() || class.contains("${") {
                continue;
            }
            let Some(token) = utility_token(normalize_class(class), self.css_tokens) else {
                continue;
            };
            let class_start = start + BytePos(class_offset as u32);
            let Some(location) = self
                .parsed
                .source_location_from_span(Span::new(class_start, class_start))
            else {
                continue;
            };
            self.sites.push(TokenSite {
                id: format!(
                    "token.react:{}:{}:{}:{}",
                    location.file,
                    location.line,
                    location.column.unwrap_or(0),
                    token.token_id
                ),
                location,
                token_id: token.token_id.clone(),
                key: token.key.clone(),
                category: token.category,
                parent: parent_for_span(self.parsed, self.components, span),
            });
        }
    }
}

fn normalize_class(class: &str) -> &str {
    let class = class.strip_prefix('!').unwrap_or(class);
    let class = class.strip_suffix('!').unwrap_or(class);
    let mut start = 0;
    let mut end = class.len();
    for (offset, delimiter) in top_level_delimiters(class) {
        match delimiter {
            b':' => {
                start = offset + 1;
                end = class.len();
            }
            b'/' if end == class.len() => end = offset,
            _ => {}
        }
    }
    let utility = &class[start..end];
    utility.strip_prefix('-').unwrap_or(utility)
}
