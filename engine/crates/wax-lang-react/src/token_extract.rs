//! AST-backed exact design-token reference extraction for React.

use swc_common::Spanned;
use swc_ecma_ast::Expr;
use swc_ecma_visit::{Visit, VisitWith};
use wax_contract::TokenSite;
use wax_lang_api::RegistryTokenIndex;

use crate::component_scope::{ComponentDefinition, parent_for_span, peel_expr};
use crate::swc_parse::ParsedReactModule;

/// Collects exact source-facing token references via SWC visitation.
#[must_use]
pub fn collect_token_sites(
    parsed: &ParsedReactModule,
    token_index: &RegistryTokenIndex,
    components: &[ComponentDefinition],
) -> Vec<TokenSite> {
    let mut visitor = TokenSiteVisitor {
        parsed,
        token_index,
        components,
        out: Vec::new(),
    };
    parsed.module.visit_with(&mut visitor);
    visitor.out
}

struct TokenSiteVisitor<'a> {
    parsed: &'a ParsedReactModule,
    token_index: &'a RegistryTokenIndex,
    components: &'a [ComponentDefinition],
    out: Vec<TokenSite>,
}

impl Visit for TokenSiteVisitor<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            // Descend through wrappers so matching uses the peeled expression span/text.
            Expr::Paren(_)
            | Expr::TsAs(_)
            | Expr::TsSatisfies(_)
            | Expr::TsConstAssertion(_)
            | Expr::TsTypeAssertion(_)
            | Expr::TsNonNull(_)
            | Expr::TsInstantiation(_) => {
                expr.visit_children_with(self);
            }
            Expr::Member(_) | Expr::OptChain(_) | Expr::Ident(_) => {
                self.maybe_emit(expr);
                expr.visit_children_with(self);
            }
            _ => expr.visit_children_with(self),
        }
    }
}

impl TokenSiteVisitor<'_> {
    fn maybe_emit(&mut self, expr: &Expr) {
        let peeled = peel_expr(expr);
        let span = peeled.span();
        let Some(slice) = self.parsed.source_slice_from_span(span) else {
            return;
        };
        let Some(token_match) = self.token_index.matches.get(&slice) else {
            return;
        };
        let Some(location) = self.parsed.source_location_from_span(span) else {
            return;
        };
        self.out.push(TokenSite {
            id: format!(
                "token.react:{}:{}:{}:{}",
                location.file,
                location.line,
                location.column.unwrap_or(0),
                token_match.token_id
            ),
            location,
            token_id: token_match.token_id.clone(),
            key: slice,
            category: token_match.category,
            parent: parent_for_span(self.parsed, self.components, span),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::collect_token_sites;
    use crate::component_scope::collect_component_definitions;
    use crate::registry::ReactRegistryIndex;
    use crate::swc_parse::{ParsedReactModule, ReactParseOutcome, parse_react_source_file};
    use std::collections::BTreeMap;
    use std::path::Path;
    use wax_contract::{DesignSystemToken, TokenCategory};
    use wax_lang_api::token_index;

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

    fn primary_registry() -> ReactRegistryIndex {
        let tokens = vec![DesignSystemToken {
            id: "color.primary".to_owned(),
            key: "theme.colors.primary".to_owned(),
            category: TokenCategory::Color,
            aliases: vec!["tokens[\"color.primary\"]".to_owned()],
        }];
        let index = token_index(&tokens).unwrap();
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets: BTreeMap::new(),
            component_packages: BTreeMap::new(),
            design_system_tokens: tokens,
            token_index: index,
        }
    }

    #[test]
    fn matches_exact_source_slice_including_computed_keys() {
        let hold = parse(
            r#"
            export function Card() {
                const a = theme.colors.primary;
                const b = theme?.colors.primary;
                const c = tokens["color.primary"];
                const d = theme.colors.primaryAction;
                // theme.colors.primary
                return <div>{a}{b}{c}{d}</div>;
            }
            "#,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_token_sites(&hold.parsed, &primary_registry().token_index, &components);
        let keys: Vec<_> = sites.iter().map(|site| site.key.as_str()).collect();
        assert!(keys.contains(&"theme.colors.primary"));
        assert!(keys.contains(&"tokens[\"color.primary\"]"));
        assert!(!keys.iter().any(|key| key.contains("primaryAction")));
        assert!(!keys.iter().any(|key| key.contains("?,")));
        assert_eq!(
            keys.iter()
                .filter(|key| **key == "theme.colors.primary")
                .count(),
            1,
            "optional chaining must not count as theme.colors.primary: {keys:?}"
        );
    }

    #[test]
    fn matches_parameter_default_references() {
        let hold = parse(
            r#"
            function Card({ color = theme.colors.primary }) {
                return <div>{color}</div>;
            }
            "#,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_token_sites(&hold.parsed, &primary_registry().token_index, &components);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].key, "theme.colors.primary");
        assert_eq!(
            sites[0]
                .parent
                .as_ref()
                .map(|parent| parent.symbol.as_str()),
            Some("Card")
        );
    }

    #[test]
    fn forward_ref_component_parents_inner_token() {
        let hold = parse(
            r##"
            import { forwardRef } from "react";
            export const Card = forwardRef(function Card() {
                const color = theme.colors.primary;
                return <div>{color}</div>;
            });
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_token_sites(&hold.parsed, &primary_registry().token_index, &components);
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
    fn default_exported_forward_ref_parents_token_and_style() {
        let hold = parse(
            r##"
            import { forwardRef } from "react";
            export default forwardRef(function Card() {
                const color = theme.colors.primary;
                return <div style={{ color: "#fff" }}>{color}</div>;
            });
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_token_sites(&hold.parsed, &primary_registry().token_index, &components);
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
    fn memo_comparator_token_is_not_attributed_to_card() {
        let hold = parse(
            r##"
            import { memo } from "react";
            export const Card = memo(
                () => <div />,
                () => {
                    const auditColor = theme.colors.primary;
                    return true;
                },
            );
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        let sites = collect_token_sites(&hold.parsed, &primary_registry().token_index, &components);
        assert_eq!(sites.len(), 1);
        assert!(
            sites[0].parent.is_none(),
            "comparator token must not inherit Card parent: {:?}",
            sites[0].parent
        );
    }
}
