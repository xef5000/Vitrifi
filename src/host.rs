//! Native syntax adapters. These facts do not imply resolved runtime call targets.
use crate::model::{Diagnostic, HostFile, HostImport, HostSymbol, Location};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Language, Node, Parser};

pub fn language_for(path: &str) -> Option<&'static str> {
    match Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => Some("python"),
        "rb" => Some("ruby"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "java" => Some("java"),
        _ => None,
    }
}

fn language(name: &str, tsx: bool) -> Language {
    match name {
        "python" => tree_sitter_python::LANGUAGE.into(),
        "ruby" => tree_sitter_ruby::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" if tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        _ => unreachable!(),
    }
}

fn text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.start_byte()..node.end_byte()).unwrap_or("")
}
fn field<'a>(node: Node, name: &str, source: &'a str) -> &'a str {
    node.child_by_field_name(name)
        .map(|n| text(n, source))
        .unwrap_or("")
}
fn nodes(root: Node) -> Vec<Node> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        result.push(node);
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
    result
}
fn named(node: Node) -> Vec<Node> {
    (0..node.named_child_count())
        .filter_map(|i| node.named_child(i))
        .collect()
}
fn named_field<'a>(node: Node<'a>, name: &str) -> Vec<Node<'a>> {
    (0..node.child_count())
        .filter_map(|i| {
            if node.field_name_for_child(i as u32) == Some(name) {
                node.child(i)
            } else {
                None
            }
        })
        .collect()
}
fn literal(node: Option<Node>, source: &str) -> Option<String> {
    let node = node?;
    if !matches!(node.kind(), "string" | "string_literal") {
        return None;
    }
    if nodes(node).iter().any(|n| {
        matches!(
            n.kind(),
            "interpolation" | "string_interpolation" | "template_substitution"
        )
    }) {
        return None;
    }
    let value = text(node, source);
    let quote = value.as_bytes().first()?;
    if value.len() < 2 || !matches!(quote, b'\'' | b'"') || value.as_bytes().last() != Some(quote) {
        return None;
    }
    if *quote == b'"' {
        if let Ok(decoded) = serde_json::from_str::<String>(value) {
            return Some(decoded);
        }
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains('\\') {
        None
    } else {
        Some(inner.to_owned())
    }
}

fn declaration(node: Node, lang: &str, source: &str) -> Option<(String, usize, usize)> {
    let mut name = field(node, "name", source).to_owned();
    let mut start = node.start_byte();
    let mut line = node.start_position().row + 1;
    let supported = match lang {
        "python" => matches!(node.kind(), "class_definition" | "function_definition"),
        "ruby" => matches!(
            node.kind(),
            "class" | "module" | "method" | "singleton_method"
        ),
        "javascript" | "typescript" => match node.kind() {
            "class_declaration"
            | "class"
            | "interface_declaration"
            | "abstract_class_declaration"
            | "enum_declaration"
            | "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "function_signature" => true,
            "arrow_function" | "function_expression" | "generator_function" => {
                if let Some(parent) = node.parent() {
                    match parent.kind() {
                        "variable_declarator"
                        | "public_field_definition"
                        | "field_definition"
                        | "pair" => {
                            name = field(
                                parent,
                                if parent.kind() == "pair" {
                                    "key"
                                } else {
                                    "name"
                                },
                                source,
                            )
                            .to_owned();
                            start = parent.start_byte();
                            line = parent.start_position().row + 1;
                        }
                        "export_statement" if name.is_empty() => name = "default".to_owned(),
                        _ => {}
                    }
                }
                !name.is_empty()
            }
            _ => false,
        },
        "java" => matches!(
            node.kind(),
            "class_declaration"
                | "interface_declaration"
                | "record_declaration"
                | "enum_declaration"
                | "annotation_type_declaration"
                | "method_declaration"
                | "constructor_declaration"
                | "compact_constructor_declaration"
                | "annotation_type_element_declaration"
        ),
        _ => false,
    };
    if supported && !name.is_empty() {
        Some((name, start, line))
    } else {
        None
    }
}

fn call(node: Node, lang: &str, source: &str) -> Option<String> {
    let name = match (lang, node.kind()) {
        ("python", "call") => field(node, "function", source).to_owned(),
        ("ruby", "call") => {
            let receiver = field(node, "receiver", source);
            format!(
                "{}{}{}",
                receiver,
                if receiver.is_empty() { "" } else { "." },
                field(node, "method", source)
            )
        }
        ("javascript" | "typescript", "call_expression") => {
            field(node, "function", source).to_owned()
        }
        ("javascript" | "typescript", "new_expression") => {
            field(node, "constructor", source).to_owned()
        }
        ("java", "method_invocation") => {
            let receiver = field(node, "object", source);
            format!(
                "{}{}{}",
                receiver,
                if receiver.is_empty() { "" } else { "." },
                field(node, "name", source)
            )
        }
        ("java", "object_creation_expression") => field(node, "type", source).to_owned(),
        _ => return None,
    };
    if name.is_empty() {
        None
    } else {
        Some(name.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

fn imports(root: Node, lang: &str, source: &str) -> Vec<HostImport> {
    let mut result = Vec::new();
    for node in nodes(root) {
        let line = node.start_position().row + 1;
        let mut push = |module: String, kind: &str, names: Vec<String>| {
            result.push(HostImport {
                module,
                kind: kind.to_owned(),
                names,
                line,
            });
        };
        match (lang, node.kind()) {
            ("python", "import_statement") => {
                for item in named_field(node, "name") {
                    push(
                        if item.kind() == "aliased_import" {
                            field(item, "name", source)
                        } else {
                            text(item, source)
                        }
                        .to_owned(),
                        "import",
                        Vec::new(),
                    );
                }
            }
            ("python", "import_from_statement") => {
                let mut names = named_field(node, "name")
                    .iter()
                    .map(|&item| {
                        if item.kind() == "aliased_import" {
                            field(item, "name", source)
                        } else {
                            text(item, source)
                        }
                        .to_owned()
                    })
                    .collect::<Vec<_>>();
                if named(node).iter().any(|n| n.kind() == "wildcard_import") {
                    names.push("*".to_owned());
                }
                push(field(node, "module_name", source).to_owned(), "from", names);
            }
            (
                "javascript" | "typescript",
                "import_statement" | "export_statement" | "import_require_clause",
            ) => {
                if let Some(module) = literal(node.child_by_field_name("source"), source) {
                    push(
                        module,
                        if node.kind() == "export_statement" {
                            "export"
                        } else {
                            "import"
                        },
                        Vec::new(),
                    );
                }
            }
            ("javascript" | "typescript", "call_expression") => {
                let function = field(node, "function", source);
                if matches!(function, "require" | "import") {
                    let arg = node
                        .child_by_field_name("arguments")
                        .and_then(|args| args.named_child(0));
                    if let Some(module) = literal(arg, source) {
                        push(module, function, Vec::new());
                    }
                }
            }
            ("ruby", "call") => {
                let method = field(node, "method", source);
                if matches!(method, "require" | "require_relative" | "load")
                    && matches!(field(node, "receiver", source), "" | "Kernel")
                {
                    let arg = node
                        .child_by_field_name("arguments")
                        .and_then(|args| args.named_child(0));
                    if let Some(module) = literal(arg, source) {
                        push(module, method, Vec::new());
                    }
                }
            }
            ("java", "import_declaration") => {
                let children = named(node);
                let mut module = children
                    .iter()
                    .find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"))
                    .map(|&n| text(n, source))
                    .unwrap_or("")
                    .to_owned();
                if children.iter().any(|n| n.kind() == "asterisk") {
                    module.push_str(".*");
                }
                let kind = if (0..node.child_count())
                    .filter_map(|i| node.child(i))
                    .any(|n| n.kind() == "static")
                {
                    "static"
                } else {
                    "import"
                };
                push(module, kind, Vec::new());
            }
            _ => {}
        }
    }
    result
}

pub fn analyze(path: &str, source: &str) -> HostFile {
    let lang = language_for(path).unwrap_or("unknown");
    let mut result = HostFile {
        path: path.to_owned(),
        language: lang.to_owned(),
        package: None,
        symbols: Vec::new(),
        imports: Vec::new(),
        diagnostics: Vec::new(),
    };
    let error = |code: &str, message: String, line: usize, column: usize| Diagnostic {
        code: code.to_owned(),
        message,
        location: Location {
            path: path.to_owned(),
            line,
            column,
        },
    };
    if lang == "unknown" {
        result.diagnostics.push(error(
            "UNSUPPORTED_LANGUAGE",
            format!("No syntax adapter for {path}"),
            1,
            1,
        ));
        return result;
    }
    let mut parser = Parser::new();
    if let Err(err) =
        parser.set_language(&language(lang, path.to_ascii_lowercase().ends_with(".tsx")))
    {
        result
            .diagnostics
            .push(error("PARSER_UNAVAILABLE", err.to_string(), 1, 1));
        return result;
    }
    let Some(tree) = parser.parse(source, None) else {
        result.diagnostics.push(error(
            "PARSER_FAILED",
            "Host parser returned no syntax tree".to_owned(),
            1,
            1,
        ));
        return result;
    };
    let root = tree.root_node();
    if root.has_error() {
        for node in nodes(root)
            .into_iter()
            .filter(|n| n.is_error() || n.is_missing())
            .take(20)
        {
            result.diagnostics.push(error(
                "HOST_SYNTAX",
                format!(
                    "{lang} source has {}; analysis is partial",
                    if node.is_missing() {
                        "missing syntax"
                    } else {
                        "invalid syntax"
                    }
                ),
                node.start_position().row + 1,
                node.start_position().column + 1,
            ));
        }
    }
    result.imports = imports(root, lang, source);
    if lang == "java" {
        result.package = named(root)
            .into_iter()
            .find(|n| n.kind() == "package_declaration")
            .and_then(|n| {
                named(n)
                    .into_iter()
                    .find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"))
                    .map(|n| text(n, source).to_owned())
            });
    }
    let mut stack = vec![(root, Vec::<String>::new(), None::<usize>)];
    while let Some((node, mut scope, mut owner)) = stack.pop() {
        if lang == "typescript" && matches!(node.kind(), "internal_module" | "module") {
            let name = field(node, "name", source);
            if !name.is_empty() {
                scope.push(name.trim_matches(['\'', '"']).to_owned());
            }
        } else if matches!(lang, "javascript" | "typescript") && node.kind() == "object" {
            if let Some(parent) = node.parent() {
                if matches!(
                    parent.kind(),
                    "variable_declarator" | "public_field_definition" | "field_definition" | "pair"
                ) {
                    let name = field(
                        parent,
                        if parent.kind() == "pair" {
                            "key"
                        } else {
                            "name"
                        },
                        source,
                    );
                    if !name.is_empty() {
                        scope.push(name.to_owned());
                    }
                }
            }
        }
        if let Some((name, start, line)) = declaration(node, lang, source) {
            scope.push(name);
            let end = node
                .child_by_field_name("body")
                .map(|n| n.start_byte())
                .unwrap_or(node.end_byte());
            let signature = source
                .get(start..end)
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .trim_end_matches([';', ':'])
                .to_owned();
            owner = Some(result.symbols.len());
            result.symbols.push(HostSymbol {
                name: scope.join("."),
                signature,
                line,
                end_line: node.end_position().row + 1,
                calls: Vec::new(),
            });
        }
        if let (Some(index), Some(call)) = (owner, call(node, lang, source)) {
            result.symbols[index].calls.push(call);
        }
        for child in named(node).into_iter().rev() {
            stack.push((child, scope.clone(), owner));
        }
    }
    let mut counts = HashMap::new();
    for symbol in &result.symbols {
        *counts.entry(symbol.name.clone()).or_insert(0) += 1;
    }
    let mut seen = HashMap::new();
    for symbol in &mut result.symbols {
        if counts[&symbol.name] > 1 {
            symbol.name.push('~');
            symbol
                .name
                .push_str(&format!("{:x}", Sha256::digest(symbol.signature.as_bytes()))[..10]);
            let count = seen.entry(symbol.name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                symbol.name.push_str(&format!("-{count}"));
            }
        }
    }
    result
}

fn clean(path: &str) -> Option<String> {
    let mut components = Vec::new();
    for item in path.split('/') {
        match item {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            other => components.push(other),
        }
    }
    Some(components.join("/"))
}
fn join(a: &str, b: &str) -> Option<String> {
    clean(&format!("{a}/{b}"))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

/// Construct once per host-file inventory. No repeated full repository scan per import.
pub struct ImportResolver {
    paths: HashMap<String, HashSet<String>>,
    java_types: HashMap<String, BTreeSet<String>>,
    java_packages: HashMap<String, BTreeSet<String>>,
}

impl ImportResolver {
    pub fn new(files: &[HostFile]) -> Self {
        let mut result = Self {
            paths: HashMap::new(),
            java_types: HashMap::new(),
            java_packages: HashMap::new(),
        };
        for file in files {
            let key = if matches!(file.language.as_str(), "javascript" | "typescript") {
                "ecmascript"
            } else {
                &file.language
            };
            result
                .paths
                .entry(key.to_owned())
                .or_default()
                .insert(file.path.clone());
            if file.language == "java" {
                let package = file.package.as_deref().unwrap_or("");
                result
                    .java_packages
                    .entry(package.to_owned())
                    .or_default()
                    .insert(file.path.clone());
                for symbol in file
                    .symbols
                    .iter()
                    .filter(|s| !s.name.contains('.') && !s.name.contains('~'))
                {
                    let name = if package.is_empty() {
                        symbol.name.clone()
                    } else {
                        format!("{package}.{}", symbol.name)
                    };
                    result
                        .java_types
                        .entry(name)
                        .or_default()
                        .insert(file.path.clone());
                }
            }
        }
        result
    }

    pub fn resolve(&self, file: &HostFile, imp: &HostImport) -> Vec<String> {
        let lang = file.language.as_str();
        let key = if matches!(lang, "javascript" | "typescript") {
            "ecmascript"
        } else {
            lang
        };
        let Some(known) = self.paths.get(key) else {
            return Vec::new();
        };
        let module = imp.module.as_str();
        if module.is_empty() {
            return Vec::new();
        }
        let parent = parent(&file.path);
        let existing = |paths: Vec<String>| -> Vec<String> {
            paths
                .into_iter()
                .filter(|p| known.contains(p))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        match lang {
            "python" => {
                let dots = module.bytes().take_while(|c| *c == b'.').count();
                let mut base = module[dots..].replace('.', "/");
                if dots > 0 {
                    let mut folder = parent;
                    for _ in 0..dots - 1 {
                        if folder.is_empty() {
                            return Vec::new();
                        }
                        folder = crate::host::parent(folder);
                    }
                    let Some(path) = join(folder, &base) else {
                        return Vec::new();
                    };
                    base = path;
                }
                let prefixes: &[&str] = if dots > 0 { &[""] } else { &["", "src", "lib"] };
                let mut groups = Vec::new();
                for prefix in prefixes {
                    let Some(target) = join(prefix, &base) else {
                        continue;
                    };
                    let mut modules = vec![target.clone()];
                    for name in imp.names.iter().filter(|n| n.as_str() != "*") {
                        if let Some(child) = join(&target, &name.replace('.', "/")) {
                            modules.push(child);
                        }
                    }
                    let mut found = BTreeSet::new();
                    for item in modules {
                        let hits = existing(vec![
                            format!("{item}.py"),
                            join(&item, "__init__.py").unwrap(),
                        ]);
                        if hits.len() > 1 {
                            return Vec::new();
                        }
                        if !hits.is_empty() {
                            found.extend(hits);
                            let mut folder = crate::host::parent(&item);
                            loop {
                                let init = join(folder, "__init__.py").unwrap();
                                if known.contains(&init) {
                                    found.insert(init);
                                }
                                if folder.is_empty() {
                                    break;
                                }
                                folder = crate::host::parent(folder);
                            }
                        }
                    }
                    if !found.is_empty() {
                        groups.push(found);
                    }
                }
                if groups.len() == 1 {
                    groups.remove(0).into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            "javascript" | "typescript" => {
                if !module.starts_with('.') {
                    return Vec::new();
                }
                let Some(mut base) = join(parent, module) else {
                    return Vec::new();
                };
                let mut candidates = vec![base.clone()];
                for ext in [".js", ".mjs", ".cjs"] {
                    if base.ends_with(ext) {
                        base.truncate(base.len() - ext.len());
                        break;
                    }
                }
                candidates.extend(
                    [".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx"]
                        .iter()
                        .map(|ext| format!("{base}{ext}")),
                );
                candidates.extend(
                    [".js", ".jsx", ".ts", ".tsx"]
                        .iter()
                        .map(|ext| format!("{base}/index{ext}")),
                );
                candidates
                    .into_iter()
                    .find(|p| known.contains(p))
                    .into_iter()
                    .collect()
            }
            "ruby" => {
                if module.starts_with('/') || (imp.kind == "load" && !module.ends_with(".rb")) {
                    return Vec::new();
                }
                let base = module.strip_suffix(".rb").unwrap_or(module);
                if imp.kind == "require_relative" {
                    return join(parent, &format!("{base}.rb"))
                        .filter(|p| known.contains(p))
                        .into_iter()
                        .collect();
                }
                let hits = existing(
                    [format!("{base}.rb"), format!("lib/{base}.rb")]
                        .iter()
                        .filter_map(|p| clean(p))
                        .collect(),
                );
                if hits.len() == 1 {
                    hits
                } else {
                    Vec::new()
                }
            }
            "java" => {
                let mut target = module.strip_suffix(".*").unwrap_or(module);
                loop {
                    if let Some(paths) = self.java_types.get(target) {
                        return if paths.len() == 1 {
                            paths.iter().cloned().collect()
                        } else {
                            Vec::new()
                        };
                    }
                    if let Some((head, _)) = target.rsplit_once('.') {
                        target = head;
                    } else {
                        break;
                    }
                }
                if let Some(package) = module.strip_suffix(".*") {
                    return self
                        .java_packages
                        .get(package)
                        .map(|p| p.iter().cloned().collect())
                        .unwrap_or_default();
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

pub fn resolve_import(file: &HostFile, import: &HostImport, files: &[HostFile]) -> Vec<String> {
    ImportResolver::new(files).resolve(file, import)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parsed(path: &str, source: &str) -> HostFile {
        let result = analyze(path, source);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        result
    }
    fn symbol<'a>(file: &'a HostFile, name: &str) -> &'a HostSymbol {
        file.symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("Missing {name}: {:?}", file.symbols))
    }
    fn resolved(caller: HostFile, others: Vec<HostFile>) -> Vec<Vec<String>> {
        let files = [vec![caller.clone()], others].concat();
        let resolver = ImportResolver::new(&files);
        caller
            .imports
            .iter()
            .map(|i| resolver.resolve(&caller, i))
            .collect()
    }
    #[test]
    fn python_scopes_signatures_import_aliases_and_calls() {
        let file = parsed("app.py", "from .storage import save as persist\nclass Cart:\n    def total(self, amount: int) -> int:\n        def tax():\n            return calculate()\n        return persist(amount)\n");
        assert_eq!(file.imports[0].module, ".storage");
        assert_eq!(file.imports[0].names, ["save"]);
        assert_eq!(symbol(&file, "Cart.total").calls, ["persist"]);
        assert_eq!(symbol(&file, "Cart.total.tax").calls, ["calculate"]);
        assert!(symbol(&file, "Cart.total").signature.contains("-> int"));
        assert_eq!(symbol(&file, "Cart.total").line, 3);
    }
    #[test]
    fn ruby_namespace_singleton_and_literal_requires() {
        let file = parsed("worker.rb", "require_relative '../store/cart'\nmodule Jobs\n  class Audit\n    def self.perform(event)\n      client.record event\n    end\n  end\nend\n");
        assert_eq!(file.imports[0].kind, "require_relative");
        assert_eq!(symbol(&file, "Jobs.Audit.perform").calls, ["client.record"]);
    }
    #[test]
    fn javascript_arrows_methods_reexports_and_comment_negatives() {
        let file = parsed("view.jsx", "// function fake() {}\nexport {x} from './domain.js';\nconst db = require('./db.cjs');\nconst apply =\n  (code) => save(code);\nconst ui = { submit() { return apply('x'); } };\nclass View { render() { return <div/>; } }\n");
        assert_eq!(file.imports.len(), 2);
        assert_eq!(symbol(&file, "apply").line, 4);
        assert_eq!(symbol(&file, "ui.submit").calls, ["apply"]);
        assert!(file.symbols.iter().all(|s| !s.name.contains("fake")));
        assert!(symbol(&file, "View.render").signature.contains("render"));
    }
    #[test]
    fn typescript_typed_namespace_interface_and_tsx() {
        let file = parsed("checkout.ts", "export namespace Checkout { export interface Result { total: number; } export const api = { apply(code: string): Result { return price(code); } }; }\n");
        assert!(symbol(&file, "Checkout.api.apply")
            .signature
            .contains("code: string"));
        assert!(symbol(&file, "Checkout.Result")
            .signature
            .contains("interface"));
        let tsx = parsed(
            "view.tsx",
            "export const View = (props: {total: number}) => <p>{props.total}</p>;",
        );
        assert!(symbol(&tsx, "View").signature.contains("props:"));
    }
    #[test]
    fn java_packages_static_imports_and_stable_overloads() {
        let source = "package checkout; import static store.DB.save; class Cart { int total(int n) { return save(n); } int total(String n) { return 0; } }";
        let a = parsed("Cart.java", source);
        let b = parsed("Cart.java", &format!("\n{source}"));
        assert_eq!(a.package.as_deref(), Some("checkout"));
        assert_eq!(a.imports[0].kind, "static");
        let names = a.symbols.iter().map(|s| &s.name).collect::<Vec<_>>();
        assert_eq!(names, b.symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
        assert_eq!(
            a.symbols
                .iter()
                .filter(|s| s.name.starts_with("Cart.total~"))
                .count(),
            2
        );
    }
    #[test]
    fn syntax_failures_are_not_clean_indexes() {
        for (path, source) in [
            ("x.py", "def f(:\n"),
            ("x.rb", "def f(\n"),
            ("x.js", "function {"),
            ("x.ts", "function {"),
            ("X.java", "class {"),
        ] {
            let file = analyze(path, source);
            assert!(!file.diagnostics.is_empty(), "{path}");
            assert!(file
                .diagnostics
                .iter()
                .all(|d| d.location.path == path && d.location.line > 0));
        }
    }
    #[test]
    fn computed_imports_are_unknown_not_fabricated_edges() {
        let js = parsed("app.js", "const x = require(path); import(`./${name}.js`);");
        let rb = parsed("app.rb", "require \"./#{name}\"\n");
        assert!(js.imports.is_empty());
        assert!(rb.imports.is_empty());
    }
    #[test]
    fn python_resolution_is_conservative_and_includes_initializers() {
        let caller = parsed("workflow/app.py", "import domain.discounts\nimport json\nimport storage\nfrom ... import storage\nfrom ..domain import discounts\n");
        let actual = resolved(
            caller,
            vec![
                parsed("domain/__init__.py", ""),
                parsed("domain/discounts.py", ""),
                parsed("tools/json.py", ""),
                parsed("workflow/storage.py", ""),
            ],
        );
        assert_eq!(
            actual,
            vec![
                vec!["domain/__init__.py", "domain/discounts.py"],
                vec![],
                vec![],
                vec![],
                vec!["domain/__init__.py", "domain/discounts.py"]
            ]
        );
    }
    #[test]
    fn python_competing_roots_remain_unresolved() {
        let caller = parsed("app.py", "import store\n");
        assert_eq!(
            resolved(
                caller,
                vec![parsed("store.py", ""), parsed("src/store.py", "")]
            ),
            vec![Vec::<String>::new()]
        );
        let caller = parsed("src/workflow/app.py", "from domain.discount import price\n");
        assert_eq!(
            resolved(caller, vec![parsed("src/domain/discount.py", "")]),
            vec![vec!["src/domain/discount.py"]]
        );
    }
    #[test]
    fn javascript_typescript_resolve_relative_exports_and_js_suffix() {
        let caller = parsed("workflow/app.ts", "export {price} from '../domain/price.js'; import '@app/store'; const old = require('../store/old.cjs');");
        assert_eq!(
            resolved(
                caller,
                vec![parsed("domain/price.ts", ""), parsed("store/old.cjs", "")]
            ),
            vec![vec!["domain/price.ts"], vec![], vec!["store/old.cjs"]]
        );
    }
    #[test]
    fn ruby_literal_paths_resolve_and_ambiguous_load_paths_do_not() {
        let caller = parsed("jobs/app.rb", "require_relative '../store/cart'\nrequire './store/cart'\nrequire 'library'\nrequire '/store/cart'\nload './store/cart'\nload './store/cart.rb'\n");
        assert_eq!(
            resolved(
                caller,
                vec![
                    parsed("store/cart.rb", ""),
                    parsed("library.rb", ""),
                    parsed("lib/library.rb", "")
                ]
            ),
            vec![
                vec!["store/cart.rb"],
                vec!["store/cart.rb"],
                vec![],
                vec![],
                vec![],
                vec!["store/cart.rb"]
            ]
        );
    }
    #[test]
    fn java_package_and_static_imports_use_declarations() {
        let caller = parsed(
            "app/Checkout.java",
            "import static store.DB.save; import domain.*; class Checkout {}",
        );
        assert_eq!(
            resolved(
                caller,
                vec![
                    parsed("src/DB.java", "package store; class DB {}"),
                    parsed("src/Cart.java", "package domain; class Cart {}"),
                    parsed("src/other/Cart.java", "package unrelated; class Cart {}")
                ]
            ),
            vec![vec!["src/DB.java"], vec!["src/Cart.java"]]
        );
    }
}
