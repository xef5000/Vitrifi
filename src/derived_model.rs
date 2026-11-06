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
    /// Declaring source class for summarized members with no callable body,
    /// such as generated Java record constructors and component accessors.
    #[serde(default)]
    pub source_owner: Option<String>,
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
    NoEffectOnRejection { effect: ResourceEffect },
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

/// Adapter inventory independent of feature markers and reachable call graphs.
/// Conformance requires a supported inventory version rather than assuming that
/// operations omitted by an older adapter do not exist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInventory {
    pub version: u32,
    pub classes: Vec<SourceClass>,
    pub resources: Vec<SourceResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceClass {
    pub name: String,
    pub path: String,
    /// Declared public method symbols, including static methods, excluding constructors.
    pub public_methods: Vec<String>,
    /// Public inherited or generated API this adapter cannot completely bind.
    pub unsupported_api: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceResource {
    pub owner: String,
    pub path: String,
    /// SQL classifier's normalized table identity, independent of the friendly name.
    pub table: String,
    pub resource: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceAnalysis {
    pub operations: Vec<SourceOperation>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub inventory: Option<SourceInventory>,
}

pub type SourceFiles = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_adapter_results_remain_readable_without_claiming_inventory_coverage() {
        let analysis: SourceAnalysis =
            serde_json::from_str(r#"{"operations":[],"diagnostics":[]}"#).unwrap();
        assert!(analysis.inventory.is_none());
        assert!(crate::derived::build(analysis).inventory.is_none());
    }

    #[test]
    fn calls_from_old_adapters_remain_readable_without_source_owner() {
        let call: SourceCall = serde_json::from_str(
            r#"{"expression":"record.value()","line":1,"column":1,"target":null,"resolution":"intrinsic","reason":"record accessor"}"#,
        )
        .unwrap();
        assert_eq!(call.source_owner, None);
    }

    #[test]
    fn derived_program_retains_independent_public_api_and_resource_inventory() {
        let inventory = SourceInventory {
            version: 2,
            classes: vec![SourceClass {
                name: "demo.App".into(),
                path: "src/App.java".into(),
                public_methods: vec!["demo.App.unmarked()".into()],
                unsupported_api: vec![],
            }],
            resources: vec![SourceResource {
                owner: "demo.Storage".into(),
                path: "src/Storage.java".into(),
                table: "shop.carts".into(),
                resource: "Carts".into(),
            }],
        };
        let program = crate::derived::build(SourceAnalysis {
            inventory: Some(inventory.clone()),
            ..SourceAnalysis::default()
        });
        assert_eq!(program.inventory, Some(inventory));
    }
}
