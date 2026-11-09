//! Small proof-checking kernel. No proof search, file I/O, Java execution or
//! producer dependency lives here. Every accepted reduction is replayed.
use crate::{
    compiler,
    workflow::{self, Statement, Value, Workflow},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

pub const RULES: &str = "vitrifi.workflow-laws/1";
pub const MAX_STEPS: usize = 512;
pub const MAX_NODES: usize = 20_000;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    BranchContinuation,
    LiteralBranch,
    IdenticalBranches,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Yes,
    No,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    pub statement: usize,
    pub arm: Arm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub operation: String,
    pub block: Vec<Branch>,
    pub statement: usize,
    pub rule: Rule,
    pub before_sha256: String,
    pub after_sha256: String,
}

// Source positions are diagnostics, never execution semantics. Keep every
// other field, including native signatures, policies, messages and leaf names.
fn without_positions(value: &mut Json) {
    match value {
        Json::Array(items) => items.iter_mut().for_each(without_positions),
        Json::Object(fields) => {
            fields.remove("location");
            for item in fields.values_mut() {
                without_positions(item);
            }
        }
        _ => (),
    }
}
pub fn syntax(workflow: &Workflow) -> Json {
    let mut value = serde_json::to_value(workflow).unwrap();
    value.as_object_mut().unwrap().remove("path");
    without_positions(&mut value);
    value
}
pub fn digest(workflow: &Workflow) -> String {
    compiler::hash_bytes(&serde_json::to_vec(&syntax(workflow)).unwrap())
}
pub fn contract(workflow: &Workflow) -> Json {
    let mut value = syntax(workflow);
    for operation in value["operations"].as_array_mut().unwrap() {
        operation.as_object_mut().unwrap().remove("body");
    }
    value
}

pub fn validate(workflow: &Workflow) -> Result<(), String> {
    fn count(body: &[Statement], depth: usize, nodes: &mut usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("PROOF_LIMIT: workflow exceeds 64 blocks".into());
        }
        *nodes += body.len();
        if *nodes > MAX_NODES {
            return Err("PROOF_LIMIT: workflow exceeds 20000 statements".into());
        }
        for statement in body {
            if let Statement::If { yes, no, .. } = statement {
                count(yes, depth + 1, nodes)?;
                count(no, depth + 1, nodes)?;
            }
        }
        Ok(())
    }
    let mut nodes = 0;
    for operation in &workflow.operations {
        count(&operation.body, 0, &mut nodes)?;
    }
    let diagnostics = workflow::lint(workflow);
    if !diagnostics.is_empty() {
        return Err(format!(
            "PROOF_ILL_TYPED: {}",
            serde_json::to_string(&diagnostics).unwrap()
        ));
    }
    Ok(())
}

/// Structural fallthrough, not a claim about whether native leaves terminate.
fn falls_through(body: &[Statement]) -> bool {
    body.iter().all(|statement| match statement {
        Statement::Return { .. } | Statement::Reject { .. } => false,
        Statement::If { yes, no, .. } => falls_through(yes) || falls_through(no),
        _ => true,
    })
}
fn block_at<'a>(
    body: &'a mut Vec<Statement>,
    path: &[Branch],
) -> Result<&'a mut Vec<Statement>, String> {
    if let Some((branch, tail)) = path.split_first() {
        match body.get_mut(branch.statement) {
            Some(Statement::If { yes, no, .. }) => {
                block_at(if branch.arm == Arm::Yes { yes } else { no }, tail)
            }
            _ => Err("PROOF_ADDRESS: block path does not select an if branch".into()),
        }
    } else {
        Ok(body)
    }
}
pub(crate) fn same_block(a: &[Statement], b: &[Statement]) -> bool {
    let mut a = serde_json::to_value(a).unwrap();
    let mut b = serde_json::to_value(b).unwrap();
    without_positions(&mut a);
    without_positions(&mut b);
    a == b
}

/// Apply exactly one checked law. This function does not find or choose a law.
pub fn reduce(
    workflow: &Workflow,
    operation: &str,
    path: &[Branch],
    index: usize,
    rule: Rule,
) -> Result<Workflow, String> {
    if path.len() > MAX_DEPTH {
        return Err("PROOF_LIMIT: address too deep".into());
    }
    let mut result = workflow.clone();
    let operation = result
        .operations
        .iter_mut()
        .find(|o| o.name == operation)
        .ok_or("PROOF_ADDRESS: unknown operation")?;
    let body = block_at(&mut operation.body, path)?;
    let Some(Statement::If {
        condition, yes, no, ..
    }) = body.get(index)
    else {
        return Err("PROOF_RULE: selected statement must be an if".into());
    };
    match rule {
        Rule::LiteralBranch => {
            let Value::Bool(value) = condition else {
                return Err("PROOF_RULE: condition must be a boolean literal".into());
            };
            let replacement = if *value { yes.clone() } else { no.clone() };
            body.splice(index..=index, replacement);
        }
        Rule::IdenticalBranches => {
            if !same_block(yes, no) {
                return Err(
                    "PROOF_RULE: branches must have identical syntax modulo positions".into(),
                );
            }
            let replacement = yes.clone();
            body.splice(index..=index, replacement);
        }
        Rule::BranchContinuation => {
            if index + 1 >= body.len() || (!falls_through(yes) && !falls_through(no)) {
                return Err(
                    "PROOF_RULE: branch continuation needs a nonempty tail and a continuing branch"
                        .into(),
                );
            }
            // Bound expansion before cloning a potentially duplicated tail.
            fn size(body: &[Statement]) -> usize {
                body.iter()
                    .map(|s| {
                        1 + match s {
                            Statement::If { yes, no, .. } => size(yes) + size(no),
                            _ => 0,
                        }
                    })
                    .sum()
            }
            let tail = &body[index + 1..];
            if size(yes) + size(no) + 2 * size(tail) > MAX_NODES {
                return Err(
                    "PROOF_LIMIT: branch continuation expands beyond statement budget".into(),
                );
            }
            let tail = tail.to_vec();
            let Statement::If { yes, no, .. } = &mut body[index] else {
                unreachable!()
            };
            if falls_through(yes) {
                yes.extend(tail.clone());
            }
            if falls_through(no) {
                no.extend(tail);
            }
            body.truncate(index + 1);
        }
    }
    // Includes lexical capture checks, types, all exits and rejection policies.
    // A law is inapplicable if moving code would shadow a binding or exceed bounds.
    validate(&result)?;
    Ok(result)
}

/// Canonical lexical binding identities justify alpha-equivalence of local
/// values. Public signatures and action contracts remain byte-for-byte syntax.
pub fn alpha_form(workflow: &Workflow) -> Json {
    fn atom(value: &Value, env: &BTreeMap<String, usize>) -> Json {
        match value {
            Value::Ref(name) => json!({"variable":env[name]}),
            Value::Text(value) => json!({"text":value}),
            Value::Bool(value) => json!({"boolean":value}),
        }
    }
    fn body(statements: &[Statement], mut env: BTreeMap<String, usize>, next: &mut usize) -> Json {
        Json::Array(statements.iter().map(|statement| match statement {
            Statement::Let {name,call,..} => {
                let arguments: Vec<_> = call.arguments.iter().map(|v|atom(v,&env)).collect();
                let id = *next; *next += 1; env.insert(name.clone(),id);
                json!({"let":id,"action":call.action,"arguments":arguments})
            }
            Statement::Do {call,..} => json!({"do":call.action,"arguments":call.arguments.iter().map(|v|atom(v,&env)).collect::<Vec<_>>()}),
            Statement::If {condition,yes,no,..} => json!({"if":atom(condition,&env),"yes":body(yes,env.clone(),next),"no":body(no,env.clone(),next)}),
            Statement::Return {value,..} => json!({"return":atom(value,&env)}),
            Statement::Reject {message,..} => json!({"reject":message}),
        }).collect())
    }
    let operations: Vec<_> = workflow
        .operations
        .iter()
        .map(|op| {
            let env = op
                .parameters
                .iter()
                .enumerate()
                .map(|(i, p)| (p.name.clone(), i))
                .collect();
            let mut next = op.parameters.len();
            json!({"operation":op.name,"body":body(&op.body,env,&mut next)})
        })
        .collect();
    json!({"contract":contract(workflow),"operations":operations})
}

pub fn replay(endpoint: &Workflow, steps: &[Step]) -> Result<Workflow, String> {
    if steps.len() > MAX_STEPS {
        return Err("PROOF_LIMIT: too many proof steps".into());
    }
    validate(endpoint)?;
    let mut state = endpoint.clone();
    for (index, step) in steps.iter().enumerate() {
        if step.before_sha256 != digest(&state) {
            return Err(format!("PROOF_CHAIN: incorrect input hash at step {index}"));
        }
        state = reduce(
            &state,
            &step.operation,
            &step.block,
            step.statement,
            step.rule,
        )?;
        if step.after_sha256 != digest(&state) {
            return Err(format!(
                "PROOF_CHAIN: incorrect output hash at step {index}"
            ));
        }
    }
    Ok(state)
}

pub fn check(
    before: &Workflow,
    after: &Workflow,
    left: &[Step],
    right: &[Step],
    common_hash: &str,
) -> Result<(), String> {
    if contract(before) != contract(after) {
        return Err(
            "PROOF_CONTRACT: feature, API, leaves, resources and policies must remain unchanged"
                .into(),
        );
    }
    let a = replay(before, left)?;
    let b = replay(after, right)?;
    let a = alpha_form(&a);
    let b = alpha_form(&b);
    if a != b {
        return Err(
            "PROOF_NOT_EQUIVALENT: derivations do not meet modulo local alpha-renaming".into(),
        );
    }
    if compiler::hash_bytes(&serde_json::to_vec(&a).unwrap()) != common_hash {
        return Err("PROOF_COMMON_HASH: incorrect common-term hash".into());
    }
    Ok(())
}
