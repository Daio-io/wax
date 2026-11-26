use std::collections::{BTreeSet, HashMap};

use serde_json::Map;
use swc_common::Spanned;
use swc_ecma_ast::{
    Callee, Decl, Expr, Id, ImportSpecifier, ModuleDecl, ModuleItem, ObjectLit, Pat, Prop,
    PropName, PropOrSpread, TsEntityName, TsType, TsTypeQueryExpr, VarDeclKind,
};
use wax_contract::{RegistryFieldGap, RegistryParameter, RegistryParameterTypeKind};

use super::{module_declarations, parameter_description, parameter_gap};
use crate::component_detect::module_export_name;
use crate::swc_parse::ParsedReactModule;

#[derive(Default)]
struct VariantImports {
    config_argument_by_factory: HashMap<Id, usize>,
    props: BTreeSet<Id>,
}

pub(super) struct VariantDefinitions {
    props: BTreeSet<Id>,
    by_binding: HashMap<Id, VariantFacts>,
}

fn collect_variant_imports(parsed: &ParsedReactModule) -> VariantImports {
    let mut bindings = VariantImports::default();
    for item in &parsed.module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            continue;
        };
        let package = import.src.value.to_string_lossy();
        if !matches!(
            package.as_ref(),
            "class-variance-authority" | "tailwind-variants"
        ) {
            continue;
        }
        for specifier in &import.specifiers {
            let ImportSpecifier::Named(named) = specifier else {
                continue;
            };
            let name = named
                .imported
                .as_ref()
                .map_or_else(|| named.local.sym.to_string(), module_export_name);
            if name == "VariantProps" {
                bindings.props.insert(named.local.to_id());
            } else if !import.type_only && !named.is_type_only {
                let config_argument = match (package.as_ref(), name.as_str()) {
                    ("class-variance-authority", "cva") => 1,
                    ("tailwind-variants", "tv") => 0,
                    _ => continue,
                };
                bindings
                    .config_argument_by_factory
                    .insert(named.local.to_id(), config_argument);
            }
        }
    }
    bindings
}

pub(super) fn collect_variant_definitions(parsed: &ParsedReactModule) -> VariantDefinitions {
    let imports = collect_variant_imports(parsed);
    let by_binding = module_declarations(parsed)
        .filter_map(|decl| match decl {
            Decl::Var(variable) if variable.kind == VarDeclKind::Const => Some(&variable.decls),
            _ => None,
        })
        .flatten()
        .filter_map(|declarator| {
            let Pat::Ident(binding) = &declarator.name else {
                return None;
            };
            let Expr::Call(call) = declarator.init.as_deref()? else {
                return None;
            };
            let Callee::Expr(callee) = &call.callee else {
                return None;
            };
            let Expr::Ident(callee) = callee.as_ref() else {
                return None;
            };
            let config_argument = *imports.config_argument_by_factory.get(&callee.to_id())?;
            let args_through_config = call.args.get(..=config_argument)?;
            if args_through_config.iter().any(|arg| arg.spread.is_some()) {
                return None;
            }
            let Expr::Object(config) = args_through_config.last()?.expr.as_ref() else {
                return None;
            };
            let Expr::Object(variants) = object_property(config, "variants")? else {
                return None;
            };
            let defaults = match object_property(config, "defaultVariants") {
                Some(Expr::Object(defaults)) => Some(defaults),
                _ => None,
            };
            let mut facts = VariantFacts::default();
            for entry in &variants.props {
                match variant_parameter(parsed, entry, defaults) {
                    Some(parameter) => facts.parameters.push(parameter),
                    None => {
                        let name = variant_entry_name(parsed, entry);
                        facts.gaps.push(parameter_gap(format!(
                            "variant `{name}` is not a literal object"
                        )));
                    }
                }
            }
            Some((binding.id.to_id(), facts))
        })
        .collect();
    VariantDefinitions {
        props: imports.props,
        by_binding,
    }
}

#[derive(Default)]
pub(super) struct VariantFacts {
    pub(super) parameters: Vec<RegistryParameter>,
    pub(super) gaps: Vec<RegistryFieldGap>,
}

fn variant_parameter(
    parsed: &ParsedReactModule,
    entry: &PropOrSpread,
    defaults: Option<&ObjectLit>,
) -> Option<RegistryParameter> {
    let property = entry.as_prop()?.as_key_value()?;
    let name = literal_property_name(&property.key)?;
    let Expr::Object(values) = property.value.as_ref() else {
        return None;
    };
    let mut allowed_values = Vec::new();
    for entry in &values.props {
        let value = property_name(entry.as_prop()?)?;
        if !allowed_values.contains(&value) {
            allowed_values.push(value);
        }
    }
    if allowed_values.is_empty() {
        return None;
    }
    let type_kind = if allowed_values
        .iter()
        .all(|value| matches!(value.as_str(), "true" | "false"))
    {
        allowed_values.clear();
        RegistryParameterTypeKind::Boolean
    } else {
        RegistryParameterTypeKind::Enum
    };
    let default = defaults
        .and_then(|defaults| object_property(defaults, &name))
        .filter(|value| matches!(value, Expr::Lit(_)))
        .and_then(|value| parsed.source_slice_from_span(value.span()))
        .map(|source| source.trim().to_owned());
    Some(RegistryParameter {
        name,
        type_text: None,
        package: None,
        type_kind: Some(type_kind),
        allowed_values,
        description: parameter_description(parsed, property.span()),
        default,
        required: Some(false),
        extra: Map::new(),
    })
}

fn variant_entry_name(parsed: &ParsedReactModule, entry: &PropOrSpread) -> String {
    if let Some(prop) = entry.as_prop() {
        if let Some(name) = property_name(prop) {
            return name;
        }
        if let Some(property) = prop.as_key_value() {
            return parsed
                .source_slice_from_span(property.key.span())
                .unwrap_or_default()
                .trim()
                .to_owned();
        }
    }
    parsed
        .source_slice_from_span(entry.span())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn property_name(property: &Prop) -> Option<String> {
    let key = match property {
        Prop::Shorthand(ident) => return Some(ident.sym.to_string()),
        Prop::Assign(property) => return Some(property.key.sym.to_string()),
        Prop::KeyValue(property) => &property.key,
        Prop::Getter(property) => &property.key,
        Prop::Setter(property) => &property.key,
        Prop::Method(property) => &property.key,
    };
    literal_property_name(key)
}

fn literal_property_name(key: &PropName) -> Option<String> {
    let name = match key {
        PropName::Ident(ident) => ident.sym.to_string(),
        PropName::Str(value) => value.value.to_string_lossy().into_owned(),
        PropName::Num(value) => value.value.to_string(),
        _ => return None,
    };
    (!name.trim().is_empty()).then_some(name)
}

fn object_property<'a>(object: &'a ObjectLit, name: &str) -> Option<&'a Expr> {
    for entry in object.props.iter().rev() {
        let Some(property) = entry.as_prop() else {
            // A later spread may replace any property before it.
            return None;
        };
        if property_name(property).as_deref() == Some(name) {
            return Some(property.as_key_value()?.value.as_ref());
        }
    }
    None
}

pub(super) fn variant_facts<'a>(
    ty: &TsType,
    variants: &'a VariantDefinitions,
) -> Option<&'a VariantFacts> {
    let TsType::TsTypeRef(reference) = ty else {
        return None;
    };
    let TsEntityName::Ident(ident) = &reference.type_name else {
        return None;
    };
    if !variants.props.contains(&ident.to_id()) {
        return None;
    }
    let [argument] = reference.type_params.as_ref()?.params.as_slice() else {
        return None;
    };
    let TsType::TsTypeQuery(query) = argument.as_ref() else {
        return None;
    };
    let TsTypeQueryExpr::TsEntityName(TsEntityName::Ident(binding)) = &query.expr_name else {
        return None;
    };
    variants.by_binding.get(&binding.to_id())
}
