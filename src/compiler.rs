//! Compile feature modules into a typed, linked architecture graph.
//! The compiler never executes `.vitrifi` input or its verification commands.
use crate::{host, model::*, parser};
use globset::Glob;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    path::{Component, Path},
    process::Command,
};
use walkdir::WalkDir;

pub const LIMITATIONS: &[&str] = &[
    "Types, visibility, relationships and effect inclusion are checked within the declared Vitrifi graph. Host bodies are not proved to implement those declarations.",
    "Host bindings validate language and symbol existence; host/Vitrifi signature equivalence is not checked. Calls extracted from host syntax have unresolved targets.",
    "Host architecture enforcement covers resolved local imports only. Dynamic dispatch, reflection, implicit wiring and arbitrary module aliases remain outside that check.",
    "Feature compilation follows imports and parses bound or boundary-selected source files. It is not a benchmarked incremental build system or a guarantee of arbitrary repository scale.",
];

pub fn location(path: &str) -> Location {
    Location {
        path: path.into(),
        line: 1,
        column: 1,
    }
}
fn error(code: &str, message: impl Into<String>, at: &Location) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        location: at.clone(),
    }
}
pub fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Normalize a path relative to a module, without permitting root escape.
pub fn relative_path(module: &str, value: &str) -> Result<String, String> {
    if Path::new(value).is_absolute() {
        return Err("Expected a repository-relative path".into());
    }
    let mut parts: Vec<String> = Path::new(module)
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .filter_map(|x| match x {
            Component::Normal(p) => Some(p.to_string_lossy().into()),
            _ => None,
        })
        .collect();
    for part in Path::new(value).components() {
        match part {
            Component::Normal(p) => parts.push(p.to_string_lossy().into()),
            Component::CurDir => (),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("Path escapes the repository root".into());
                }
            }
            _ => return Err("Expected a relative path".into()),
        }
    }
    Ok(parts.join("/"))
}

pub fn read_inside(root: &Path, path: &str) -> Result<String, String> {
    let full = root.join(path);
    let actual = full
        .canonicalize()
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    if !actual.starts_with(root) {
        return Err(format!("{path} resolves outside the repository"));
    }
    if actual != full {
        return Err(format!("Symlinked source paths are not supported: {path}"));
    }
    if fs::metadata(&actual).map_err(|e| e.to_string())?.len() > 2_000_000 {
        return Err(format!("{path} exceeds the 2 MB source limit"));
    }
    fs::read_to_string(actual).map_err(|e| format!("Cannot read UTF-8 source {path}: {e}"))
}

pub(crate) fn ignored(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::Normal(p) if matches!(p.to_str(), Some(".git"|".vitrifi"|".venv"|"venv"|"node_modules"|"vendor"|"target"|"build"|"dist"|".toolchain"|"__pycache__")) || p.to_string_lossy().ends_with(".egg-info")))
}

/// Git ignore semantics when available, otherwise a bounded ordinary directory walk.
pub fn repository_files(root: &Path) -> Result<Vec<String>, String> {
    let is_git = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let mut files = BTreeSet::new();
    if is_git {
        let result = Command::new("git")
            .args(["-c", "core.fsmonitor=false", "-C"])
            .arg(root)
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err("Git file discovery failed".into());
        }
        for raw in result.stdout.split(|b| *b == 0).filter(|x| !x.is_empty()) {
            let p = std::str::from_utf8(raw).map_err(|_| "Non-UTF-8 repository path")?;
            files.insert(p.to_string());
        }
    } else {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !ignored(e.path().strip_prefix(root).unwrap_or(e.path())))
        {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().is_file() {
                files.insert(slash(entry.path().strip_prefix(root).unwrap()));
            }
        }
    }
    Ok(files
        .into_iter()
        .filter(|p| {
            !ignored(Path::new(p))
                && !root.join(p).is_symlink()
                && root.join(p).is_file()
                && root
                    .join(p)
                    .canonicalize()
                    .map(|x| x.starts_with(root))
                    .unwrap_or(false)
        })
        .collect())
}

pub fn content_digest(inputs: &BTreeMap<String, String>) -> String {
    let mut digest = Sha256::new();
    digest.update(env!("CARGO_PKG_VERSION"));
    for (path, hash) in inputs {
        digest.update(path.len().to_le_bytes());
        digest.update(path);
        digest.update(hash);
    }
    format!("{:x}", digest.finalize())
}
pub fn hash_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
pub fn workspace_digest(root: &Path) -> Result<String, String> {
    let mut hashes = BTreeMap::new();
    for path in repository_files(root)? {
        hashes.insert(
            path.clone(),
            hash_bytes(&fs::read(root.join(path)).map_err(|e| e.to_string())?),
        );
    }
    Ok(content_digest(&hashes))
}

type Id = (usize, usize);
struct Symbols<'a> {
    modules: &'a [Module],
    names: HashMap<(usize, String), usize>,
    imports: HashMap<(usize, String), usize>,
}
impl<'a> Symbols<'a> {
    fn new(modules: &'a [Module], diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut names = HashMap::new();
        let mut imports = HashMap::new();
        let mut feature_names = HashSet::new();
        let paths: HashMap<&str, usize> = modules
            .iter()
            .enumerate()
            .map(|(i, m)| (m.path.as_str(), i))
            .collect();
        for (i, m) in modules.iter().enumerate() {
            if !feature_names.insert(&m.name) {
                diagnostics.push(error(
                    "DUPLICATE_FEATURE",
                    format!("Feature {} is declared more than once", m.name),
                    &location(&m.path),
                ));
            }
            for (j, d) in m.declarations.iter().enumerate() {
                if names.insert((i, d.name.clone()), j).is_some() {
                    diagnostics.push(error(
                        "DUPLICATE_NAME",
                        format!("{} is already declared in this feature", d.name),
                        &d.location,
                    ));
                }
                if builtin_arity(&d.name).is_some() {
                    diagnostics.push(error(
                        "RESERVED_TYPE",
                        format!("{} is a builtin type", d.name),
                        &d.location,
                    ));
                }
            }
            for import in &m.imports {
                if names.contains_key(&(i, import.alias.clone()))
                    || imports.contains_key(&(i, import.alias.clone()))
                {
                    diagnostics.push(error(
                        "DUPLICATE_NAME",
                        format!("Import alias {} is already declared", import.alias),
                        &import.location,
                    ));
                }
                if let Ok(p) = relative_path(&m.path, &import.path) {
                    if let Some(target) = paths.get(p.as_str()) {
                        imports.insert((i, import.alias.clone()), *target);
                    }
                }
            }
        }
        Self {
            modules,
            names,
            imports,
        }
    }
    fn declaration(&self, id: Id) -> &Declaration {
        &self.modules[id.0].declarations[id.1]
    }
    fn name(&self, id: Id) -> String {
        format!("{}::{}", self.modules[id.0].name, self.declaration(id).name)
    }
    fn resolve(&self, module: usize, reference: &Reference, kind: &str) -> Result<Id, Diagnostic> {
        let segments: Vec<_> = reference.name.split('.').collect();
        let (target, name, external) = match segments.as_slice() {
            [name] => (module, *name, false),
            [alias, name] => (
                *self
                    .imports
                    .get(&(module, alias.to_string()))
                    .ok_or_else(|| {
                        error(
                            "UNKNOWN_IMPORT",
                            format!("No imported feature alias {alias}"),
                            &reference.location,
                        )
                    })?,
                *name,
                true,
            ),
            _ => {
                return Err(error(
                    "INVALID_REFERENCE",
                    "Use a local name or ImportAlias.Name",
                    &reference.location,
                ))
            }
        };
        let id = (
            target,
            *self.names.get(&(target, name.into())).ok_or_else(|| {
                error(
                    "UNKNOWN_NAME",
                    format!("Unknown {kind} {}", reference.name),
                    &reference.location,
                )
            })?,
        );
        let declaration = self.declaration(id);
        if external && !declaration.public {
            return Err(error(
                "PRIVATE_MEMBER",
                format!("{} is private to {}", name, self.modules[target].name),
                &reference.location,
            ));
        }
        if declaration.body.kind() != kind {
            return Err(error(
                "TYPE_MISMATCH",
                format!(
                    "Expected {kind}, found {} {}",
                    declaration.body.kind(),
                    reference.name
                ),
                &reference.location,
            ));
        }
        Ok(id)
    }
    fn check_type(
        &self,
        module: usize,
        ty: &TypeExpr,
        public: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let arity = if let Some(n) = builtin_arity(&ty.name) {
            n
        } else {
            match self.resolve(
                module,
                &Reference {
                    name: ty.name.clone(),
                    location: ty.location.clone(),
                },
                "type",
            ) {
                Err(e) => diagnostics.push(e),
                Ok(id) if public && !self.declaration(id).public => diagnostics.push(error(
                    "PRIVATE_INTERFACE",
                    format!("Exported operation exposes private type {}", ty.name),
                    &ty.location,
                )),
                Ok(_) => (),
            }
            0
        };
        if ty.arguments.len() != arity {
            diagnostics.push(error(
                "TYPE_ARITY",
                format!(
                    "{} expects {arity} type arguments, got {}",
                    ty.name,
                    ty.arguments.len()
                ),
                &ty.location,
            ));
        }
        for argument in &ty.arguments {
            self.check_type(module, argument, public, diagnostics);
        }
    }
}
fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "String" | "Int" | "Bool" | "Unit" | "Any" => Some(0),
        "List" | "Option" => Some(1),
        "Result" => Some(2),
        _ => None,
    }
}

pub fn compile(root: &Path, entry: Option<&Path>) -> Result<Program, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let paths = repository_files(&root)?;
    let mut queue = VecDeque::new();
    if let Some(entry) = entry {
        let full = if entry.is_absolute() {
            entry.to_path_buf()
        } else {
            root.join(entry)
        };
        let full = full
            .canonicalize()
            .map_err(|e| format!("Cannot find entry {}: {e}", entry.display()))?;
        if !full.starts_with(&root) {
            return Err("Entry must be inside --root".into());
        }
        if full.extension().and_then(|x| x.to_str()) != Some("vitrifi") {
            return Err("Entry must be a .vitrifi feature file".into());
        }
        queue.push_back(slash(full.strip_prefix(&root).unwrap()));
    } else {
        queue.extend(
            paths
                .iter()
                .filter(|p| {
                    p.ends_with(".vitrifi")
                        && !p.ends_with(".target.vitrifi")
                        && !p.ends_with(".workflow.vitrifi")
                })
                .cloned(),
        );
    }
    if queue.is_empty() {
        return Err(
            "No .vitrifi feature files found. Start with `vitrifi init path/to/feature.vitrifi`."
                .into(),
        );
    }
    let mut modules = vec![];
    let mut diagnostics = vec![];
    let mut visited = HashSet::new();
    let mut inputs = BTreeMap::new();
    while let Some(path) = queue.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }
        match read_inside(&root, &path) {
            Err(e) => diagnostics.push(error("MODULE_READ", e, &location(&path))),
            Ok(source) => {
                inputs.insert(path.clone(), hash_bytes(source.as_bytes()));
                match parser::parse(&path, &source) {
                    Err(errors) => diagnostics.extend(errors),
                    Ok(module) => {
                        for import in &module.imports {
                            match relative_path(&path, &import.path) {
                                Ok(p) if p.ends_with(".vitrifi") => queue.push_back(p),
                                Ok(_) => diagnostics.push(error(
                                    "IMPORT_EXTENSION",
                                    "Feature imports must name a .vitrifi file",
                                    &import.location,
                                )),
                                Err(e) => {
                                    diagnostics.push(error("IMPORT_PATH", e, &import.location))
                                }
                            }
                        }
                        modules.push(module);
                    }
                }
            }
        }
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    let symbols = Symbols::new(&modules, &mut diagnostics);
    let mut edges = vec![];
    let mut effects: HashMap<Id, BTreeSet<(String, String)>> = HashMap::new();
    let mut call_edges = vec![];
    let mut boundaries = vec![];
    let mut bound = vec![];
    let mut selected = BTreeSet::new();
    for (i, module) in modules.iter().enumerate() {
        for (j, declaration) in module.declarations.iter().enumerate() {
            let id = (i, j);
            let name = symbols.name(id);
            edges.push(Edge {
                from: module.name.clone(),
                to: name.clone(),
                relation: "owns".into(),
                provenance: "declared".into(),
                location: declaration.location.clone(),
            });
            let mut link = |reference: &Reference, kind: &str, relation: &str| -> Option<Id> {
                match symbols.resolve(i, reference, kind) {
                    Ok(target) => {
                        if declaration.public
                            && matches!(declaration.body, DeclBody::Operation { .. })
                            && relation != "calls"
                            && !symbols.declaration(target).public
                        {
                            diagnostics.push(error(
                                "PRIVATE_INTERFACE",
                                format!(
                                    "Exported operation exposes private {kind} {}",
                                    reference.name
                                ),
                                &reference.location,
                            ));
                        }
                        edges.push(Edge {
                            from: name.clone(),
                            to: symbols.name(target),
                            relation: relation.into(),
                            provenance: "declared; reference type checked".into(),
                            location: reference.location.clone(),
                        });
                        Some(target)
                    }
                    Err(e) => {
                        diagnostics.push(e);
                        None
                    }
                }
            };
            match &declaration.body {
                DeclBody::Operation {
                    parameters,
                    returns,
                    binding,
                    effects: declared,
                    requires,
                    ensures,
                    calls,
                    ..
                } => {
                    let mut observed = BTreeSet::new();
                    for effect in declared {
                        if let Some(target) = link(&effect.resource, "resource", &effect.action) {
                            observed.insert((effect.action.clone(), symbols.name(target)));
                        }
                    }
                    effects.insert(id, observed);
                    for reference in requires {
                        link(reference, "requirement", "requires");
                    }
                    for reference in ensures {
                        link(reference, "requirement", "ensures");
                    }
                    for reference in calls {
                        if let Some(target) = link(reference, "operation", "calls") {
                            call_edges.push((id, target, reference.location.clone()));
                        }
                    }
                    let mut param_names = HashSet::new();
                    for parameter in parameters {
                        if !param_names.insert(&parameter.name) {
                            diagnostics.push(error(
                                "DUPLICATE_PARAMETER",
                                format!("Duplicate parameter {}", parameter.name),
                                &declaration.location,
                            ));
                        }
                        symbols.check_type(i, &parameter.ty, declaration.public, &mut diagnostics);
                    }
                    symbols.check_type(i, returns, declaration.public, &mut diagnostics);
                    match relative_path(&module.path, &binding.path) {
                        Ok(path) => {
                            if host::language_for(&path) != Some(binding.language.as_str()) {
                                diagnostics.push(error(
                                    "BINDING_LANGUAGE",
                                    format!(
                                        "{} is not a supported {} source file",
                                        path, binding.language
                                    ),
                                    &binding.location,
                                ));
                            }
                            selected.insert(path.clone());
                            bound.push((id, path, binding));
                        }
                        Err(e) => diagnostics.push(error("BINDING_PATH", e, &binding.location)),
                    }
                }
                DeclBody::Boundary {
                    paths: patterns,
                    allows,
                } => {
                    let allowed: BTreeSet<String> = allows
                        .iter()
                        .filter_map(|r| link(r, "boundary", "allows").map(|x| symbols.name(x)))
                        .collect();
                    let mut matchers = vec![];
                    for pattern in patterns {
                        match relative_path(&module.path, pattern).and_then(|p| {
                            Glob::new(&p)
                                .map(|g| g.compile_matcher())
                                .map_err(|e| e.to_string())
                        }) {
                            Ok(matcher) => {
                                for p in &paths {
                                    if host::language_for(p).is_some() && matcher.is_match(p) {
                                        selected.insert(p.clone());
                                    }
                                }
                                matchers.push(matcher);
                            }
                            Err(e) => diagnostics.push(error(
                                "BOUNDARY_PATTERN",
                                e,
                                &declaration.location,
                            )),
                        }
                    }
                    boundaries.push((name, matchers, allowed, declaration.location.clone()));
                }
                DeclBody::Check {
                    command,
                    cwd,
                    covers,
                    timeout,
                } => {
                    for reference in covers {
                        link(reference, "requirement", "covers");
                    }
                    if command.is_empty() || command[0].is_empty() {
                        diagnostics.push(error(
                            "EMPTY_COMMAND",
                            "A check requires a nonempty command",
                            &declaration.location,
                        ));
                    }
                    if *timeout == 0 || *timeout > 3600 {
                        diagnostics.push(error(
                            "CHECK_TIMEOUT",
                            "Check timeout must be 1..3600 seconds",
                            &declaration.location,
                        ));
                    }
                    match relative_path(&module.path, cwd) {
                        Ok(p)
                            if root.join(&p).is_dir()
                                && root
                                    .join(&p)
                                    .canonicalize()
                                    .map(|x| x.starts_with(&root))
                                    .unwrap_or(false) =>
                        {
                            ()
                        }
                        _ => diagnostics.push(error(
                            "CHECK_DIRECTORY",
                            "Check cwd must be an existing directory inside the repository",
                            &declaration.location,
                        )),
                    }
                }
                _ => (),
            }
        }
    }
    for (caller, callee, at) in call_edges {
        if let (Some(own), Some(target)) = (effects.get(&caller), effects.get(&callee)) {
            let missing: Vec<_> = target
                .difference(own)
                .map(|(verb, resource)| format!("{verb} {resource}"))
                .collect();
            if !missing.is_empty() {
                diagnostics.push(error(
                    "UNDECLARED_EFFECT",
                    format!(
                        "{} calls {}, but omits declared effects: {}",
                        symbols.name(caller),
                        symbols.name(callee),
                        missing.join(", ")
                    ),
                    &at,
                ));
            }
        }
    }
    let mut host_files = vec![];
    for path in selected {
        match read_inside(&root, &path) {
            Ok(source) => {
                inputs.insert(path.clone(), hash_bytes(source.as_bytes()));
                let file = host::analyze(&path, &source);
                diagnostics.extend(file.diagnostics.clone());
                host_files.push(file);
            }
            Err(e) => diagnostics.push(error("HOST_READ", e, &location(&path))),
        }
    }
    let files: HashMap<_, _> = host_files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut owners: HashMap<String, String> = HashMap::new();
    for (id, path, binding) in bound {
        let selector = format!("{path}::{}", binding.symbol);
        if let Some(previous) = owners.insert(selector.clone(), symbols.name(id)) {
            diagnostics.push(error(
                "DUPLICATE_BINDING",
                format!("{selector} is already owned by {previous}; import that operation instead"),
                &binding.location,
            ));
        }
        if let Some(file) = files.get(path.as_str()) {
            let matches: Vec<_> = file
                .symbols
                .iter()
                .filter(|s| s.name == binding.symbol)
                .collect();
            if matches.len() != 1 {
                diagnostics.push(error(
                    "MISSING_HOST_SYMBOL",
                    format!(
                        "Expected one {} symbol {} in {path}; found {}",
                        binding.language,
                        binding.symbol,
                        matches.len()
                    ),
                    &binding.location,
                ));
            } else {
                edges.push(Edge {
                    from: symbols.name(id),
                    to: selector,
                    relation: "binds".into(),
                    provenance: "declared; host symbol checked".into(),
                    location: binding.location.clone(),
                });
            }
        }
    }
    // Build a path inventory once. Unparsed files are candidates for resolving local
    // imports, but are never presented as analyzed implementations.
    let mut inventory = host_files.clone();
    for p in &paths {
        if !files.contains_key(p.as_str()) {
            if let Some(language) = host::language_for(p) {
                inventory.push(HostFile {
                    path: p.clone(),
                    language: language.into(),
                    package: None,
                    symbols: vec![],
                    imports: vec![],
                    diagnostics: vec![],
                });
            }
        }
    }
    let resolver = host::ImportResolver::new(&inventory);
    let mut assignments: HashMap<String, Option<usize>> = HashMap::new();
    for file in &inventory {
        let groups: Vec<_> = boundaries
            .iter()
            .enumerate()
            .filter(|(_, (_, matchers, _, _))| matchers.iter().any(|m| m.is_match(&file.path)))
            .map(|(i, _)| i)
            .collect();
        if groups.len() > 1 && files.contains_key(file.path.as_str()) {
            diagnostics.push(error(
                "AMBIGUOUS_BOUNDARY",
                format!("{} belongs to multiple boundaries", file.path),
                &location(&file.path),
            ));
        }
        assignments.insert(
            file.path.clone(),
            if groups.len() == 1 {
                Some(groups[0])
            } else {
                None
            },
        );
    }
    let mut unresolved = 0;
    for file in &host_files {
        for import in &file.imports {
            let targets = resolver.resolve(file, import);
            if targets.is_empty() {
                unresolved += 1;
            }
            for target in targets {
                if target == file.path {
                    continue;
                }
                let at = Location {
                    path: file.path.clone(),
                    line: import.line,
                    column: 1,
                };
                edges.push(Edge {
                    from: file.path.clone(),
                    to: target.clone(),
                    relation: "imports".into(),
                    provenance: "host syntax + path resolution".into(),
                    location: at.clone(),
                });
                if let Some(Some(a)) = assignments.get(&file.path) {
                    if let Some(Some(b)) = assignments.get(&target) {
                        if a != b && !boundaries[*a].2.contains(&boundaries[*b].0) {
                            diagnostics.push(error(
                                "BOUNDARY_VIOLATION",
                                format!(
                                    "{} may not import {} ({target}); policy declared at {}:{}",
                                    boundaries[*a].0,
                                    boundaries[*b].0,
                                    boundaries[*a].3.path,
                                    boundaries[*a].3.line
                                ),
                                &at,
                            ));
                        }
                    } else {
                        diagnostics.push(error("UNCLASSIFIED_DEPENDENCY",format!("{} imports {target}, which has no unambiguous boundary in the compiled feature closure",boundaries[*a].0),&at));
                    }
                }
            }
        }
    }
    // A concurrent edit must not silently combine source from different revisions.
    for (path, hash) in &inputs {
        if hash_bytes(&fs::read(root.join(path)).map_err(|e| e.to_string())?) != *hash {
            return Err("Source changed during compilation; retry".into());
        }
    }
    let mut limitations: Vec<String> = LIMITATIONS.iter().map(|s| s.to_string()).collect();
    limitations.push(format!("{unresolved} host imports are external or unresolved; no absence-of-dependency claim is made for them."));
    limitations.push(format!("Compiled {} feature modules and analyzed {} source files. Revision covers this compilation's inputs, not every repository file.",modules.len(),host_files.len()));
    Ok(Program {
        modules,
        host_files,
        edges,
        diagnostics,
        revision: content_digest(&inputs),
        inputs,
        limitations,
    })
}
