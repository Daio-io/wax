//! AST-backed exact design-token reference extraction for React.

use std::collections::HashMap;

use swc_common::{GLOBALS, Globals, Mark, Spanned};
use swc_ecma_ast::{
    Expr, Id, Ident, ImportSpecifier, Lit, MemberExpr, MemberProp, Module, ModuleDecl, ModuleItem,
    OptChainBase,
};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::{Visit, VisitMutWith, VisitWith};
use wax_contract::TokenSite;
use wax_lang_api::{RegistryTokenIndex, TokenMatch, npm_import_package_root};

use crate::component_scope::{ComponentDefinition, parent_for_span, peel_expr};
use crate::extract::is_path_like_import;
use crate::module_graph::{ReactModuleGraph, module_export_name};
use crate::swc_parse::ParsedReactModule;

/// Collects exact source-facing token references via SWC visitation.
#[must_use]
pub fn collect_token_sites(
    parsed: &ParsedReactModule,
    token_index: &RegistryTokenIndex,
    components: &[ComponentDefinition],
    module_graph: &ReactModuleGraph,
) -> Vec<TokenSite> {
    let mut visitor = TokenSiteVisitor {
        parsed,
        token_index,
        components,
        imports: HashMap::new(),
        out: Vec::new(),
    };
    if token_index.has_scoped_tokens() {
        // Resolve a copy so JSX extraction keeps the original parse and spans.
        GLOBALS.set(&Globals::new(), || {
            let mut module = parsed.module.clone();
            module.visit_mut_with(&mut resolver(Mark::new(), Mark::new(), true));
            visitor.imports = scoped_imports(&module, module_graph);
            module.visit_with(&mut visitor);
        });
    } else {
        parsed.module.visit_with(&mut visitor);
    }
    visitor.out
}

struct TokenImport {
    package: String,
    exported_name: Option<String>,
}

fn scoped_imports(module: &Module, module_graph: &ReactModuleGraph) -> HashMap<Id, TokenImport> {
    let mut imports = HashMap::new();
    for item in &module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(declaration)) = item else {
            continue;
        };
        let Some(specifier) = declaration.src.value.as_str() else {
            continue;
        };
        if declaration.type_only
            || is_path_like_import(specifier)
            || specifier.starts_with(['.', '#'])
            || specifier.contains([':', '\\'])
            || module_graph.is_path_alias(specifier)
        {
            continue;
        }
        let package = npm_import_package_root(specifier);
        for binding in &declaration.specifiers {
            let (local, exported_name) = match binding {
                ImportSpecifier::Named(named) if !named.is_type_only => (
                    &named.local,
                    Some(
                        named
                            .imported
                            .as_ref()
                            .map_or_else(|| named.local.sym.to_string(), module_export_name),
                    ),
                ),
                ImportSpecifier::Namespace(namespace) => (&namespace.local, None),
                _ => continue,
            };
            imports.insert(
                local.to_id(),
                TokenImport {
                    package: package.clone(),
                    exported_name,
                },
            );
        }
    }
    imports
}

fn static_reference<'a>(expr: &'a Expr, members: &mut Vec<&'a str>) -> Option<&'a Ident> {
    let member = match peel_expr(expr) {
        Expr::Ident(root) => return Some(root),
        Expr::Member(member) => member,
        Expr::OptChain(chain) => match chain.base.as_ref() {
            OptChainBase::Member(member) => member,
            OptChainBase::Call(_) => return None,
        },
        _ => return None,
    };
    let MemberExpr { obj, prop, .. } = member;
    let root = static_reference(obj, members)?;
    let name = match prop {
        MemberProp::Ident(name) => name.sym.as_str(),
        MemberProp::Computed(computed) => match peel_expr(&computed.expr) {
            Expr::Lit(Lit::Str(value)) => value.value.as_str()?,
            _ => return None,
        },
        MemberProp::PrivateName(_) => return None,
    };
    if name.is_empty() || name.contains('.') {
        return None;
    }
    members.push(name);
    Some(root)
}

struct TokenSiteVisitor<'a> {
    parsed: &'a ParsedReactModule,
    token_index: &'a RegistryTokenIndex,
    components: &'a [ComponentDefinition],
    imports: HashMap<Id, TokenImport>,
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
    fn match_reference(&self, expr: &Expr, source: &str) -> Option<&TokenMatch> {
        if !self.token_index.has_scoped_tokens() {
            return self.token_index.matches.get(source);
        }
        let mut members = Vec::new();
        let Some(root) = static_reference(expr, &mut members) else {
            return self.token_index.match_reference(source, None, None);
        };
        let local_path = std::iter::once(root.sym.as_str())
            .chain(members.iter().copied())
            .collect::<Vec<_>>()
            .join(".");
        let Some(import) = self.imports.get(&root.to_id()) else {
            return self
                .token_index
                .match_reference(source, Some(&local_path), None);
        };
        let exported_path = import
            .exported_name
            .iter()
            .map(String::as_str)
            .chain(members)
            .collect::<Vec<_>>()
            .join(".");
        let origin = (!exported_path.is_empty())
            .then_some((import.package.as_str(), exported_path.as_str()));
        self.token_index
            .match_reference(source, Some(&local_path), origin)
    }

    fn maybe_emit(&mut self, expr: &Expr) {
        let peeled = peel_expr(expr);
        let span = peeled.span();
        let Some(slice) = self.parsed.source_slice_from_span(span) else {
            return;
        };
        let Some(token_match) = self.match_reference(peeled, &slice) else {
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
            key: token_match.key.clone(),
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
            value: None,
        }];
        let index = token_index(&tokens).unwrap();
        ReactRegistryIndex {
            components: Default::default(),
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
        let sites = collect_token_sites(
            &hold.parsed,
            &primary_registry().token_index,
            &components,
            &Default::default(),
        );
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
        let sites = collect_token_sites(
            &hold.parsed,
            &primary_registry().token_index,
            &components,
            &Default::default(),
        );
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
        let sites = collect_token_sites(
            &hold.parsed,
            &primary_registry().token_index,
            &components,
            &Default::default(),
        );
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
        let sites = collect_token_sites(
            &hold.parsed,
            &primary_registry().token_index,
            &components,
            &Default::default(),
        );
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
        let sites = collect_token_sites(
            &hold.parsed,
            &primary_registry().token_index,
            &components,
            &Default::default(),
        );
        assert_eq!(sites.len(), 1);
        assert!(
            sites[0].parent.is_none(),
            "comparator token must not inherit Card parent: {:?}",
            sites[0].parent
        );
    }
}
