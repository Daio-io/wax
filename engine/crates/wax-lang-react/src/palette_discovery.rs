//! Color tokens proven by public React palette declarations.

use std::collections::{BTreeMap, BTreeSet};

use swc_common::Spanned;
use swc_ecma_ast::{
    Decl, Expr, KeyValueProp, Lit, ModuleDecl, ModuleItem, Pat, Prop, PropName, PropOrSpread, Stmt,
    VarDeclKind,
};
use wax_contract::{
    RegistryDetails, RegistryDocument, RegistryError, RegistryExtractionStatus,
    RegistrySourceEvidence, RegistryToken, TokenCategory,
};

use wax_lang_api::{
    DISCOVERED_LITERAL_VALUE, discovered_token_marker, refresh_discovered_token_value,
};

use crate::discover::{DamagedReactSources, ParsedReactSourceFile, ReactDiscoverError};
use crate::public_exports::PublicExports;
use crate::swc_parse::ParsedReactModule;

pub(crate) fn discover_palette_tokens(
    document: &mut RegistryDocument,
    seeds: &[RegistryToken],
    exports: &PublicExports<'_>,
    files: &[ParsedReactSourceFile],
    damaged: &DamagedReactSources,
) -> Result<(), ReactDiscoverError> {
    let palettes: BTreeMap<_, _> = files
        .iter()
        .map(|file| {
            (
                file.parsed.file.as_path(),
                palette_declarations(&file.parsed),
            )
        })
        .collect();
    let mut tokens = Vec::new();
    for binding in &exports.bindings {
        let Some(properties) =
            palettes[binding.declaration_path].get(binding.declaration_name.as_str())
        else {
            continue;
        };
        for (property, value, evidence) in properties {
            let key = format!("{}.{property}", binding.symbol);
            let id = palette_id(binding.package, &key);
            tokens.push(RegistryToken {
                id,
                key,
                package: binding.package.map(str::to_owned),
                category: TokenCategory::Color,
                aliases: Vec::new(),
                value: Some((*value).to_owned()),
                css_variants: Vec::new(),
                metadata: None,
                details: Some(RegistryDetails {
                    extra: [(DISCOVERED_LITERAL_VALUE.to_owned(), (*value).into())]
                        .into_iter()
                        .collect(),
                    source_evidence: Some(evidence.clone()),
                    ..complete_details()
                }),
                extra: Default::default(),
            });
        }
    }
    tokens.sort_by(|left, right| (&left.package, &left.key).cmp(&(&right.package, &right.key)));
    tokens.dedup_by(|right, left| right.package == left.package && right.key == left.key);
    let matches = match_seeds(seeds, &tokens)?;
    let matched_seeds: BTreeSet<_> = matches.iter().flatten().copied().collect();
    for (index, (token, seed)) in document.tokens.iter_mut().zip(seeds).enumerate() {
        if !matched_seeds.contains(&index) {
            retain_unobserved_token(token, seed, exports, damaged);
        }
    }
    for (observed, matched) in tokens.into_iter().zip(matches) {
        if let Some(index) = matched {
            let token = &mut document.tokens[index];
            let prior_marker = discovered_token_marker(seeds[index].details.as_ref());
            if let Some(evidence) = observed.details.and_then(|details| details.source_evidence) {
                apply_token_evidence(token, evidence);
            }
            refresh_discovered_token_value(
                &mut token.value,
                prior_marker,
                token.details.as_mut(),
                observed.value.as_deref(),
            );
        } else {
            document.tokens.push(observed);
        }
    }
    Ok(())
}

fn palette_id(package: Option<&str>, key: &str) -> String {
    match package {
        Some(package) => format!("token:{package}#{key}"),
        None => format!("token:{key}"),
    }
}

fn retain_unobserved_token(
    token: &mut RegistryToken,
    seed: &RegistryToken,
    exports: &PublicExports<'_>,
    damaged: &DamagedReactSources,
) {
    let package = seed.package.as_deref();
    let prior = seed.details.as_ref();
    if let Some(prior) = prior {
        let was_generated = prior.extra.contains_key(DISCOVERED_LITERAL_VALUE)
            || (prior.source_evidence.is_some() && seed.id == palette_id(package, &seed.key));
        let declaration_damaged = prior
            .source_evidence
            .as_ref()
            .is_some_and(|evidence| damaged.contains_key(&evidence.path));
        if was_generated || declaration_damaged {
            let mut details = prior.clone();
            if declaration_damaged || exports.is_damaged(package, &seed.key) {
                details.status = RegistryExtractionStatus::Stale;
            } else if damaged.is_empty() {
                details.status = RegistryExtractionStatus::NotObserved;
            }
            token.details = Some(details);
        }
    }
    refresh_discovered_token_value(
        &mut token.value,
        discovered_token_marker(prior),
        token.details.as_mut(),
        None,
    );
}

fn match_seeds(
    seeds: &[RegistryToken],
    observations: &[RegistryToken],
) -> Result<Vec<Option<usize>>, ReactDiscoverError> {
    let mut seeds_by_key = BTreeMap::<_, Vec<_>>::new();
    for (index, seed) in seeds.iter().enumerate() {
        seeds_by_key
            .entry(seed.key.as_str())
            .or_default()
            .push((seed.package.as_deref(), index));
    }
    let mut observation_counts = BTreeMap::new();
    for token in observations {
        *observation_counts.entry(token.key.as_str()).or_insert(0) += 1;
    }
    observations
        .iter()
        .map(|token| {
            let key = token.key.as_str();
            let package = token.package.as_deref();
            let Some(seeds) = seeds_by_key.get(key) else {
                return Ok(None);
            };
            let has_scoped = seeds.iter().any(|(candidate, _)| candidate.is_some());
            let has_exact = seeds.iter().any(|(candidate, _)| *candidate == package);
            let partition = if has_exact {
                package
            } else if observation_counts[key] == 1 && !has_scoped {
                None
            } else {
                return Ok(None);
            };
            let mut matches = seeds
                .iter()
                .filter(|(candidate, _)| *candidate == partition);
            let first = matches.next();
            if matches.next().is_some() {
                return Err(invalid_token(format!(
                    "multiple seed tokens match {}",
                    token.id
                )));
            }
            Ok(first.map(|(_, index)| *index))
        })
        .collect()
}

pub(super) fn invalid_token(message: String) -> ReactDiscoverError {
    ReactDiscoverError::InvalidRegistry(RegistryError::ContractViolation {
        field: "tokens".to_owned(),
        message,
    })
}

pub(super) fn apply_token_evidence(
    token: &mut RegistryToken,
    mut evidence: RegistrySourceEvidence,
) {
    let details = token.details.get_or_insert_with(complete_details);
    if let Some(previous) = &details.source_evidence {
        evidence.extra.clone_from(&previous.extra);
    }
    details.source_evidence = Some(evidence);
    details.status = RegistryExtractionStatus::Complete;
}

fn complete_details() -> RegistryDetails {
    RegistryDetails {
        status: RegistryExtractionStatus::Complete,
        source_evidence: None,
        documentation: None,
        signature: None,
        deprecation: None,
        attachments: Vec::new(),
        field_gaps: Vec::new(),
        best_practices: Vec::new(),
        extra: Default::default(),
    }
}

type PaletteProperty<'a> = (&'a str, &'a str, RegistrySourceEvidence);

fn palette_declarations<'a>(
    parsed: &'a ParsedReactModule,
) -> BTreeMap<&'a str, Vec<PaletteProperty<'a>>> {
    let mut declarations = BTreeMap::new();
    for item in &parsed.module.body {
        let declaration = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => &export.decl,
            ModuleItem::Stmt(Stmt::Decl(declaration)) => declaration,
            _ => continue,
        };
        let Decl::Var(variable) = declaration else {
            continue;
        };
        if variable.kind != VarDeclKind::Const || variable.declare {
            continue;
        }
        for declaration in &variable.decls {
            if let (Pat::Ident(name), Some(initializer)) = (&declaration.name, &declaration.init)
                && let Some(properties) = palette_properties(initializer, parsed)
            {
                declarations.insert(name.id.sym.as_ref(), properties);
            }
        }
    }
    declarations
}

fn palette_properties<'a>(
    mut initializer: &'a Expr,
    parsed: &ParsedReactModule,
) -> Option<Vec<PaletteProperty<'a>>> {
    let object = loop {
        initializer = match initializer {
            Expr::Paren(wrapper) => &wrapper.expr,
            Expr::TsAs(wrapper) => &wrapper.expr,
            Expr::TsConstAssertion(wrapper) => &wrapper.expr,
            Expr::TsTypeAssertion(wrapper) => &wrapper.expr,
            Expr::TsSatisfies(wrapper) => &wrapper.expr,
            Expr::Object(object) => break object,
            _ => return None,
        };
    };
    let mut names = BTreeSet::new();
    let mut properties = Vec::new();
    for property in &object.props {
        let PropOrSpread::Prop(property) = property else {
            return None;
        };
        let Prop::KeyValue(pair) = property.as_ref() else {
            return None;
        };
        let name = match &pair.key {
            PropName::Ident(name) => name.sym.as_ref(),
            PropName::Str(name) => name.value.as_str()?,
            _ => return None,
        };
        if !names.insert(name) {
            return None;
        }
        if let Some(property) = literal_property(parsed, name, pair) {
            properties.push(property);
        }
    }
    Some(properties)
}

fn literal_property<'a>(
    parsed: &ParsedReactModule,
    name: &'a str,
    pair: &'a KeyValueProp,
) -> Option<PaletteProperty<'a>> {
    let Expr::Lit(Lit::Str(literal)) = pair.value.as_ref() else {
        return None;
    };
    let value = literal
        .value
        .as_str()
        .filter(|value| color_literal(value))?;
    let (start_line, end_line) = parsed.source_lines_from_span(pair.span())?;
    Some((
        name,
        value,
        RegistrySourceEvidence {
            path: parsed.file.to_string_lossy().replace('\\', "/"),
            start_line: Some(start_line),
            end_line: Some(end_line),
            extra: Default::default(),
        },
    ))
}

fn color_literal(value: &str) -> bool {
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8)
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    let Some(p3) = value
        .strip_prefix("color(display-p3 ")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    let channels = p3.split_once('/');
    let (channels, alpha) = channels.map_or((p3, None), |(channels, alpha)| {
        (channels, Some(alpha.trim()))
    });
    let channels = channels.split_whitespace();
    channels.clone().count() == 3
        && channels.into_iter().all(unit_decimal)
        && alpha.is_none_or(unit_decimal)
}

fn unit_decimal(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'+' | b'-'))
        && value
            .parse::<f64>()
            .is_ok_and(|number| (0.0..=1.0).contains(&number))
}
