//! Source facts shared by language adapters and the generated feature workflow.
//! A resolved call and an explicitly labeled effect boundary are different evidence.
use crate::model::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceEffect {
    pub action: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallResolution {
    Resolved,
    Intrinsic,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceCall {
    pub expression: String,
    pub line: usize,
    pub column: usize,
    pub target: Option<String>,
    pub resolution: CallResolution,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceParameter {
    pub name: String,
    pub annotation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureMarker {
    pub name: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourcePolicy {
    Forbid { effect: ResourceEffect },
    NoEffectOnError { effect: ResourceEffect },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceOperation {
    /// Stable within a source revision: repository-relative path + `::` + symbol.
    pub id: String,
    pub path: String,
    pub symbol: String,
    pub language: String,
    pub line: usize,
    pub signature: String,
    pub parameters: Vec<SourceParameter>,
    pub returns: String,
    pub feature: Option<FeatureMarker>,
    pub calls: Vec<SourceCall>,
    /// Trusted adapter labels, never presented as inferred host body effects.
    pub direct_effects: Vec<ResourceEffect>,
    pub policies: Vec<SourcePolicy>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceAnalysis {
    pub operations: Vec<SourceOperation>,
    pub diagnostics: Vec<Diagnostic>,
}

pub type SourceFiles = BTreeMap<String, String>;
