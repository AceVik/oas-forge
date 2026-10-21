//! Module paths of Rust types (ADR 0004): which documented type a name
//! written in a type position means, and the component names of documented
//! types whose names collide.
//!
//! Every Rust file has a module path: the crate (the `[package] name` of the
//! `Cargo.toml` next to its `src` directory, `-` read as `_`; else the name
//! of that directory) and the modules below `src` (`#[path]` of top-level
//! `mod` declarations honoured). A name written in a type position resolves
//! in this order: a definition of the same module, an explicit `use`, glob
//! imports, a written path (exact, then by its last segments), the one
//! documented type of that name; several are ambiguous.

use crate::attrs::directives::{ItemDocs, ItemKind};
use crate::attrs::serde::SerdeAttrs;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::docs::{doc_lines, line_of};
use crate::package::Package;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use syn::ext::IdentExt;
use syn::spanned::Spanned;

/// A path of Rust names, crate first (`baylee_catalog`, `model`, `Printing`).
type Segments = Vec<String>;

/// The inline modules of a file: their path below the file's module, their
/// first and last line.
type Inline = Vec<(Segments, usize, usize)>;

/// How far `use` declarations are followed (re-exports of re-exports).
const MAX_DEPTH: usize = 8;

/// What a name defined in a module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Definition {
    /// A struct, enum, union or trait.
    Type,
    /// A type alias (without `@openapi` it is transparent).
    Alias,
}

/// A path of a `use` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UsePath {
    /// The segments, `crate`, `self` and `super` already replaced.
    segments: Segments,
    /// Whether the path may start at the module of the declaration (Rust
    /// 2018: `use models::User` names a child module or another crate).
    relative: bool,
    /// Whether the segments so far came from `crate`, `self` or `super`
    /// only (then another `super` goes up again).
    keywords: bool,
}

/// The names of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Module {
    /// The types defined here.
    defined: BTreeMap<String, Definition>,
    /// The names `use` declarations bring in, and their paths.
    uses: BTreeMap<String, UsePath>,
    /// The modules of glob imports (`use a::b::*;`).
    globs: Vec<UsePath>,
}

/// A documented type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Documented {
    /// Its component name: the serialized name, or the qualified name of a
    /// type whose name collides with another one.
    component: String,
    /// Where it is declared.
    location: Location,
    /// Whether it is a template (generic types are not qualified).
    generic: bool,
}

/// What a name written in a type position means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// The documented type with this component name.
    Component(String),
    /// Several documented types, by full path; the reference stays
    /// unresolved.
    Ambiguous(Vec<String>),
    /// A type of the module that is not documented (its full path), while
    /// other documented types have its name.
    Undocumented(String),
    /// Nothing that is known: the name is used as written.
    Unknown,
}

/// The module paths of the scanned Rust files and their documented types.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NameTable {
    /// The module path of each file and its inline modules (path below the
    /// file's module, first and last line).
    files: BTreeMap<PathBuf, (Segments, Inline)>,
    /// The names of each module, by full path.
    modules: BTreeMap<Segments, Module>,
    /// The documented types, by full path.
    documented: BTreeMap<Segments, Documented>,
    /// The full paths of the documented types, by Rust name.
    by_name: BTreeMap<String, Vec<Segments>>,
}

impl NameTable {
    /// The table of the parsed Rust `files`. Documented types whose
    /// component names collide get qualified names (`crate.module.Name`);
    /// each is reported ([`Code::DuplicateSchema`]).
    pub(crate) fn build(files: &[(&Path, &syn::File)], diagnostics: &mut Diagnostics) -> Self {
        let mut table = Self::default();
        let mut crates = BTreeMap::new();
        for (path, _) in files {
            let module = file_module(path, &mut crates);
            table.files.insert(normalize(path), (module, Vec::new()));
        }
        // `#[path = "…"] mod name;` puts a file under the declaring module.
        for (path, file) in files {
            let Some(declaring) = table.module_of(path) else {
                continue;
            };
            for item in &file.items {
                if let syn::Item::Mod(declaration) = item
                    && declaration.content.is_none()
                    && let Some(target) = path_attribute(&declaration.attrs)
                {
                    let parent = path.parent().unwrap_or(Path::new(""));
                    let target = normalize(&parent.join(target));
                    let mut module = declaring.clone();
                    module.push(declaration.ident.unraw().to_string());
                    if let Some(entry) = table.files.get_mut(&target) {
                        entry.0 = module;
                    }
                }
            }
        }
        for (path, file) in files {
            let Some(module) = table.module_of(path) else {
                continue;
            };
            let mut inline = Vec::new();
            table.collect(&file.items, &module, path, &[], &mut inline);
            if let Some(entry) = table.files.get_mut(&normalize(path)) {
                entry.1 = inline;
            }
        }
        table.qualify(diagnostics);
        for path in table.documented.keys() {
            if let Some(name) = path.last() {
                table
                    .by_name
                    .entry(name.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
        table
    }

    /// The module path of `file`.
    fn module_of(&self, file: &Path) -> Option<Segments> {
        self.files
            .get(&normalize(file))
            .map(|(module, _)| module.clone())
    }

    /// The module path at `line` of `file`: the file's module and the inline
    /// modules around the line. `None` for files the table does not know.
    pub(crate) fn scope_at(&self, file: &Path, line: usize) -> Option<Vec<String>> {
        let (module, inline) = self.files.get(&normalize(file))?;
        let mut scope = module.clone();
        // The innermost inline module that contains the line (the longest).
        if let Some((path, _, _)) = inline
            .iter()
            .filter(|(_, first, last)| (*first..=*last).contains(&line))
            .max_by_key(|(path, _, _)| path.len())
        {
            scope.extend(path.iter().cloned());
        }
        Some(scope)
    }

    /// Adds the types, documented types, `use` declarations and inline
    /// modules of `items` (in the module `module`; `inline` is the path of
    /// the inline module below the file's module).
    fn collect(
        &mut self,
        items: &[syn::Item],
        module: &Segments,
        file: &Path,
        inline_path: &[String],
        inline: &mut Inline,
    ) {
        self.modules.entry(module.clone()).or_default();
        for item in items {
            match item {
                syn::Item::Struct(s) => {
                    self.define(module, &s.ident, Definition::Type);
                    self.document(module, &s.ident, &s.attrs, &s.generics, file, s.span());
                }
                syn::Item::Enum(e) => {
                    self.define(module, &e.ident, Definition::Type);
                    self.document(module, &e.ident, &e.attrs, &e.generics, file, e.span());
                }
                syn::Item::Union(u) => self.define(module, &u.ident, Definition::Type),
                syn::Item::Trait(t) => self.define(module, &t.ident, Definition::Type),
                syn::Item::Type(t) => {
                    self.define(module, &t.ident, Definition::Alias);
                    self.document(module, &t.ident, &t.attrs, &t.generics, file, t.span());
                }
                syn::Item::Use(u) => {
                    let start = UsePath {
                        segments: Vec::new(),
                        relative: u.leading_colon.is_none(),
                        keywords: false,
                    };
                    self.import(module, &u.tree, start);
                }
                syn::Item::Mod(m) => {
                    let Some((_, content)) = &m.content else {
                        continue;
                    };
                    let name = m.ident.unraw().to_string();
                    let mut child = module.clone();
                    child.push(name.clone());
                    let mut path = inline_path.to_vec();
                    path.push(name);
                    inline.push((path.clone(), line_of(m.span()), m.span().end().line));
                    self.collect(content, &child, file, &path, inline);
                }
                _ => {}
            }
        }
    }

    /// Records that `module` defines the type `ident`.
    fn define(&mut self, module: &Segments, ident: &syn::Ident, definition: Definition) {
        self.modules
            .entry(module.clone())
            .or_default()
            .defined
            .insert(ident.unraw().to_string(), definition);
    }

    /// Records the type `ident` of `module` if its doc comment exports it.
    fn document(
        &mut self,
        module: &Segments,
        ident: &syn::Ident,
        attrs: &[syn::Attribute],
        generics: &syn::Generics,
        file: &Path,
        span: proc_macro2::Span,
    ) {
        let docs = ItemDocs::parse(&doc_lines(attrs), ItemKind::Container);
        if !docs.exported || docs.skip {
            return;
        }
        let name = ident.unraw().to_string();
        let component = docs
            .rename
            .clone()
            .or_else(|| SerdeAttrs::parse(attrs).rename)
            .unwrap_or_else(|| name.clone());
        let mut path = module.clone();
        path.push(name);
        self.documented.insert(
            path,
            Documented {
                component,
                location: Location::new(file, line_of(span)),
                generic: generics.type_params().next().is_some() || docs.blueprint.is_some(),
            },
        );
    }

    /// Records the names a `use` tree brings into `module`; `prefix` is the
    /// path before the tree.
    fn import(&mut self, module: &Segments, tree: &syn::UseTree, prefix: UsePath) {
        match tree {
            syn::UseTree::Path(path) => {
                let next = step(module, prefix, &path.ident.unraw().to_string());
                self.import(module, &path.tree, next);
            }
            syn::UseTree::Name(name) => {
                let ident = name.ident.unraw().to_string();
                let (local, target) = if ident == "self" {
                    (prefix.segments.last().cloned(), prefix)
                } else {
                    (Some(ident.clone()), step(module, prefix, &ident))
                };
                if let Some(local) = local {
                    self.modules
                        .entry(module.clone())
                        .or_default()
                        .uses
                        .insert(local, target);
                }
            }
            syn::UseTree::Rename(rename) => {
                let ident = rename.ident.unraw().to_string();
                let target = if ident == "self" {
                    prefix
                } else {
                    step(module, prefix, &ident)
                };
                self.modules
                    .entry(module.clone())
                    .or_default()
                    .uses
                    .insert(rename.rename.unraw().to_string(), target);
            }
            syn::UseTree::Glob(_) => {
                self.modules
                    .entry(module.clone())
                    .or_default()
                    .globs
                    .push(prefix);
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.import(module, tree, prefix.clone());
                }
            }
        }
    }

    /// Gives the documented types whose component names collide their
    /// qualified names and reports each.
    fn qualify(&mut self, diagnostics: &mut Diagnostics) {
        let mut claims: BTreeMap<String, Vec<Segments>> = BTreeMap::new();
        for (path, documented) in &self.documented {
            if !documented.generic {
                claims
                    .entry(documented.component.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
        for (component, paths) in claims {
            if paths.len() < 2 {
                continue;
            }
            let all: Vec<String> = paths
                .iter()
                .map(|p| format!("`{}`", p.join("::")))
                .collect();
            for path in &paths {
                let Some(documented) = self.documented.get_mut(path) else {
                    continue;
                };
                documented.component = path.join(".");
                diagnostics.push(
                    Diagnostic::new(
                        Code::DuplicateSchema,
                        format!(
                            "schema `{component}` is defined by several types ({}); this one is named `{}`; give them distinct names with `@openapi rename`",
                            all.join(", "),
                            documented.component
                        ),
                    )
                    .at(documented.location.clone()),
                );
            }
        }
    }

    /// The component name of the documented type `name` defined in `scope`,
    /// when it differs from `name` because it collides with another one.
    pub(crate) fn qualified(&self, scope: &[String], name: &str) -> Option<&str> {
        let mut path = scope.to_vec();
        path.push(name.to_string());
        let documented = self.documented.get(&path)?;
        (documented.component.contains('.')).then_some(documented.component.as_str())
    }

    /// What the type written as `written` (path segments) in the module
    /// `scope` means.
    pub(crate) fn resolve(&self, scope: &[String], written: &[String]) -> Resolved {
        let Some(name) = written.last() else {
            return Resolved::Unknown;
        };
        let found = match written {
            [_] => self.bare(scope, name),
            _ => self.qualified_path(scope, written),
        };
        found.unwrap_or_else(|| self.by_name(name))
    }

    /// A name without path: defined in `scope`, imported or brought in by a
    /// glob import. `None` when `scope` says nothing about it.
    fn bare(&self, scope: &[String], name: &str) -> Option<Resolved> {
        let module = self.modules.get(scope)?;
        let mut path = scope.to_vec();
        path.push(name.to_string());
        if let Some(definition) = module.defined.get(name) {
            return Some(self.defined(&path, *definition));
        }
        if let Some(target) = module.uses.get(name) {
            let target = self.absolute(scope, target);
            return Some(
                self.follow(&target, 0)
                    .or_else(|| self.by_suffix(&target))
                    .unwrap_or_else(|| self.by_name(name)),
            );
        }
        let mut found: BTreeSet<String> = BTreeSet::new();
        let mut paths = Vec::new();
        for glob in &module.globs {
            let mut candidate = self.absolute(scope, glob);
            candidate.push(name.to_string());
            if let Some(Resolved::Component(component)) = self.follow(&candidate, 0)
                && found.insert(component)
            {
                paths.push(candidate.join("::"));
            }
        }
        match (found.into_iter().next(), paths.len()) {
            (Some(component), 1) => Some(Resolved::Component(component)),
            (Some(_), _) => Some(Resolved::Ambiguous(paths)),
            (None, _) => None,
        }
    }

    /// A written path (`baylee_cardtext::Printing`, `crate::model::Status`).
    fn qualified_path(&self, scope: &[String], written: &[String]) -> Option<Resolved> {
        let segments;
        // A path that starts with an imported name (`use a::model; model::X`).
        if let Some(first) = written.first()
            && let Some(target) = self.modules.get(scope).and_then(|m| m.uses.get(first))
        {
            let mut imported = self.absolute(scope, target);
            imported.extend(written.iter().skip(1).cloned());
            segments = imported;
        } else {
            let start = UsePath {
                segments: Vec::new(),
                relative: true,
                keywords: false,
            };
            let path = written
                .iter()
                .fold(start, |path, segment| step(scope, path, segment));
            segments = self.absolute(scope, &path);
        }
        self.follow(&segments, 0)
            .or_else(|| self.by_suffix(&segments))
    }

    /// The meaning of the type `path`, defined as `definition` in its module.
    fn defined(&self, path: &Segments, definition: Definition) -> Resolved {
        if let Some(documented) = self.documented.get(path) {
            return Resolved::Component(documented.component.clone());
        }
        let others = path
            .last()
            .and_then(|name| self.by_name.get(name))
            .is_some_and(|paths| !paths.is_empty());
        match definition {
            // A transparent alias is replaced where the references are
            // resolved.
            Definition::Alias => Resolved::Unknown,
            Definition::Type if others => Resolved::Undocumented(path.join("::")),
            Definition::Type => Resolved::Unknown,
        }
    }

    /// The meaning of the absolute `path`: a documented type, a type of a
    /// scanned module, or what a `use` of its module re-exports. `None` when
    /// the scanned sources do not define it.
    fn follow(&self, path: &Segments, depth: usize) -> Option<Resolved> {
        if let Some(documented) = self.documented.get(path) {
            return Some(Resolved::Component(documented.component.clone()));
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        let (name, parent) = path.split_last()?;
        let parent = self.module_path(parent, depth)?;
        let module = self.modules.get(&parent)?;
        if let Some(definition) = module.defined.get(name) {
            let mut path = parent.clone();
            path.push(name.clone());
            return Some(self.defined(&path, *definition));
        }
        if let Some(target) = module.uses.get(name) {
            return self.follow(&self.absolute(&parent, target), depth + 1);
        }
        module.globs.iter().find_map(|glob| {
            let mut candidate = self.absolute(&parent, glob);
            candidate.push(name.clone());
            self.follow(&candidate, depth + 1)
        })
    }

    /// The scanned module `path` names, also through `use` declarations of
    /// modules (`use crate::models as m;` then `m::User`).
    fn module_path(&self, path: &[String], depth: usize) -> Option<Segments> {
        if self.modules.contains_key(path) {
            return Some(path.to_vec());
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        let (last, prefix) = path.split_last()?;
        let prefix = self.module_path(prefix, depth + 1)?;
        let target = self.modules.get(&prefix)?.uses.get(last)?;
        self.module_path(&self.absolute(&prefix, target), depth + 1)
    }

    /// The documented types whose full paths end with `path` (without its
    /// first segment when nothing matches it whole: a crate re-exporting
    /// the type from a module).
    fn by_suffix(&self, path: &Segments) -> Option<Resolved> {
        let ends_with = |suffix: &[String]| -> Vec<&Segments> {
            self.documented
                .keys()
                .filter(|full| full.ends_with(suffix))
                .collect()
        };
        let mut matches = ends_with(path);
        if matches.is_empty()
            && let (Some(first), Some(name)) = (path.first(), path.last())
            && path.len() > 1
        {
            // `baylee_catalog::Printing` for `baylee_catalog::model::Printing`.
            matches = self
                .documented
                .keys()
                .filter(|full| full.first() == Some(first) && full.last() == Some(name))
                .collect();
        }
        self.unique(&matches)
    }

    /// The one documented type named `name`, else ambiguous or unknown.
    fn by_name(&self, name: &str) -> Resolved {
        let paths: Vec<&Segments> = self.by_name.get(name).into_iter().flatten().collect();
        self.unique(&paths).unwrap_or(Resolved::Unknown)
    }

    /// The component of the only path of `paths`; several are ambiguous,
    /// none is `None`.
    fn unique(&self, paths: &[&Segments]) -> Option<Resolved> {
        match paths {
            [] => None,
            [path] => self
                .documented
                .get(*path)
                .map(|documented| Resolved::Component(documented.component.clone())),
            _ => Some(Resolved::Ambiguous(
                paths.iter().map(|path| path.join("::")).collect(),
            )),
        }
    }

    /// `path` of a `use` in `scope` as an absolute path: a relative one
    /// starts at `scope` when `scope` has a module of its first segment.
    fn absolute(&self, scope: &[String], path: &UsePath) -> Segments {
        if path.relative
            && let Some(first) = path.segments.first()
        {
            let mut local = scope.to_vec();
            local.push(first.clone());
            if self.modules.contains_key(&local) {
                local.extend(path.segments.iter().skip(1).cloned());
                return local;
            }
        }
        path.segments.clone()
    }
}

/// `path` with `segment` appended. `crate`, `self` and `super` at the start
/// of a relative path (also `super::super`) are replaced by the crate,
/// `module` and its parents.
fn step(module: &[String], mut path: UsePath, segment: &str) -> UsePath {
    let at_start = path.segments.is_empty() && path.relative;
    match segment {
        "crate" if at_start => {
            path.segments = module.iter().take(1).cloned().collect();
            path.relative = false;
            path.keywords = true;
        }
        "self" if at_start => {
            path.segments = module.to_vec();
            path.relative = false;
            path.keywords = true;
        }
        "super" if at_start || path.keywords => {
            if at_start {
                path.segments = module.to_vec();
                path.relative = false;
                path.keywords = true;
            }
            // The crate root has no parent: `super` stays at the crate.
            if path.segments.len() > 1 {
                path.segments.pop();
            }
        }
        _ => {
            path.segments.push(segment.to_string());
            path.keywords = false;
        }
    }
    path
}

/// The module path of the Rust file `path`: the crate name, then the
/// modules below the last `src` directory (`mod.rs`, `lib.rs` and `main.rs`
/// are their directory's module). Without `src`, the crate is the file's
/// directory and the module the file's name. `crates` caches the crate
/// names by directory.
fn file_module(path: &Path, crates: &mut BTreeMap<PathBuf, String>) -> Segments {
    let components: Vec<Component<'_>> = path.components().collect();
    let src = components
        .iter()
        .rposition(|c| matches!(c, Component::Normal(name) if *name == "src"));
    let (crate_dir, below): (PathBuf, Vec<Component<'_>>) = match src {
        Some(index) => (
            components.iter().take(index).collect(),
            components.iter().skip(index + 1).copied().collect(),
        ),
        None => (
            path.parent().map(Path::to_path_buf).unwrap_or_default(),
            components.last().copied().into_iter().collect(),
        ),
    };
    let name = crates
        .entry(crate_dir.clone())
        .or_insert_with(|| crate_name(&crate_dir))
        .clone();
    let mut module = vec![name];
    let count = below.len();
    for (i, component) in below.iter().enumerate() {
        let Component::Normal(text) = component else {
            continue;
        };
        let text = text.to_string_lossy();
        if i + 1 < count {
            module.push(text.into_owned());
        } else {
            let stem = text.strip_suffix(".rs").unwrap_or(&text);
            if !matches!(stem, "lib" | "main" | "mod") {
                module.push(stem.to_string());
            }
        }
    }
    module
}

/// The crate name of the crate in `dir`: the `[package] name` of its
/// `Cargo.toml`, else the directory's name, else `crate`; as an identifier
/// (`-` and other characters as `_`). An empty `dir` (an in-memory file) is
/// not looked up.
fn crate_name(dir: &Path) -> String {
    let manifest = (!dir.as_os_str().is_empty())
        .then(|| {
            Package::from_manifest(dir.join("Cargo.toml"))
                .ok()
                .flatten()
        })
        .flatten();
    let name = match manifest {
        Some(package) => package.name,
        None => dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "crate".to_string()),
    };
    let mut name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        name.insert(0, '_');
    }
    name
}

/// The value of a `#[path = "file"]` attribute.
fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(meta) if meta.path.is_ident("path") => match &meta.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(path),
                ..
            }) => Some(path.value()),
            _ => None,
        },
        _ => None,
    })
}

/// `path` without `.` and with `..` resolved where possible.
fn normalize(path: &Path) -> PathBuf {
    super::normalize_path(path)
}
