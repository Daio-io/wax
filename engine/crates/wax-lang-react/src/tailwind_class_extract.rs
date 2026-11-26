//! Tailwind token references and hard-coded styles in React class strings.

use std::collections::{BTreeMap, HashSet};

use swc_common::{BytePos, Span};
use swc_ecma_ast::{
    BinaryOp, CallExpr, Expr, ExprOrSpread, JSXAttr, JSXAttrName, JSXAttrValue, JSXExpr, Lit,
    OptCall, OptChainBase, Prop, PropName, PropOrSpread, Tpl,
};
use swc_ecma_visit::{Visit, VisitWith};
use wax_contract::{HardcodedStyleSite, TokenSite};
use wax_lang_api::TokenMatch;

use crate::component_scope::{ComponentDefinition, parent_for_span, peel_expr};
use crate::swc_parse::ParsedReactModule;
use crate::tailwind_theme::{arbitrary_style, top_level_delimiters, utility_tokens};

pub(crate) fn collect_class_style_facts(
    parsed: &ParsedReactModule,
    css_tokens: &BTreeMap<String, TokenMatch>,
    components: &[ComponentDefinition],
) -> (Vec<TokenSite>, Vec<HardcodedStyleSite>) {
    let mut collector = ClassStyleCollector {
        parsed,
        css_tokens,
        components,
        seen: HashSet::new(),
        token_sites: Vec::new(),
        hardcoded_sites: Vec::new(),
    };
    parsed.module.visit_with(&mut collector);
    (collector.token_sites, collector.hardcoded_sites)
}

struct ClassStyleCollector<'a> {
    parsed: &'a ParsedReactModule,
    css_tokens: &'a BTreeMap<String, TokenMatch>,
    components: &'a [ComponentDefinition],
    seen: HashSet<Span>,
    token_sites: Vec<TokenSite>,
    hardcoded_sites: Vec<HardcodedStyleSite>,
}

impl Visit for ClassStyleCollector<'_> {
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

impl ClassStyleCollector<'_> {
    fn collect_expr(&mut self, expr: &Expr, object_keys: bool) {
        match peel_expr(expr) {
            Expr::Lit(Lit::Str(literal)) => self.collect_string(literal.span),
            Expr::Tpl(template) => self.collect_template(template),
            Expr::Cond(conditional) => {
                self.collect_expr(&conditional.cons, object_keys);
                self.collect_expr(&conditional.alt, object_keys);
            }
            Expr::Bin(binary) if binary.op == BinaryOp::Add => {
                self.collect_concat(expr);
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

    /// `cn`/`clsx`-style objects map class keys to conditions, while
    /// `cva`/`tv` configuration objects carry classes in their values.
    fn collect_property(&mut self, property: &Prop, object_keys: bool) {
        let key = match property {
            Prop::KeyValue(property) if !object_keys => {
                self.collect_expr(&property.value, object_keys);
                return;
            }
            Prop::KeyValue(property) => &property.key,
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
        if let Some(text) = self.string_text(span) {
            self.collect_classes(span, span.lo() + BytePos(1), &text);
        }
    }

    fn string_text(&self, span: Span) -> Option<String> {
        let raw = self.parsed.source_slice_from_span(span)?;
        raw.get(1..raw.len().saturating_sub(1)).map(str::to_owned)
    }

    /// Tailwind only generates classes that appear whole in source, so each
    /// operand is scanned on its own and fragments joined across an operand
    /// boundary without whitespace are excluded, as in templates.
    fn collect_concat(&mut self, expr: &Expr) {
        fn flatten<'a>(expr: &'a Expr, parts: &mut Vec<&'a Expr>) {
            match peel_expr(expr) {
                Expr::Bin(binary) if binary.op == BinaryOp::Add => {
                    flatten(&binary.left, parts);
                    flatten(&binary.right, parts);
                }
                part => parts.push(part),
            }
        }

        let mut parts = Vec::new();
        flatten(expr, &mut parts);
        let texts = parts
            .iter()
            .map(|part| match part {
                Expr::Lit(Lit::Str(literal)) => self
                    .string_text(literal.span)
                    .map(|text| (literal.span, text)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let whitespace = |ch: char| ch.is_ascii_whitespace();
        let separated = |left: usize| {
            texts[left]
                .as_ref()
                .is_some_and(|(_, text)| text.ends_with(whitespace))
                || texts[left + 1]
                    .as_ref()
                    .is_some_and(|(_, text)| text.starts_with(whitespace))
        };
        let last = parts.len() - 1;
        for (index, part) in parts.iter().enumerate() {
            let open_start = index > 0 && !separated(index - 1);
            let open_end = index < last && !separated(index);
            match &texts[index] {
                Some((span, text)) => {
                    self.collect_bounded(*span, span.lo() + BytePos(1), text, open_start, open_end)
                }
                None if !open_start && !open_end => self.collect_expr(part, false),
                None => {}
            }
        }
    }

    fn collect_template(&mut self, template: &Tpl) {
        let whitespace = |ch: char| ch.is_ascii_whitespace();
        for (index, quasi) in template.quasis.iter().enumerate() {
            // SWC's quasi spans already exclude delimiters.
            self.collect_bounded(
                quasi.span,
                quasi.span.lo(),
                quasi.raw.as_ref(),
                index > 0,
                !quasi.tail,
            );
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

    /// Collects classes from `text`, ignoring fragments at an open edge that
    /// join a neighbouring expression without intervening whitespace.
    fn collect_bounded(
        &mut self,
        span: Span,
        start: BytePos,
        text: &str,
        open_start: bool,
        open_end: bool,
    ) {
        let whitespace = |ch: char| ch.is_ascii_whitespace();
        let from = if open_start {
            text.find(whitespace).unwrap_or(text.len())
        } else {
            0
        };
        let to = if open_end {
            text.rfind(whitespace).unwrap_or(0)
        } else {
            text.len()
        };
        if let Some(text) = text.get(from..to) {
            self.collect_classes(span, start + BytePos(from as u32), text);
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
            let (utility, modifier) = normalize_class(class);
            let tokens = utility_tokens(
                utility.strip_prefix('-').unwrap_or(utility),
                modifier,
                self.css_tokens,
            );
            let hardcoded = arbitrary_style(utility);
            if tokens.is_empty() && hardcoded.is_none() {
                continue;
            }
            let class_start = start + BytePos(class_offset as u32);
            let Some(location) = self
                .parsed
                .source_location_from_span(Span::new(class_start, class_start))
            else {
                continue;
            };
            let parent = parent_for_span(self.parsed, self.components, span);
            for token in tokens {
                self.token_sites.push(TokenSite {
                    id: format!(
                        "token.react:{}:{}:{}:{}",
                        location.file,
                        location.line,
                        location.column.unwrap_or(0),
                        token.token_id
                    ),
                    location: location.clone(),
                    token_id: token.token_id.clone(),
                    key: token.key.clone(),
                    category: token.category,
                    parent: parent.clone(),
                });
            }
            if let Some((value, category, context)) = hardcoded {
                self.hardcoded_sites.push(HardcodedStyleSite {
                    id: format!(
                        "hardcoded.react:{}:{}:{}:{category:?}",
                        location.file,
                        location.line,
                        location.column.unwrap_or(0),
                    ),
                    location,
                    value,
                    category,
                    context,
                    parent,
                });
            }
        }
    }
}

/// Strips variants and the important flag, returning the utility and its
/// optional `/` modifier.
fn normalize_class(class: &str) -> (&str, Option<&str>) {
    let class = class.strip_prefix('!').unwrap_or(class);
    let class = class.strip_suffix('!').unwrap_or(class);
    let mut start = 0;
    let mut slash = None;
    for (offset, delimiter) in top_level_delimiters(class) {
        match delimiter {
            b':' => {
                start = offset + 1;
                slash = None;
            }
            b'/' if slash.is_none() => slash = Some(offset),
            _ => {}
        }
    }
    let (utility, modifier) = match slash {
        Some(slash) => (&class[start..slash], Some(&class[slash + 1..])),
        None => (&class[start..], None),
    };
    (utility, modifier.filter(|modifier| !modifier.is_empty()))
}
