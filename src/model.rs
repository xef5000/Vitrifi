use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeExpr {
    pub name: String,
    pub arguments: Vec<TypeExpr>,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub alias: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub language: String,
    pub path: String,
    pub symbol: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Effect {
    pub action: String,
    pub resource: Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub public: bool,
    pub location: Location,
    pub body: DeclBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeclBody {
    Type,
    Resource,
    Requirement {
        text: String,
    },
    Boundary {
        paths: Vec<String>,
        allows: Vec<Reference>,
    },
    Operation {
        parameters: Vec<Parameter>,
        returns: TypeExpr,
        purpose: Option<String>,
        binding: Binding,
        effects: Vec<Effect>,
        requires: Vec<Reference>,
        ensures: Vec<Reference>,
        calls: Vec<Reference>,
    },
    Check {
        command: Vec<String>,
        cwd: String,
        covers: Vec<Reference>,
        timeout: u64,
    },
}

impl DeclBody {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Resource => "resource",
            Self::Requirement { .. } => "requirement",
            Self::Boundary { .. } => "boundary",
            Self::Operation { .. } => "operation",
            Self::Check { .. } => "check",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Module {
    pub path: String,
    pub name: String,
    pub purpose: String,
    pub imports: Vec<Import>,
    pub declarations: Vec<Declaration>,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostSymbol {
    pub name: String,
    pub signature: String,
    pub line: usize,
    pub end_line: usize,
    pub calls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostImport {
    pub module: String,
    pub kind: String,
    pub names: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostFile {
    pub path: String,
    pub language: String,
    pub package: Option<String>,
    pub symbols: Vec<HostSymbol>,
    pub imports: Vec<HostImport>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub provenance: String,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub modules: Vec<Module>,
    pub host_files: Vec<HostFile>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
    pub revision: String,
    pub inputs: std::collections::BTreeMap<String, String>,
    pub limitations: Vec<String>,
}
