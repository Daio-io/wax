//! Public entry points and export traversal for React registry discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::Value;
use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    DefaultDecl, ExportSpecifier, Expr, ImportSpecifier, ModuleDecl, ModuleItem, Stmt,
};
use wax_contract::{Diagnostic, DiagnosticSeverity};

use crate::component_detect::{
    class_returns_jsx, function_returns_jsx, is_pascal_case, module_export_name,
};

use crate::discover::{
    ParsedReactSourceFile, default_expression_component, local_component_bindings,
    react_wrapper_callees,
};
use crate::module_graph::{
    declared_names, normalize_relative_path, resolve_path_with_extensions_and_index,
};
use crate::swc_parse::ParsedReactModule;

#[derive(Debug)]
pub(crate) struct PackageManifest {
    pub name: String,
    directory: PathBuf,
    json: Value,
}

pub(crate) fn package_for_file(
    repo_root: &Path,
    file: &Path,
    manifests: &mut BTreeMap<PathBuf, Option<Rc<PackageManifest>>>,
) -> Option<Rc<PackageManifest>> {
    file.parent()?.ancestors().find_map(|directory| {
        manifests
            .entry(directory.to_path_buf())
            .or_insert_with(|| {
                let contents =
                    fs::read_to_string(repo_root.join(directory).join("package.json")).ok()?;
                let json: Value = serde_json::from_str(&contents).ok()?;
                let name = json.get("name")?.as_str()?.to_owned();
                (!name.is_empty()).then(|| {
                    Rc::new(PackageManifest {
                        name,
                        directory: directory.to_path_buf(),
                        json,
                    })
                })
            })
            .clone()
    })
}

fn entry_modules<'a>(
    repo_root: &Path,
    roots: &[PathBuf],
    files: &'a [ParsedReactSourceFile],
) -> Vec<&'a ParsedReactSourceFile> {
    let roots: BTreeSet<_> = roots
        .iter()
        .filter_map(|root| normalize_relative_path(root.strip_prefix(repo_root).ok()?))
        .collect();
    let mut partitions = BTreeMap::<_, Vec<_>>::new();
    for file in files {
        partitions
            .entry(file.package.as_ref().map(|package| &package.directory))
            .or_default()
            .push(file);
    }
    let mut entries = BTreeMap::new();
    for partition in partitions.values() {
        let known_files = partition
            .iter()
            .map(|file| file.parsed.file.clone())
            .collect();
        let package = partition.first().and_then(|file| file.package.as_deref());
        if let Some(entry) = package.and_then(|package| package_entry(package, &known_files)) {
            entries.extend(
                partition
                    .iter()
                    .filter(|file| file.parsed.file == entry)
                    .map(|file| (&file.parsed.file, *file)),
            );
            continue;
        }
        for root in &roots {
            let root = package
                .filter(|package| package.directory.starts_with(root))
                .map_or(root, |package| &package.directory);
            let index = ["tsx", "ts", "jsx", "js"].iter().find_map(|extension| {
                let index = root.join("index").with_extension(extension);
                known_files.get(&index)
            });
            for file in partition {
                let path = &file.parsed.file;
                if index.map_or_else(|| path.starts_with(root), |index| path == index) {
                    entries.insert(path, *file);
                }
            }
        }
    }
    entries.into_values().collect()
}

fn package_entry(package: &PackageManifest, files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let exports = package.json.get("exports").and_then(|exports| {
        exports.get(".").or_else(|| {
            exports
                .as_object()
                .is_none_or(|object| object.keys().all(|key| !key.starts_with('.')))
                .then_some(exports)
        })
    });
    exports
        .into_iter()
        .chain(
            ["source", "module", "main"]
                .iter()
                .filter_map(|key| package.json.get(key)),
        )
        .find_map(|target| entry_target(target, &package.directory, files))
}

fn entry_target(target: &Value, directory: &Path, files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    match target {
        Value::String(target) if !target.contains('*') => {
            let path = normalize_relative_path(&directory.join(target))?;
            resolve_discovery_path(&path, files)
        }
        Value::Array(targets) => targets
            .iter()
            .find_map(|target| entry_target(target, directory, files)),
        Value::Object(conditions) => ["source", "import", "default", "require"]
            .into_iter()
            .filter_map(|key| conditions.get(key))
            .chain(conditions.iter().filter_map(|(key, value)| {
                (!matches!(key.as_str(), "source" | "import" | "default" | "require"))
                    .then_some(value)
            }))
            .find_map(|target| entry_target(target, directory, files)),
        _ => None,
    }
}

pub(crate) struct PublicBinding<'a> {
    pub symbol: String,
    pub package: Option<&'a str>,
    pub declaration_path: &'a Path,
    pub declaration_name: String,
}

enum Target {
    Local(String),
    Remote { edge: Edge, name: Option<String> },
}

struct Edge {
    source: String,
    span: Span,
    path: Option<PathBuf>,
}

impl Edge {
    fn new(
        record: &ModuleExports<'_>,
        source: String,
        span: Span,
        files: &BTreeSet<PathBuf>,
    ) -> Self {
        let path = if is_relative_specifier(&source) {
            record
                .parsed
                .file
                .parent()
                .and_then(|directory| normalize_relative_path(&directory.join(&source)))
                .and_then(|path| resolve_discovery_path(&path, files))
        } else {
            None
        };
        Self { source, span, path }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin<'files, 'graph> {
    Declaration {
        module: &'files Path,
        name: &'graph str,
    },
    Namespace(&'files Path),
}

struct ModuleExports<'a> {
    parsed: &'a ParsedReactModule,
    locals: BTreeSet<String>,
    components: BTreeSet<String>,
    default_name: Option<String>,
    imports: BTreeMap<String, Target>,
    exports: BTreeMap<String, Target>,
    stars: Vec<Edge>,
}

impl<'a> ModuleExports<'a> {
    fn new(file: &'a ParsedReactSourceFile, files: &BTreeSet<PathBuf>) -> Self {
        let items = &file.parsed.module.body;
        let wrappers = react_wrapper_callees(items);
        let mut record = Self {
            parsed: &file.parsed,
            locals: BTreeSet::new(),
            components: local_component_bindings(items, &wrappers),
            default_name: None,
            imports: BTreeMap::new(),
            exports: BTreeMap::new(),
            stars: Vec::new(),
        };
        for item in items {
            match item {
                ModuleItem::Stmt(Stmt::Decl(decl)) => record.locals.extend(declared_names(decl)),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => {
                    for name in declared_names(&export.decl) {
                        record.locals.insert(name.clone());
                        record.exports.insert(name.clone(), Target::Local(name));
                    }
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(export)) => {
                    let (ident, component) = match &export.decl {
                        DefaultDecl::Fn(function) => {
                            (&function.ident, function_returns_jsx(&function.function))
                        }
                        DefaultDecl::Class(class) => {
                            (&class.ident, class_returns_jsx(&class.class))
                        }
                        DefaultDecl::TsInterfaceDecl(_) => continue,
                    };
                    let name = ident
                        .as_ref()
                        .map_or_else(|| "default".to_owned(), |ident| ident.sym.to_string());
                    record.locals.insert(name.clone());
                    if component {
                        record.components.insert(name.clone());
                    }
                    record
                        .exports
                        .insert("default".to_owned(), Target::Local(name));
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(export)) => {
                    let name = if let Expr::Ident(ident) = &*export.expr {
                        ident.sym.to_string()
                    } else {
                        record.locals.insert("default".to_owned());
                        record.default_name = default_expression_component(
                            &export.expr,
                            &record.components,
                            &wrappers,
                        );
                        if record.default_name.is_some() {
                            record.components.insert("default".to_owned());
                        }
                        "default".to_owned()
                    };
                    record
                        .exports
                        .insert("default".to_owned(), Target::Local(name));
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export)) if !export.type_only => {
                    for specifier in &export.specifiers {
                        let (public_name, name) = match specifier {
                            ExportSpecifier::Named(named) if !named.is_type_only => {
                                let name = module_export_name(&named.orig);
                                let public_name = named
                                    .exported
                                    .as_ref()
                                    .map_or_else(|| name.clone(), module_export_name);
                                (public_name, Some(name))
                            }
                            ExportSpecifier::Namespace(namespace) => {
                                (module_export_name(&namespace.name), None)
                            }
                            ExportSpecifier::Default(default) => {
                                (default.exported.sym.to_string(), Some("default".to_owned()))
                            }
                            _ => continue,
                        };
                        let target = if let Some(src) = &export.src {
                            Target::Remote {
                                edge: Edge::new(
                                    &record,
                                    src.value.to_string_lossy().into_owned(),
                                    specifier.span(),
                                    files,
                                ),
                                name,
                            }
                        } else if let Some(name) = name {
                            Target::Local(name)
                        } else {
                            continue;
                        };
                        record.exports.insert(public_name, target);
                    }
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export)) if !export.type_only => {
                    record.stars.push(Edge::new(
                        &record,
                        export.src.value.to_string_lossy().into_owned(),
                        export.span,
                        files,
                    ));
                }
                ModuleItem::ModuleDecl(ModuleDecl::Import(import)) if !import.type_only => {
                    for specifier in &import.specifiers {
                        let (local, name) = match specifier {
                            ImportSpecifier::Named(named) if !named.is_type_only => {
                                let local = named.local.sym.to_string();
                                let name = named
                                    .imported
                                    .as_ref()
                                    .map_or_else(|| local.clone(), module_export_name);
                                (local, Some(name))
                            }
                            ImportSpecifier::Default(default) => {
                                (default.local.sym.to_string(), Some("default".to_owned()))
                            }
                            ImportSpecifier::Namespace(namespace) => {
                                (namespace.local.sym.to_string(), None)
                            }
                            _ => continue,
                        };
                        record.imports.insert(
                            local,
                            Target::Remote {
                                edge: Edge::new(
                                    &record,
                                    import.src.value.to_string_lossy().into_owned(),
                                    specifier.span(),
                                    files,
                                ),
                                name,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        record
    }
}

pub(crate) fn public_bindings<'a>(
    repo_root: &Path,
    roots: &[PathBuf],
    files: &'a [ParsedReactSourceFile],
    damaged_paths: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<PublicBinding<'a>> {
    let known_files = files
        .iter()
        .map(|file| file.parsed.file.clone())
        .chain(damaged_paths.iter().map(PathBuf::from))
        .collect();
    let modules = files
        .iter()
        .map(|file| {
            (
                file.parsed.file.as_path(),
                ModuleExports::new(file, &known_files),
            )
        })
        .collect();
    let mut traversal = ExportTraversal {
        modules: &modules,
        diagnostics: BTreeMap::new(),
        bindings: Vec::new(),
        active_namespaces: BTreeSet::new(),
        resolved: BTreeMap::new(),
    };
    for entry in entry_modules(repo_root, roots, files) {
        traversal.expand_namespace(
            &entry.parsed.file,
            "",
            entry.package.as_ref().map(|package| package.name.as_str()),
        );
    }
    diagnostics.extend(traversal.diagnostics.into_values());
    traversal.bindings
}

struct ExportTraversal<'graph, 'files> {
    modules: &'graph BTreeMap<&'files Path, ModuleExports<'files>>,
    diagnostics: BTreeMap<(&'files Path, u32, String), Diagnostic>,
    bindings: Vec<PublicBinding<'files>>,
    active_namespaces: BTreeSet<&'files Path>,
    resolved: BTreeMap<(&'files Path, &'graph str), Origin<'files, 'graph>>,
}

#[derive(Debug)]
struct AmbiguousExport<'a> {
    module: &'a Path,
    name: &'a str,
}

impl<'graph, 'files> ExportTraversal<'graph, 'files> {
    fn expand_namespace(
        &mut self,
        module: &'files Path,
        prefix: &str,
        package: Option<&'files str>,
    ) {
        if !self.active_namespaces.insert(module) {
            return;
        }
        let mut names = BTreeSet::new();
        self.export_names(module, true, &mut names, &mut BTreeSet::new());
        for name in names {
            match self.resolve_export(module, name, &mut BTreeSet::new()) {
                Ok(Some(Origin::Namespace(target))) => {
                    self.expand_namespace(target, &format!("{prefix}{name}."), package);
                }
                Ok(Some(Origin::Declaration {
                    module,
                    name: local,
                })) => {
                    let origin_module = &self.modules[module];
                    // Standalone default exports keep the legacy component name.
                    let symbol = match (name, prefix.is_empty(), local) {
                        ("default", true, "default") => {
                            origin_module.default_name.as_deref().unwrap_or(local)
                        }
                        ("default", true, _) => local,
                        _ => name,
                    };
                    if (is_pascal_case(symbol) || (name == "default" && !prefix.is_empty()))
                        && origin_module.components.contains(local)
                    {
                        self.bindings.push(PublicBinding {
                            symbol: format!("{prefix}{symbol}"),
                            package,
                            declaration_path: module,
                            declaration_name: local.to_owned(),
                        });
                    }
                }
                Err(ambiguity) => {
                    let parsed = self.modules[ambiguity.module].parsed;
                    self.warn(
                        parsed,
                        parsed.module.span,
                        "discover_export_ambiguous",
                        format!("star export '{}' has multiple origins", ambiguity.name),
                    );
                }
                Ok(None) => {}
            }
        }
        self.active_namespaces.remove(module);
    }

    fn export_names(
        &mut self,
        module: &Path,
        include_default: bool,
        names: &mut BTreeSet<&'graph str>,
        visited: &mut BTreeSet<&'files Path>,
    ) {
        let record = &self.modules[module];
        if !visited.insert(record.parsed.file.as_path()) {
            return;
        }
        names.extend(
            record
                .exports
                .keys()
                .filter(|name| include_default || *name != "default")
                .map(String::as_str),
        );
        for edge in &record.stars {
            if let Some(path) = self.edge_target(record, edge) {
                self.export_names(path, false, names, visited);
            }
        }
    }

    fn resolve_export(
        &mut self,
        module: &'files Path,
        name: &'graph str,
        active: &mut BTreeSet<(&'files Path, &'graph str)>,
    ) -> Result<Option<Origin<'files, 'graph>>, AmbiguousExport<'graph>> {
        if let Some(origin) = self.resolved.get(&(module, name)) {
            return Ok(Some(*origin));
        }
        if !active.insert((module, name)) {
            return Ok(None);
        }
        let record = &self.modules[module];
        let result = if let Some(target) = record.exports.get(name) {
            let target = match target {
                Target::Local(local) => record.imports.get(local).unwrap_or(target),
                _ => target,
            };
            match target {
                Target::Local(name) if record.locals.contains(name) => {
                    Ok(Some(Origin::Declaration { module, name }))
                }
                Target::Local(_) => Ok(None),
                Target::Remote { edge, name } => {
                    self.follow_edge(record, edge, name.as_deref(), active)
                }
            }
        } else if name != "default" {
            self.resolve_star_export(record, name, active)
        } else {
            Ok(None)
        };
        active.remove(&(module, name));
        if let Ok(Some(origin)) = &result {
            self.resolved.insert((module, name), *origin);
        }
        result
    }

    fn follow_edge(
        &mut self,
        record: &'graph ModuleExports<'files>,
        edge: &'graph Edge,
        name: Option<&'graph str>,
        active: &mut BTreeSet<(&'files Path, &'graph str)>,
    ) -> Result<Option<Origin<'files, 'graph>>, AmbiguousExport<'graph>> {
        let Some(path) = self.edge_target(record, edge) else {
            return Ok(None);
        };
        let Some(name) = name else {
            return Ok(Some(Origin::Namespace(path)));
        };
        let result = self.resolve_export(path, name, active)?;
        if result.is_none() && !self.modules[path].exports.contains_key(name) {
            self.warn(
                record.parsed,
                edge.span,
                "discover_export_unresolved",
                format!("export '{name}' was not found in '{}'", edge.source),
            );
        }
        Ok(result)
    }

    fn resolve_star_export(
        &mut self,
        record: &'graph ModuleExports<'files>,
        name: &'graph str,
        active: &mut BTreeSet<(&'files Path, &'graph str)>,
    ) -> Result<Option<Origin<'files, 'graph>>, AmbiguousExport<'graph>> {
        let mut resolved = None;
        for edge in &record.stars {
            if let Some(path) = self.edge_target(record, edge)
                && let Some(origin) = self.resolve_export(path, name, active)?
            {
                if resolved.is_some_and(|previous| previous != origin) {
                    return Err(AmbiguousExport {
                        module: &record.parsed.file,
                        name,
                    });
                }
                resolved = Some(origin);
            }
        }
        Ok(resolved)
    }

    fn edge_target(&mut self, record: &ModuleExports<'files>, edge: &Edge) -> Option<&'files Path> {
        let message = match &edge.path {
            Some(path) => {
                if let Some((path, _)) = self.modules.get_key_value(path.as_path()) {
                    return Some(path);
                }
                format!(
                    "could not follow export from '{}' because '{}' failed to parse",
                    edge.source,
                    path.display()
                )
            }
            None if is_relative_specifier(&edge.source) => {
                format!("could not resolve local export source '{}'", edge.source)
            }
            None => return None,
        };
        self.warn(
            record.parsed,
            edge.span,
            "discover_export_unresolved",
            message,
        );
        None
    }

    fn warn(&mut self, parsed: &'files ParsedReactModule, span: Span, code: &str, message: String) {
        self.diagnostics
            .entry((&parsed.file, span.lo.0, message.clone()))
            .or_insert_with(|| Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: code.to_owned(),
                message,
                location: parsed.source_location_from_span(span),
            });
    }
}

fn is_relative_specifier(source: &str) -> bool {
    matches!(source, "." | "..") || source.starts_with("./") || source.starts_with("../")
}

fn resolve_discovery_path(path: &Path, files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    resolve_path_with_extensions_and_index(path, files).or_else(|| {
        let extensions = match path.extension()?.to_str()? {
            "js" => ["ts", "tsx"],
            "jsx" => ["tsx", "ts"],
            _ => return None,
        };
        extensions
            .into_iter()
            .map(|extension| path.with_extension(extension))
            .find(|path| files.contains(path))
    })
}
