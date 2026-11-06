//! Independently authored Java architecture targets and pure conformance checks.
//! Parsing never reads implementation files. Conformance never runs host code.
use crate::{
    derived::{DerivedOperation, DerivedProgram, PolicyStatus},
    derived_model::{ResourceEffect, SourcePolicy},
    model::{Diagnostic, Location},
};
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetFeature {
    pub path: String,
    pub name: String,
    pub purpose: String,
    pub covers: Vec<String>,
    pub resources: Vec<TargetResource>,
    pub boundaries: Vec<TargetBoundary>,
    pub operations: Vec<TargetOperation>,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetResource {
    pub name: String,
    pub table: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetBoundary {
    pub name: String,
    pub classes: Vec<String>,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetParameter {
    pub name: String,
    pub native_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetOperation {
    pub name: String,
    pub parameters: Vec<TargetParameter>,
    pub returns: String,
    pub binding: String,
    pub effects: Vec<ResourceEffect>,
    pub dependencies: Vec<String>,
    pub policies: Vec<SourcePolicy>,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformedOperation {
    pub feature: String,
    pub name: String,
    pub binding: String,
    pub operation_id: String,
    pub policies: Vec<SourcePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetPolicyResult {
    pub feature: String,
    pub operation: String,
    pub binding: String,
    pub policy: SourcePolicy,
    pub status: PolicyStatus,
    pub effect_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceReport {
    pub status: String,
    pub diagnostics: Vec<Diagnostic>,
    pub operations: Vec<ConformedOperation>,
    pub policy_results: Vec<TargetPolicyResult>,
}

fn diagnostic(code: &str, message: impl Into<String>, location: &Location) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        location: location.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Word(String),
    String(String),
    Punct(char),
    Arrow,
    End,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    at: Location,
}

fn tokenize(path: &str, source: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut offset = 0;
    let mut line = 1;
    let mut column = 1;
    let mut result = Vec::new();
    let bump = |offset: &mut usize, line: &mut usize, column: &mut usize| {
        let c = source[*offset..].chars().next().unwrap();
        *offset += c.len_utf8();
        if c == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
        c
    };
    while offset < source.len() {
        let at = Location {
            path: path.into(),
            line,
            column,
        };
        let tail = &source[offset..];
        let first = tail.chars().next().unwrap();
        if first.is_whitespace() {
            bump(&mut offset, &mut line, &mut column);
            continue;
        }
        if tail.starts_with("//") {
            while offset < source.len() && !source[offset..].starts_with('\n') {
                bump(&mut offset, &mut line, &mut column);
            }
            continue;
        }
        if tail.starts_with("/*") {
            bump(&mut offset, &mut line, &mut column);
            bump(&mut offset, &mut line, &mut column);
            while offset < source.len() && !source[offset..].starts_with("*/") {
                bump(&mut offset, &mut line, &mut column);
            }
            if offset == source.len() {
                return Err(diagnostic(
                    "TARGET_PARSE",
                    "Unterminated block comment",
                    &at,
                ));
            }
            bump(&mut offset, &mut line, &mut column);
            bump(&mut offset, &mut line, &mut column);
            continue;
        }
        let kind = if first == '"' {
            let start = offset;
            bump(&mut offset, &mut line, &mut column);
            let mut closed = false;
            while offset < source.len() {
                match bump(&mut offset, &mut line, &mut column) {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\n' | '\r' => {
                        return Err(diagnostic(
                            "TARGET_PARSE",
                            "Unescaped newline in string",
                            &at,
                        ))
                    }
                    '\\' => {
                        if offset == source.len() {
                            break;
                        }
                        bump(&mut offset, &mut line, &mut column);
                    }
                    _ => {}
                }
            }
            if !closed {
                return Err(diagnostic("TARGET_PARSE", "Unterminated string", &at));
            }
            let value = serde_json::from_str(&source[start..offset])
                .map_err(|e| diagnostic("TARGET_PARSE", format!("Invalid string: {e}"), &at))?;
            Kind::String(value)
        } else if first.is_ascii_alphabetic() || first == '_' {
            let start = offset;
            while offset < source.len()
                && source[offset..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                bump(&mut offset, &mut line, &mut column);
            }
            Kind::Word(source[start..offset].into())
        } else if tail.starts_with("->") {
            bump(&mut offset, &mut line, &mut column);
            bump(&mut offset, &mut line, &mut column);
            Kind::Arrow
        } else if "{}[](),;:.".contains(first) {
            bump(&mut offset, &mut line, &mut column);
            Kind::Punct(first)
        } else {
            return Err(diagnostic(
                "TARGET_PARSE",
                format!("Unexpected character {first:?}"),
                &at,
            ));
        };
        result.push(Token { kind, at });
    }
    result.push(Token {
        kind: Kind::End,
        at: Location {
            path: path.into(),
            line,
            column,
        },
    });
    Ok(result)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn at(&self) -> &Location {
        &self.tokens[self.cursor].at
    }
    fn kind(&self) -> &Kind {
        &self.tokens[self.cursor].kind
    }
    fn take(&mut self) -> Token {
        let result = self.tokens[self.cursor].clone();
        if result.kind != Kind::End {
            self.cursor += 1;
        }
        result
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        diagnostic("TARGET_PARSE", message, self.at())
    }
    fn punct(&mut self, c: char) -> Result<(), Diagnostic> {
        if self.kind() == &Kind::Punct(c) {
            self.take();
            Ok(())
        } else {
            Err(self.error(format!("Expected '{c}'")))
        }
    }
    fn eat(&mut self, c: char) -> bool {
        if self.kind() == &Kind::Punct(c) {
            self.take();
            true
        } else {
            false
        }
    }
    fn word(&mut self) -> Result<String, Diagnostic> {
        match self.take() {
            Token {
                kind: Kind::Word(value),
                ..
            } => Ok(value),
            token => Err(diagnostic(
                "TARGET_PARSE",
                "Expected an identifier",
                &token.at,
            )),
        }
    }
    fn keyword(&mut self, expected: &str) -> Result<(), Diagnostic> {
        let at = self.at().clone();
        if self.word()? == expected {
            Ok(())
        } else {
            Err(diagnostic(
                "TARGET_PARSE",
                format!("Expected '{expected}'"),
                &at,
            ))
        }
    }
    fn string(&mut self) -> Result<String, Diagnostic> {
        match self.take() {
            Token {
                kind: Kind::String(value),
                ..
            } => Ok(value),
            token => Err(diagnostic(
                "TARGET_PARSE",
                "Expected a quoted string",
                &token.at,
            )),
        }
    }
    fn strings(&mut self) -> Result<Vec<String>, Diagnostic> {
        self.punct('[')?;
        let mut result = Vec::new();
        if !self.eat(']') {
            loop {
                result.push(self.string()?);
                if self.eat(']') {
                    break;
                }
                self.punct(',')?;
            }
        }
        Ok(result)
    }
    fn names(&mut self) -> Result<Vec<String>, Diagnostic> {
        self.punct('[')?;
        let mut result = Vec::new();
        if !self.eat(']') {
            loop {
                result.push(self.word()?);
                if self.eat(']') {
                    break;
                }
                self.punct(',')?;
            }
        }
        Ok(result)
    }
    fn effects(&mut self) -> Result<Vec<ResourceEffect>, Diagnostic> {
        self.punct('[')?;
        let mut result = Vec::new();
        if !self.eat(']') {
            loop {
                result.push(ResourceEffect {
                    action: self.word()?,
                    resource: self.word()?,
                });
                if self.eat(']') {
                    break;
                }
                self.punct(',')?;
            }
        }
        Ok(result)
    }
    fn operation(&mut self, location: Location) -> Result<TargetOperation, Diagnostic> {
        let name = self.word()?;
        self.punct('(')?;
        let mut parameters = Vec::new();
        if !self.eat(')') {
            loop {
                let name = self.word()?;
                self.punct(':')?;
                parameters.push(TargetParameter {
                    name,
                    native_type: self.string()?,
                });
                if self.eat(')') {
                    break;
                }
                self.punct(',')?;
            }
        }
        if self.take().kind != Kind::Arrow {
            return Err(self.error("Expected '->'"));
        }
        let returns = self.string()?;
        self.punct('{')?;
        let mut binding = None;
        let mut effects = None;
        let mut dependencies = None;
        let mut policies = Vec::new();
        let mut fields = BTreeSet::new();
        while !self.eat('}') {
            let field = self.word()?;
            if !fields.insert(field.clone()) {
                return Err(self.error(format!("Duplicate operation field '{field}'")));
            }
            match field.as_str() {
                "bind" => {
                    self.keyword("java")?;
                    binding = Some(self.string()?);
                }
                "effects" => effects = Some(self.effects()?),
                "dependencies" => dependencies = Some(self.names()?),
                "forbid" => policies.extend(
                    self.effects()?
                        .into_iter()
                        .map(|effect| SourcePolicy::Forbid { effect }),
                ),
                "no_effect_on_error" => policies.extend(
                    self.effects()?
                        .into_iter()
                        .map(|effect| SourcePolicy::NoEffectOnError { effect }),
                ),
                _ => return Err(self.error(format!("Unknown target operation field '{field}'"))),
            }
            self.punct(';')?;
        }
        Ok(TargetOperation {
            name,
            parameters,
            returns,
            binding: binding.ok_or_else(|| self.error("Target operation requires bind java"))?,
            effects: effects
                .ok_or_else(|| self.error("Target operation requires an explicit effects list"))?,
            dependencies: dependencies.ok_or_else(|| {
                self.error("Target operation requires an explicit dependencies list")
            })?,
            policies,
            location,
        })
    }
    fn feature(&mut self, path: &str) -> Result<TargetFeature, Diagnostic> {
        let location = self.at().clone();
        self.keyword("target")?;
        self.keyword("feature")?;
        let mut name = self.word()?;
        while self.eat('.') {
            name.push('.');
            name.push_str(&self.word()?);
        }
        self.punct('{')?;
        let mut purpose = None;
        let mut covers = Vec::new();
        let mut resources = Vec::new();
        let mut boundaries = Vec::new();
        let mut operations = Vec::new();
        while !self.eat('}') {
            let at = self.at().clone();
            match self.word()?.as_str() {
                "purpose" => {
                    if purpose.is_some() {
                        return Err(self.error("Duplicate purpose"));
                    }
                    purpose = Some(self.string()?);
                    self.punct(';')?;
                }
                "cover" => {
                    self.keyword("java")?;
                    covers.push(self.string()?);
                    self.punct(';')?;
                }
                "resource" => {
                    let name = self.word()?;
                    self.keyword("table")?;
                    let table = self.string()?;
                    self.punct(';')?;
                    resources.push(TargetResource {
                        name,
                        table,
                        location: at,
                    });
                }
                "boundary" => {
                    let name = self.word()?;
                    self.keyword("java")?;
                    let classes = self.strings()?;
                    self.punct(';')?;
                    boundaries.push(TargetBoundary {
                        name,
                        classes,
                        location: at,
                    });
                }
                "operation" => operations.push(self.operation(at)?),
                other => return Err(self.error(format!("Unknown target declaration '{other}'"))),
            }
        }
        if self.kind() != &Kind::End {
            return Err(self.error("Unexpected content after target feature"));
        }
        Ok(TargetFeature {
            path: path.into(),
            name,
            purpose: purpose.ok_or_else(|| self.error("Target feature requires purpose"))?,
            covers,
            resources,
            boundaries,
            operations,
            location,
        })
    }
}

/// Parse and locally validate a target without reading or requiring Java source.
pub fn parse(path: &str, source: &str) -> Result<TargetFeature, Vec<Diagnostic>> {
    if source.len() > 2_000_000 {
        return Err(vec![diagnostic(
            "TARGET_PARSE",
            "Target exceeds the 2 MB source limit",
            &Location {
                path: path.into(),
                line: 1,
                column: 1,
            },
        )]);
    }
    let tokens = tokenize(path, source).map_err(|d| vec![d])?;
    let target = Parser { tokens, cursor: 0 }
        .feature(path)
        .map_err(|d| vec![d])?;
    let errors = lint(std::slice::from_ref(&target));
    if errors.is_empty() {
        Ok(target)
    } else {
        Err(errors)
    }
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn native_class(value: &str) -> bool {
    value.contains('.') && value.split('.').all(identifier)
}

fn normalized(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Parse the supported native type literals and derive their erased binding name.
/// Unbound type variables and type-use annotations need adapter semantics we do
/// not have; rejecting them is safer than guessing their erasure.
fn native_type(value: &str, allow_void: bool) -> Result<String, String> {
    struct TypeParser<'a> {
        source: &'a str,
        offset: usize,
    }
    impl TypeParser<'_> {
        fn skip(&mut self) {
            while self
                .source
                .as_bytes()
                .get(self.offset)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.offset += 1;
            }
        }
        fn eat(&mut self, c: u8) -> bool {
            self.skip();
            if self.source.as_bytes().get(self.offset) == Some(&c) {
                self.offset += 1;
                true
            } else {
                false
            }
        }
        fn name(&mut self) -> String {
            self.skip();
            let start = self.offset;
            while self
                .source
                .as_bytes()
                .get(self.offset)
                .is_some_and(|c| c.is_ascii_alphanumeric() || b"._$".contains(c))
            {
                self.offset += 1;
            }
            self.source[start..self.offset].into()
        }
        fn ty(&mut self, depth: usize, allow_void: bool, wildcard: bool) -> Result<String, String> {
            if depth > 64 {
                return Err("Native type nesting exceeds 64 levels".into());
            }
            if wildcard && self.eat(b'?') {
                let saved = self.offset;
                let bound = self.name();
                if bound == "extends" || bound == "super" {
                    let bound_type = self.ty(depth + 1, false, false)?;
                    if !bound_type.contains('.') && !bound_type.ends_with("[]") {
                        return Err("Wildcard bounds must be reference types".into());
                    }
                } else {
                    self.offset = saved;
                }
                return Ok("java.lang.Object".into());
            }
            let mut name = self.name();
            let primitive = matches!(
                name.as_str(),
                "boolean"
                    | "byte"
                    | "short"
                    | "int"
                    | "long"
                    | "char"
                    | "float"
                    | "double"
                    | "void"
            );
            if !(primitive || native_class(&name)) || name == "void" && !allow_void {
                return Err("Expected a fully qualified Java type or supported primitive; unbound type variables are unsupported".into());
            }
            if self.eat(b'<') {
                if primitive {
                    return Err("Primitive types cannot have generic arguments".into());
                }
                loop {
                    let arg = self.ty(depth + 1, false, true)?;
                    if !arg.contains('.') && !arg.ends_with("[]") {
                        return Err("Java generic arguments must be reference types".into());
                    }
                    if self.eat(b'>') {
                        break;
                    }
                    if !self.eat(b',') {
                        return Err("Expected ',' or '>' in native generic type".into());
                    }
                }
            }
            while self.eat(b'[') {
                if name == "void" || !self.eat(b']') {
                    return Err("Invalid native array type".into());
                }
                name.push_str("[]");
            }
            Ok(name)
        }
    }
    let mut parser = TypeParser {
        source: value,
        offset: 0,
    };
    let erased = parser.ty(0, allow_void, false)?;
    parser.skip();
    if parser.offset != value.len() {
        return Err("Unexpected content in native type literal".into());
    }
    Ok(erased)
}

fn binding_parts(binding: &str) -> Option<(&str, &str, &str)> {
    let (prefix, arguments) = binding.split_once('(')?;
    let arguments = arguments.strip_suffix(')')?;
    if arguments.contains(['(', ')']) {
        return None;
    }
    let (owner, method) = prefix.rsplit_once('.')?;
    if !native_class(owner) || !identifier(method) {
        return None;
    }
    Some((owner, method, arguments))
}

fn source_owner(symbol: &str) -> Option<&str> {
    // Callbacks retain the enclosing method prefix; constructors use <init>.
    symbol
        .split_once('(')?
        .0
        .rsplit_once('.')
        .map(|(owner, _)| owner)
}

fn tables_may_alias(left: &str, right: &str) -> bool {
    left == right
        || ((!left.contains('.') || !right.contains('.'))
            && left.rsplit('.').next() == right.rsplit('.').next())
}

fn policy_effect(policy: &SourcePolicy) -> &ResourceEffect {
    match policy {
        SourcePolicy::Forbid { effect } | SourcePolicy::NoEffectOnError { effect } => effect,
    }
}

fn boundaries(target: &TargetFeature) -> Vec<(&str, Vec<GlobMatcher>)> {
    target
        .boundaries
        .iter()
        .map(|boundary| {
            (
                boundary.name.as_str(),
                boundary
                    .classes
                    .iter()
                    .filter_map(|pattern| Glob::new(pattern).ok().map(|g| g.compile_matcher()))
                    .collect(),
            )
        })
        .collect()
}

fn matching<'a>(boundaries: &'a [(&str, Vec<GlobMatcher>)], owner: &str) -> Vec<&'a str> {
    boundaries
        .iter()
        .filter(|(_, patterns)| patterns.iter().any(|p| p.is_match(owner)))
        .map(|(name, _)| *name)
        .collect()
}

/// Target-only checks, including conflicts between separately authored modules.
pub fn lint(targets: &[TargetFeature]) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    if targets.is_empty() {
        errors.push(diagnostic(
            "TARGET_EMPTY",
            "No architecture targets were supplied",
            &Location {
                path: "<targets>".into(),
                line: 1,
                column: 1,
            },
        ));
    }
    let mut features = BTreeSet::new();
    let mut coverage = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    let mut global_resources = BTreeMap::new();
    let mut global_tables: BTreeMap<String, &String> = BTreeMap::new();
    for target in targets {
        let at = &target.location;
        if !target.name.split('.').all(identifier) || target.name.is_empty() {
            errors.push(diagnostic("TARGET_NAME", "Invalid target feature name", at));
        }
        if !features.insert(&target.name) {
            errors.push(diagnostic(
                "TARGET_DUPLICATE_FEATURE",
                format!("Feature {} has multiple authored targets", target.name),
                at,
            ));
        }
        if target.purpose.trim().is_empty() {
            errors.push(diagnostic(
                "TARGET_PURPOSE",
                "Target purpose must not be empty",
                at,
            ));
        }
        if target.covers.is_empty() {
            errors.push(diagnostic(
                "TARGET_COVERAGE",
                "Target requires at least one covered Java class",
                at,
            ));
        }
        if target.operations.is_empty() {
            errors.push(diagnostic(
                "TARGET_OPERATIONS",
                "Target requires at least one public operation",
                at,
            ));
        }
        let mut names = BTreeSet::new();
        let mut resources = BTreeSet::new();
        let mut boundary_names = BTreeSet::new();
        let mut tables = BTreeSet::new();
        for class in &target.covers {
            if !native_class(class) {
                errors.push(diagnostic(
                    "TARGET_COVERAGE",
                    format!("Coverage must name an exact fully qualified Java class: {class}"),
                    at,
                ));
            }
            if let Some(previous) = coverage.insert(class, &target.name) {
                errors.push(diagnostic(
                    "TARGET_DUPLICATE_COVERAGE",
                    format!("Class {class} is already covered by {previous}"),
                    at,
                ));
            }
        }
        for resource in &target.resources {
            if !identifier(&resource.name) || !names.insert(&resource.name) {
                errors.push(diagnostic(
                    "TARGET_DUPLICATE_NAME",
                    format!("Invalid or duplicate declaration {}", resource.name),
                    &resource.location,
                ));
            }
            resources.insert(&resource.name);
            let table = resource.table.trim().to_ascii_lowercase();
            if table.is_empty()
                || table.split('.').count() > 2
                || !table
                    .split('.')
                    .all(|part| identifier(part) && !part.contains('$'))
            {
                errors.push(diagnostic(
                    "TARGET_RESOURCE",
                    format!("Unsupported SQL table identity {}", resource.table),
                    &resource.location,
                ));
            }
            if !tables.insert(table.clone()) {
                errors.push(diagnostic(
                    "TARGET_RESOURCE",
                    format!("Table {table} has multiple resource declarations"),
                    &resource.location,
                ));
            }
            if let Some(previous) = global_resources.insert(&resource.name, table.clone()) {
                if previous != table {
                    errors.push(diagnostic(
                        "TARGET_RESOURCE_CONFLICT",
                        format!(
                            "Resource {} maps to both {previous} and {table}",
                            resource.name
                        ),
                        &resource.location,
                    ));
                }
            }
            for (other_table, previous) in &global_tables {
                if tables_may_alias(other_table, &table) && *previous != &resource.name {
                    errors.push(diagnostic(
                        "TARGET_RESOURCE_CONFLICT",
                        format!(
                            "Tables {other_table} and {table} may refer to the same SQL table but map to both {previous} and {}; use consistent table identities and resource names",
                            resource.name
                        ),
                        &resource.location,
                    ));
                }
            }
            global_tables.insert(table, &resource.name);
        }
        for boundary in &target.boundaries {
            if !identifier(&boundary.name) || !names.insert(&boundary.name) {
                errors.push(diagnostic(
                    "TARGET_DUPLICATE_NAME",
                    format!("Invalid or duplicate declaration {}", boundary.name),
                    &boundary.location,
                ));
            }
            boundary_names.insert(&boundary.name);
            if boundary.classes.is_empty() {
                errors.push(diagnostic(
                    "TARGET_BOUNDARY",
                    "Boundary requires at least one class pattern",
                    &boundary.location,
                ));
            }
            let mut patterns = BTreeSet::new();
            for pattern in &boundary.classes {
                if pattern.is_empty()
                    || pattern.contains(['/', '\\'])
                    || pattern.chars().any(char::is_whitespace)
                    || Glob::new(pattern).is_err()
                    || !patterns.insert(pattern)
                {
                    errors.push(diagnostic(
                        "TARGET_BOUNDARY",
                        format!("Invalid or duplicate class-name glob {pattern:?}"),
                        &boundary.location,
                    ));
                }
            }
        }
        let matchers = boundaries(target);
        for class in &target.covers {
            if matching(&matchers, class).len() != 1 {
                errors.push(diagnostic(
                    "TARGET_BOUNDARY",
                    format!("Covered class {class} must match exactly one boundary"),
                    at,
                ));
            }
        }
        for operation in &target.operations {
            let at = &operation.location;
            if !identifier(&operation.name) || !names.insert(&operation.name) {
                errors.push(diagnostic(
                    "TARGET_DUPLICATE_NAME",
                    format!("Invalid or duplicate declaration {}", operation.name),
                    at,
                ));
            }
            let mut parameter_names = BTreeSet::new();
            let mut erased = Vec::new();
            for parameter in &operation.parameters {
                if !identifier(&parameter.name) || !parameter_names.insert(&parameter.name) {
                    errors.push(diagnostic(
                        "TARGET_PARAMETER",
                        format!("Invalid or duplicate parameter {}", parameter.name),
                        at,
                    ));
                }
                match native_type(&parameter.native_type, false) {
                    Ok(native) => erased.push(native),
                    Err(reason) => errors.push(diagnostic(
                        "TARGET_NATIVE_TYPE",
                        format!("{}: {reason}", parameter.native_type),
                        at,
                    )),
                }
            }
            if let Err(reason) = native_type(&operation.returns, true) {
                errors.push(diagnostic(
                    "TARGET_NATIVE_TYPE",
                    format!("{}: {reason}", operation.returns),
                    at,
                ));
            }
            match binding_parts(&operation.binding) {
                Some((owner, _, parameters)) => {
                    if !target.covers.iter().any(|c| c == owner) {
                        errors.push(diagnostic(
                            "TARGET_BINDING",
                            format!("Binding owner {owner} is outside target coverage"),
                            at,
                        ));
                    }
                    if parameters != erased.join(",") {
                        errors.push(diagnostic("TARGET_BINDING", format!("Binding parameters must use the declared native types' erasure: ({})", erased.join(",")), at));
                    }
                }
                None => errors.push(diagnostic(
                    "TARGET_BINDING",
                    format!(
                        "Expected a canonical Java method binding, got {}",
                        operation.binding
                    ),
                    at,
                )),
            }
            if let Some(previous) = bindings.insert(&operation.binding, &target.name) {
                errors.push(diagnostic(
                    "TARGET_DUPLICATE_BINDING",
                    format!(
                        "Method {} is already targeted by {previous}",
                        operation.binding
                    ),
                    at,
                ));
            }
            let mut dependency_names = BTreeSet::new();
            for dependency in &operation.dependencies {
                if !boundary_names.contains(dependency) || !dependency_names.insert(dependency) {
                    errors.push(diagnostic(
                        "TARGET_DEPENDENCY",
                        format!("Unknown or duplicate dependency boundary {dependency}"),
                        at,
                    ));
                }
            }
            let mut effects = BTreeSet::new();
            for effect in &operation.effects {
                validate_effect(effect, &resources, at, &mut errors);
                if !effects.insert(effect) {
                    errors.push(diagnostic(
                        "TARGET_EFFECT",
                        "Duplicate permitted effect",
                        at,
                    ));
                }
            }
            let mut policies = BTreeSet::new();
            for policy in &operation.policies {
                validate_effect(policy_effect(policy), &resources, at, &mut errors);
                if !policies.insert(serde_json::to_string(policy).unwrap()) {
                    errors.push(diagnostic("TARGET_POLICY", "Duplicate target policy", at));
                }
            }
        }
    }
    sort_diagnostics(&mut errors);
    errors
}

fn validate_effect(
    effect: &ResourceEffect,
    resources: &BTreeSet<&String>,
    at: &Location,
    errors: &mut Vec<Diagnostic>,
) {
    if !matches!(effect.action.as_str(), "read" | "write") {
        errors.push(diagnostic(
            "TARGET_EFFECT",
            format!(
                "Java target effects currently support JDBC read/write, not {}",
                effect.action
            ),
            at,
        ));
    }
    if !resources.contains(&effect.resource) {
        errors.push(diagnostic(
            "TARGET_RESOURCE",
            format!("Unknown target resource {}", effect.resource),
            at,
        ));
    }
}

fn sort_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        (
            &a.location.path,
            a.location.line,
            a.location.column,
            &a.code,
            &a.message,
        )
            .cmp(&(
                &b.location.path,
                b.location.line,
                b.location.column,
                &b.code,
                &b.message,
            ))
    });
    diagnostics.dedup();
}

fn reachable<'a>(start: &'a str, program: &'a DerivedProgram) -> Vec<&'a DerivedOperation> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(operation) = program.operations.get(id) {
            result.push(operation);
            queue.extend(operation.callees.iter().map(String::as_str));
        }
    }
    result
}

fn effect_path(start: &str, effect: &ResourceEffect, program: &DerivedProgram) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([vec![start.to_owned()]]);
    while let Some(path) = queue.pop_front() {
        let id = path.last().unwrap();
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(operation) = program.operations.get(id) {
            if operation.source.direct_effects.contains(effect) {
                return path;
            }
            for next in &operation.callees {
                let mut child = path.clone();
                child.push(next.clone());
                queue.push_back(child);
            }
        }
    }
    Vec::new()
}

/// Check independently authored constraints against the adapter's derived model.
/// `passed` is static conformance; runtime_required policy results still need
/// fresh execution evidence. Known violations take precedence over unknowns.
pub fn conform(targets: &[TargetFeature], program: &DerivedProgram) -> ConformanceReport {
    let mut report = ConformanceReport {
        status: "passed".into(),
        diagnostics: lint(targets),
        operations: Vec::new(),
        policy_results: Vec::new(),
    };
    if !report.diagnostics.is_empty() {
        report.status = "failed".into();
        return report;
    }
    let selected: BTreeSet<_> = targets
        .iter()
        .flat_map(|target| &target.operations)
        .flat_map(|declared| {
            program.operations.values().filter(move |operation| {
                operation.source.language == "java" && operation.source.symbol == declared.binding
            })
        })
        .flat_map(|operation| reachable(&operation.source.id, program))
        .map(|operation| operation.source.id.clone())
        .collect();
    report
        .diagnostics
        .extend(crate::derived::scoped_diagnostics(program, &selected));
    let Some(inventory) = &program.inventory else {
        report.status = if report.diagnostics.is_empty() {
            "incomplete"
        } else {
            "failed"
        }
        .into();
        report.diagnostics.push(diagnostic("TARGET_INVENTORY", "Analyzer does not provide target coverage/resource inventory; rebuild the Java analyzer", &targets[0].location));
        return report;
    };
    if inventory.version != 2 {
        report.status = if report.diagnostics.is_empty() {
            "incomplete"
        } else {
            "failed"
        }
        .into();
        report.diagnostics.push(diagnostic(
            "TARGET_INVENTORY",
            format!(
                "Unsupported analyzer inventory version {}; version 2 with source-member owner metadata is required, rebuild the Java analyzer",
                inventory.version
            ),
            &targets[0].location,
        ));
        return report;
    }
    let mut incomplete = false;
    for target in targets {
        let mut covered_symbols = BTreeSet::new();
        let expected: BTreeSet<_> = target
            .operations
            .iter()
            .map(|o| o.binding.as_str())
            .collect();
        for class in &target.covers {
            let found: Vec<_> = inventory
                .classes
                .iter()
                .filter(|c| &c.name == class)
                .collect();
            if found.len() != 1 {
                report.diagnostics.push(diagnostic(
                    "TARGET_CLASS",
                    format!("Expected one source class {class}, found {}", found.len()),
                    &target.location,
                ));
                continue;
            }
            let class = found[0];
            for reason in &class.unsupported_api {
                incomplete = true;
                report.diagnostics.push(diagnostic(
                    "TARGET_API_GAP",
                    format!("{}: {reason}", class.name),
                    &target.location,
                ));
            }
            for symbol in &class.public_methods {
                if !covered_symbols.insert(symbol.as_str()) {
                    report.diagnostics.push(diagnostic(
                        "TARGET_INVENTORY",
                        format!("Duplicate public method in analyzer inventory: {symbol}"),
                        &target.location,
                    ));
                }
                if !expected.contains(symbol.as_str()) {
                    report.diagnostics.push(diagnostic(
                        "TARGET_UNDECLARED_ENTRY",
                        format!("Covered public method has no target operation: {symbol}"),
                        &target.location,
                    ));
                }
            }
        }
        if let Some(feature) = program.features.get(&target.name) {
            for root in &feature.roots {
                let Some(operation) = program.operations.get(root) else {
                    continue;
                };
                if !expected.contains(operation.source.symbol.as_str()) {
                    report.diagnostics.push(diagnostic(
                        "TARGET_UNDECLARED_ENTRY",
                        format!(
                            "Feature {} exposes an entry outside its target: {}",
                            target.name, operation.source.symbol
                        ),
                        &target.location,
                    ));
                }
            }
        }
        for resource in &target.resources {
            let table = resource.table.trim().to_ascii_lowercase();
            let candidates: Vec<_> = inventory
                .resources
                .iter()
                .filter(|r| {
                    tables_may_alias(&r.table.trim().to_ascii_lowercase(), &table)
                        || r.resource == resource.name
                })
                .collect();
            if candidates.is_empty() {
                report.diagnostics.push(diagnostic(
                    "TARGET_RESOURCE_MAPPING",
                    format!("No source resource mapping for {} = {table}", resource.name),
                    &resource.location,
                ));
            }
            for actual in candidates {
                if actual.table.trim().to_ascii_lowercase() != table
                    || actual.resource != resource.name
                {
                    report.diagnostics.push(diagnostic(
                        "TARGET_RESOURCE_MAPPING",
                        format!(
                            "Target {} = {table} disagrees with {} mapping {} = {}",
                            resource.name, actual.owner, actual.resource, actual.table
                        ),
                        &resource.location,
                    ));
                }
            }
        }
        let matchers = boundaries(target);
        for class in &inventory.classes {
            let matches = matching(&matchers, &class.name);
            if matches.len() > 1 {
                report.diagnostics.push(diagnostic(
                    "TARGET_AMBIGUOUS_BOUNDARY",
                    format!(
                        "Source class {} matches multiple boundaries: {}",
                        class.name,
                        matches.join(", ")
                    ),
                    &target.location,
                ));
            }
        }
        for declared in &target.operations {
            let at = &declared.location;
            if !covered_symbols.contains(declared.binding.as_str()) {
                report.diagnostics.push(diagnostic(
                    "TARGET_MISSING_ENTRY",
                    format!(
                        "Target binding is not a covered public source method: {}",
                        declared.binding
                    ),
                    at,
                ));
            }
            let found: Vec<_> = program
                .operations
                .values()
                .filter(|o| o.source.language == "java" && o.source.symbol == declared.binding)
                .collect();
            if found.len() != 1 {
                report.diagnostics.push(diagnostic(
                    "TARGET_BINDING",
                    format!(
                        "Expected one implementation for {}, found {}",
                        declared.binding,
                        found.len()
                    ),
                    at,
                ));
                continue;
            }
            let actual = found[0];
            report.operations.push(ConformedOperation {
                feature: target.name.clone(),
                name: declared.name.clone(),
                binding: declared.binding.clone(),
                operation_id: actual.source.id.clone(),
                policies: declared.policies.clone(),
            });
            let actual_types: Vec<_> = actual
                .source
                .parameters
                .iter()
                .map(|p| normalized(&p.annotation))
                .collect();
            let target_types: Vec<_> = declared
                .parameters
                .iter()
                .map(|p| normalized(&p.native_type))
                .collect();
            if actual_types != target_types
                || normalized(&actual.source.returns) != normalized(&declared.returns)
            {
                report.diagnostics.push(diagnostic("TARGET_SIGNATURE", format!("Native signature mismatch for {}: target ({}) -> {}, implementation ({}) -> {}", declared.binding, target_types.join(", "), declared.returns, actual_types.join(", "), actual.source.returns), at));
            }
            if actual.source.feature.as_ref().map(|f| f.name.as_str()) != Some(target.name.as_str())
            {
                report.diagnostics.push(diagnostic(
                    "TARGET_FEATURE_MARKER",
                    format!(
                        "{} must retain its {} feature marker for runtime interception",
                        declared.binding, target.name
                    ),
                    at,
                ));
            }
            for effect in &actual.effective_effects {
                if !declared.effects.contains(effect) {
                    report.diagnostics.push(diagnostic(
                        "TARGET_EFFECT_EXCEEDED",
                        format!(
                            "{} reaches {} {}, outside its permitted effects; path: {}",
                            declared.binding,
                            effect.action,
                            effect.resource,
                            effect_path(&actual.source.id, effect, program).join(" → ")
                        ),
                        at,
                    ));
                }
            }
            for unknown in &actual.reachable_unknowns {
                incomplete = true;
                report.diagnostics.push(diagnostic(
                    "TARGET_ANALYSIS_GAP",
                    format!(
                        "{} reaches unresolved call {}: {}",
                        declared.binding, unknown.expression, unknown.reason
                    ),
                    at,
                ));
            }
            let owner = source_owner(&declared.binding).unwrap();
            let own_boundaries = matching(&matchers, owner);
            let mut allowed: BTreeSet<&str> =
                declared.dependencies.iter().map(String::as_str).collect();
            allowed.extend(own_boundaries);
            for reached in reachable(&actual.source.id, program) {
                let mut references: Vec<_> = reached
                    .source
                    .calls
                    .iter()
                    .filter_map(|call| {
                        call.source_owner
                            .as_deref()
                            .map(|owner| (owner, call.expression.as_str()))
                    })
                    .collect();
                if let Some(owner) = source_owner(&reached.source.symbol) {
                    references.push((owner, &reached.source.symbol));
                } else {
                    incomplete = true;
                    report.diagnostics.push(diagnostic(
                        "TARGET_ANALYSIS_GAP",
                        format!("Cannot identify source owner for {}", reached.source.symbol),
                        at,
                    ));
                }
                for (owner, reference) in references {
                    let matches = matching(&matchers, owner);
                    if matches.len() != 1 {
                        report.diagnostics.push(diagnostic(
                            "TARGET_BOUNDARY_COVERAGE",
                            format!(
                                "{} reaches {owner}, which must match exactly one target boundary",
                                declared.binding
                            ),
                            at,
                        ));
                    } else if !allowed.contains(matches[0]) {
                        report.diagnostics.push(diagnostic(
                            "TARGET_DEPENDENCY_EXCEEDED",
                            format!(
                                "{} reaches {reference} on {owner} in unpermitted boundary {}",
                                declared.binding, matches[0]
                            ),
                            at,
                        ));
                    }
                }
            }
            for policy in &declared.policies {
                if !actual.source.policies.contains(policy) {
                    report.diagnostics.push(diagnostic("TARGET_POLICY_ANNOTATION", format!("{} is missing required source policy {}; removing an annotation cannot remove a target obligation", declared.binding, serde_json::to_string(policy).unwrap()), at));
                }
                let (status, path) = match policy {
                    SourcePolicy::Forbid { effect }
                        if actual.effective_effects.contains(effect) =>
                    {
                        let path = effect_path(&actual.source.id, effect, program);
                        report.diagnostics.push(diagnostic(
                            "TARGET_FORBIDDEN_EFFECT",
                            format!(
                                "Target forbids {} {} on {}; path: {}",
                                effect.action,
                                effect.resource,
                                declared.binding,
                                path.join(" → ")
                            ),
                            at,
                        ));
                        (PolicyStatus::Violated, path)
                    }
                    SourcePolicy::Forbid { .. } if !actual.reachable_unknowns.is_empty() => {
                        (PolicyStatus::Unknown, Vec::new())
                    }
                    SourcePolicy::Forbid { .. } => (PolicyStatus::Passed, Vec::new()),
                    SourcePolicy::NoEffectOnError { .. } => {
                        (PolicyStatus::RuntimeRequired, Vec::new())
                    }
                };
                report.policy_results.push(TargetPolicyResult {
                    feature: target.name.clone(),
                    operation: declared.name.clone(),
                    binding: declared.binding.clone(),
                    policy: policy.clone(),
                    status,
                    effect_path: path,
                });
            }
        }
    }
    sort_diagnostics(&mut report.diagnostics);
    let violated = report
        .diagnostics
        .iter()
        .any(|d| !matches!(d.code.as_str(), "TARGET_API_GAP" | "TARGET_ANALYSIS_GAP"));
    report.status = if violated {
        "failed"
    } else if incomplete {
        "incomplete"
    } else {
        "passed"
    }
    .into();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{derived, derived_model::*};

    const TARGET: &str = r#"
target feature Checkout.Discounts {
  purpose "Preview carts without writes; source metadata cannot grant permission.";
  cover java "example.Discounts";
  resource Carts table "CARTS";
  boundary Workflow java ["example.Discounts", "example.Cart"];
  boundary Storage java ["example.storage.*"];
  operation preview(cartId: "java.lang.String") -> "example.Cart" {
    bind java "example.Discounts.preview(java.lang.String)";
    effects [read Carts];
    dependencies [Storage];
    forbid [write Carts];
  }
}
"#;

    fn effect(action: &str) -> ResourceEffect {
        ResourceEffect {
            action: action.into(),
            resource: "Carts".into(),
        }
    }
    fn target() -> TargetFeature {
        parse("feature.target.vitrifi", TARGET).unwrap()
    }
    fn root() -> SourceOperation {
        SourceOperation {
            id: "src/Discounts.java::example.Discounts.preview(java.lang.String)".into(),
            path: "src/Discounts.java".into(),
            symbol: "example.Discounts.preview(java.lang.String)".into(),
            language: "java".into(),
            line: 1,
            signature: "example.Cart example.Discounts.preview(java.lang.String)".into(),
            parameters: vec![SourceParameter {
                name: "cartId".into(),
                annotation: "java.lang.String".into(),
            }],
            returns: "example.Cart".into(),
            feature: Some(FeatureMarker {
                name: "Checkout.Discounts".into(),
                purpose: "Preview".into(),
            }),
            calls: vec![resolved(&helper().id)],
            direct_effects: vec![],
            policies: vec![SourcePolicy::Forbid {
                effect: effect("write"),
            }],
        }
    }
    fn helper() -> SourceOperation {
        SourceOperation {
            id: "src/Repository.java::example.storage.Repository.load(java.lang.String)".into(),
            path: "src/Repository.java".into(),
            symbol: "example.storage.Repository.load(java.lang.String)".into(),
            language: "java".into(),
            line: 1,
            signature: "example.Cart example.storage.Repository.load(java.lang.String)".into(),
            parameters: vec![SourceParameter {
                name: "cartId".into(),
                annotation: "java.lang.String".into(),
            }],
            returns: "example.Cart".into(),
            feature: None,
            calls: vec![],
            direct_effects: vec![effect("read")],
            policies: vec![],
        }
    }
    fn resolved(id: &str) -> SourceCall {
        SourceCall {
            expression: "repository.load".into(),
            line: 1,
            column: 1,
            target: Some(id.into()),
            source_owner: None,
            resolution: CallResolution::Resolved,
            reason: "source method".into(),
        }
    }
    fn inventory(root: &SourceOperation) -> SourceInventory {
        SourceInventory {
            version: 2,
            classes: vec![
                SourceClass {
                    name: "example.Discounts".into(),
                    path: root.path.clone(),
                    public_methods: vec![root.symbol.clone()],
                    unsupported_api: vec![],
                },
                SourceClass {
                    name: "example.storage.Repository".into(),
                    path: helper().path,
                    public_methods: vec![helper().symbol],
                    unsupported_api: vec![],
                },
            ],
            resources: vec![SourceResource {
                owner: "example.storage.Repository".into(),
                path: "src/Repository.java".into(),
                table: "carts".into(),
                resource: "Carts".into(),
            }],
        }
    }
    fn program(entry: SourceOperation, helpers: Vec<SourceOperation>) -> DerivedProgram {
        let mut operations = vec![entry.clone()];
        operations.extend(helpers);
        derived::build(SourceAnalysis {
            operations,
            diagnostics: vec![],
            inventory: Some(inventory(&entry)),
        })
    }
    fn check(entry: SourceOperation, helpers: Vec<SourceOperation>) -> ConformanceReport {
        conform(&[target()], &program(entry, helpers))
    }
    fn has(report: &ConformanceReport, code: &str) -> bool {
        report.diagnostics.iter().any(|d| d.code == code)
    }

    #[test]
    fn target_lints_without_implementation_and_missing_implementation_fails_conformance() {
        let target = target();
        assert!(lint(std::slice::from_ref(&target)).is_empty());
        let program = derived::build(SourceAnalysis {
            inventory: Some(SourceInventory {
                version: 2,
                classes: vec![],
                resources: vec![],
            }),
            ..SourceAnalysis::default()
        });
        let report = conform(&[target], &program);
        assert_eq!(report.status, "failed");
        assert!(has(&report, "TARGET_CLASS"));
        assert!(has(&report, "TARGET_MISSING_ENTRY"));
        assert!(has(&report, "TARGET_BINDING"));
    }

    #[test]
    fn complete_matching_implementation_passes_without_changing_target() {
        let before = target();
        let frozen = serde_json::to_string(&before).unwrap();
        let report = conform(
            std::slice::from_ref(&before),
            &program(root(), vec![helper()]),
        );
        assert_eq!(report.status, "passed", "{:?}", report.diagnostics);
        assert_eq!(serde_json::to_string(&before).unwrap(), frozen);
        assert_eq!(report.operations.len(), 1);
        assert_eq!(report.policy_results[0].status, PolicyStatus::Passed);
    }

    #[test]
    fn effects_are_upper_bounds_and_private_helper_extraction_is_allowed() {
        let mut root = root();
        let mut private = root.clone();
        private.id = "src/Discounts.java::example.Discounts.helper(java.lang.String)".into();
        private.symbol = "example.Discounts.helper(java.lang.String)".into();
        private.feature = None;
        private.policies.clear();
        root.calls = vec![resolved(&private.id)];
        let mut load = helper();
        load.direct_effects.clear();
        let report = check(root, vec![private, load]);
        assert_eq!(report.status, "passed", "{:?}", report.diagnostics);
    }

    #[test]
    fn transitive_write_violates_target_even_when_source_policy_is_removed() {
        let mut root = root();
        root.policies.clear();
        let mut load = helper();
        load.direct_effects.push(effect("write"));
        let report = check(root, vec![load]);
        assert_eq!(report.status, "failed");
        assert!(has(&report, "TARGET_EFFECT_EXCEEDED"));
        assert!(has(&report, "TARGET_FORBIDDEN_EFFECT"));
        assert!(has(&report, "TARGET_POLICY_ANNOTATION"));
        assert_eq!(report.policy_results[0].effect_path.len(), 2);
    }

    #[test]
    fn native_types_preserve_int_long_and_generic_argument_distinctions() {
        let mut target = target();
        target.operations[0].returns = "int".into();
        let mut entry = root();
        entry.returns = "long".into();
        let report = conform(&[target], &program(entry, vec![helper()]));
        assert!(has(&report, "TARGET_SIGNATURE"));

        let source = TARGET
            .replace(
                "cartId: \"java.lang.String\"",
                "cartId: \"java.util.List<java.lang.String>\"",
            )
            .replace("preview(java.lang.String)", "preview(java.util.List)");
        let target = parse("generic.target.vitrifi", &source).unwrap();
        let mut entry = root();
        entry.parameters[0].annotation = "java.util.List<java.lang.Integer>".into();
        entry.symbol = "example.Discounts.preview(java.util.List)".into();
        let report = conform(&[target], &program(entry, vec![helper()]));
        assert!(has(&report, "TARGET_SIGNATURE"));
    }

    #[test]
    fn unsupported_native_type_literals_and_inconsistent_binding_are_lint_errors() {
        for native in [
            "String",
            "T",
            "java.util.List<int>",
            "java.util.List<? extends int>",
            "void[]",
            "java.lang.String rubbish",
        ] {
            let source = TARGET.replace(
                "cartId: \"java.lang.String\"",
                &format!("cartId: {native:?}"),
            );
            assert!(
                parse("bad.target.vitrifi", &source).is_err(),
                "accepted {native}"
            );
        }
        let source = TARGET.replace("preview(java.lang.String)", "preview(long)");
        assert!(parse("bad.target.vitrifi", &source)
            .unwrap_err()
            .iter()
            .any(|d| d.code == "TARGET_BINDING"));
        assert_eq!(
            native_type("java.util.List<? extends java.lang.Number>[]", false).unwrap(),
            "java.util.List[]"
        );
    }

    #[test]
    fn extra_public_entry_and_removed_feature_marker_do_not_escape_coverage() {
        let mut derived = program(root(), vec![helper()]);
        derived.inventory.as_mut().unwrap().classes[0]
            .public_methods
            .push("example.Discounts.save()".into());
        let report = conform(&[target()], &derived);
        assert!(has(&report, "TARGET_UNDECLARED_ENTRY"));
        let mut entry = root();
        entry.feature = None;
        let report = check(entry, vec![helper()]);
        assert!(has(&report, "TARGET_FEATURE_MARKER"));
        assert_eq!(report.operations.len(), 1);
    }

    #[test]
    fn another_class_cannot_export_an_extra_entry_under_the_target_feature_name() {
        let mut extra = root();
        extra.id = "src/Other.java::example.Other.extra()".into();
        extra.symbol = "example.Other.extra()".into();
        let report = check(root(), vec![helper(), extra]);
        assert!(has(&report, "TARGET_UNDECLARED_ENTRY"));
    }

    #[test]
    fn changing_friendly_resource_name_or_table_identity_fails() {
        for changed_table in [false, true] {
            let mut derived = program(root(), vec![helper()]);
            let mapping = &mut derived.inventory.as_mut().unwrap().resources[0];
            if changed_table {
                mapping.table = "coupons".into();
            } else {
                mapping.resource = "InnocentAlias".into();
            }
            let report = conform(&[target()], &derived);
            assert!(has(&report, "TARGET_RESOURCE_MAPPING"));
        }
    }

    #[test]
    fn target_resource_names_cannot_disguise_qualified_table_aliases() {
        for (table, alias) in [
            ("CARTS", "public.carts"),
            ("public.carts", "carts"),
            ("public.carts", "PUBLIC.CARTS"),
        ] {
            let mut target = target();
            target.resources[0].table = table.into();
            target.resources.push(TargetResource {
                name: "CartWrites".into(),
                table: alias.into(),
                location: target.location.clone(),
            });
            assert!(lint(&[target])
                .iter()
                .any(|d| d.code == "TARGET_RESOURCE_CONFLICT"));
        }

        let first = target();
        let source = TARGET
            .replace("Checkout.Discounts", "Checkout.Other")
            .replace("example.Discounts", "example.Other")
            .replace("Carts", "CartWrites")
            .replace("CARTS", "public.carts");
        let second = parse("other.target.vitrifi", &source).unwrap();
        assert!(lint(&[first, second])
            .iter()
            .any(|d| d.code == "TARGET_RESOURCE_CONFLICT"));
    }

    #[test]
    fn source_only_aliases_cannot_escape_target_table_identity_checks() {
        for (table, alias) in [("public.carts", "carts"), ("carts", "public.carts")] {
            let mut target = target();
            target.resources[0].table = table.into();
            let mut derived = program(root(), vec![helper()]);
            let resources = &mut derived.inventory.as_mut().unwrap().resources;
            resources[0].table = table.into();
            resources.push(SourceResource {
                owner: "example.storage.Writer".into(),
                path: "src/Writer.java".into(),
                table: alias.into(),
                resource: "CartWrites".into(),
            });
            let report = conform(&[target], &derived);
            assert_eq!(report.status, "failed");
            assert!(has(&report, "TARGET_RESOURCE_MAPPING"));
        }
    }

    #[test]
    fn distinct_fully_qualified_schemas_remain_distinct_resources() {
        let mut target = target();
        target.resources[0].table = "public.carts".into();
        target.resources.push(TargetResource {
            name: "ArchivedCarts".into(),
            table: "archive.carts".into(),
            location: target.location.clone(),
        });
        let mut derived = program(root(), vec![helper()]);
        let resources = &mut derived.inventory.as_mut().unwrap().resources;
        resources[0].table = "public.carts".into();
        resources.push(SourceResource {
            owner: "example.storage.Archive".into(),
            path: "src/Archive.java".into(),
            table: "archive.carts".into(),
            resource: "ArchivedCarts".into(),
        });
        let report = conform(&[target], &derived);
        assert_eq!(report.status, "passed", "{:?}", report.diagnostics);
    }

    #[test]
    fn unknown_reachable_calls_are_incomplete_and_known_violations_still_fail() {
        let mut load = helper();
        load.calls.push(SourceCall {
            expression: "supplier.get".into(),
            line: 1,
            column: 1,
            target: None,
            source_owner: None,
            resolution: CallResolution::Unknown,
            reason: "unsupported dispatch".into(),
        });
        let report = check(root(), vec![load.clone()]);
        assert_eq!(report.status, "incomplete", "{:?}", report.diagnostics);
        assert!(has(&report, "TARGET_ANALYSIS_GAP"));
        assert_eq!(report.policy_results[0].status, PolicyStatus::Unknown);
        load.direct_effects.push(effect("write"));
        assert_eq!(check(root(), vec![load]).status, "failed");
    }

    #[test]
    fn dependency_boundaries_are_transitive_and_ambiguous_mapping_fails() {
        let mut restricted = target();
        restricted.operations[0].dependencies.clear();
        let report = conform(&[restricted], &program(root(), vec![helper()]));
        assert!(has(&report, "TARGET_DEPENDENCY_EXCEEDED"));
        let mut target = target();
        target.boundaries[0]
            .classes
            .push("example.storage.Repository".into());
        let report = conform(&[target], &program(root(), vec![helper()]));
        assert!(has(&report, "TARGET_AMBIGUOUS_BOUNDARY"));
    }

    #[test]
    fn summarized_record_members_require_transitive_boundary_permission() {
        for expression in ["new example.records.Value()", "record.value()"] {
            let mut load = helper();
            load.calls.push(SourceCall {
                expression: expression.into(),
                line: 1,
                column: 1,
                target: None,
                source_owner: Some("example.records.Value".into()),
                resolution: CallResolution::Intrinsic,
                reason: "summarized source record member".into(),
            });
            let derived = program(root(), vec![load]);
            let mut target = target();
            let report = conform(std::slice::from_ref(&target), &derived);
            assert_eq!(report.status, "failed");
            assert!(has(&report, "TARGET_BOUNDARY_COVERAGE"));

            target.boundaries.push(TargetBoundary {
                name: "Records".into(),
                classes: vec!["example.records.*".into()],
                location: target.location.clone(),
            });
            let report = conform(std::slice::from_ref(&target), &derived);
            assert_eq!(report.status, "failed");
            assert!(has(&report, "TARGET_DEPENDENCY_EXCEEDED"));

            target.operations[0].dependencies.push("Records".into());
            let report = conform(&[target], &derived);
            assert_eq!(report.status, "passed", "{:?}", report.diagnostics);
        }
    }

    #[test]
    fn external_intrinsics_do_not_require_source_boundary_mapping() {
        let mut load = helper();
        load.calls.push(SourceCall {
            expression: "java.util.List.of()".into(),
            line: 1,
            column: 1,
            target: None,
            source_owner: None,
            resolution: CallResolution::Intrinsic,
            reason: "external modeled intrinsic".into(),
        });
        assert_eq!(check(root(), vec![load]).status, "passed");
    }

    #[test]
    fn source_policy_diagnostics_follow_selected_operation_ids() {
        let mut unrelated = helper();
        unrelated.id = "src/Repository.java::example.storage.Repository.unrelated()".into();
        unrelated.symbol = "example.storage.Repository.unrelated()".into();
        unrelated.direct_effects = vec![effect("write")];
        unrelated.policies = vec![SourcePolicy::Forbid {
            effect: effect("write"),
        }];
        // Both helpers deliberately have the same source path and line.
        let mut derived = program(root(), vec![helper(), unrelated]);
        assert!(derived
            .diagnostics
            .iter()
            .any(|d| d.code == "FORBIDDEN_EFFECT"));
        let report = conform(&[target()], &derived);
        assert_eq!(report.status, "passed", "{:?}", report.diagnostics);

        derived.diagnostics.push(diagnostic(
            "JAVA_METADATA",
            "Global adapter metadata remains invalid",
            &target().location,
        ));
        assert!(has(&conform(&[target()], &derived), "JAVA_METADATA"));

        let mut selected = helper();
        selected.policies = vec![SourcePolicy::Forbid {
            effect: effect("read"),
        }];
        let report = check(root(), vec![selected]);
        assert_eq!(report.status, "failed");
        assert!(has(&report, "FORBIDDEN_EFFECT"));
    }

    #[test]
    fn old_inventory_and_unmodeled_inherited_api_cannot_pass() {
        let mut derived = program(root(), vec![helper()]);
        derived.inventory = None;
        assert_eq!(conform(&[target()], &derived).status, "incomplete");
        for version in [1, 3] {
            derived.inventory = Some(inventory(&root()));
            derived.inventory.as_mut().unwrap().version = version;
            let report = conform(&[target()], &derived);
            assert_eq!(report.status, "incomplete");
            assert!(has(&report, "TARGET_INVENTORY"));
        }
        derived.inventory = Some(inventory(&root()));
        derived.inventory.as_mut().unwrap().classes[0]
            .unsupported_api
            .push("Inherited method is not modeled".into());
        let report = conform(&[target()], &derived);
        assert_eq!(report.status, "incomplete");
        assert!(has(&report, "TARGET_API_GAP"));
    }

    #[test]
    fn target_error_path_policy_remains_a_runtime_obligation() {
        let mut target = target();
        let policy = SourcePolicy::NoEffectOnError {
            effect: effect("write"),
        };
        target.operations[0].policies = vec![policy.clone()];
        let mut entry = root();
        entry.policies = vec![policy];
        let report = conform(&[target], &program(entry, vec![helper()]));
        assert_eq!(report.status, "passed");
        assert_eq!(
            report.policy_results[0].status,
            PolicyStatus::RuntimeRequired
        );
        assert_eq!(report.operations[0].policies.len(), 1);
    }

    #[test]
    fn target_lint_rejects_cross_file_duplicates_and_resource_conflicts() {
        let first = target();
        let mut second = target();
        second.path = "other.target.vitrifi".into();
        let errors = lint(&[first.clone(), second.clone()]);
        for code in [
            "TARGET_DUPLICATE_FEATURE",
            "TARGET_DUPLICATE_COVERAGE",
            "TARGET_DUPLICATE_BINDING",
        ] {
            assert!(errors.iter().any(|d| d.code == code), "missing {code}");
        }
        second.name = "Other".into();
        second.resources[0].table = "coupons".into();
        assert!(lint(&[first, second])
            .iter()
            .any(|d| d.code == "TARGET_RESOURCE_CONFLICT"));
    }

    #[test]
    fn parser_requires_explicit_bounds_rejects_duplicates_and_handles_comments() {
        let with_comments = format!(
            "/* before */ {} // after",
            TARGET.replace("read Carts", "read /* within effect */ Carts")
        );
        assert!(parse("comments.target.vitrifi", &with_comments).is_ok());
        for source in [
            TARGET.replace("effects [read Carts];", ""),
            TARGET.replace("dependencies [Storage];", ""),
            TARGET.replace("effects [read Carts];", "effects [read Carts]; effects [];"),
            TARGET.replace("dependencies [Storage]", "dependencies [Missing]"),
            TARGET.replace("resource Carts table", "resource Workflow table"),
            format!("{TARGET} trailing"),
            "/* unterminated".into(),
        ] {
            assert!(parse("bad.target.vitrifi", &source).is_err());
        }
    }
}
