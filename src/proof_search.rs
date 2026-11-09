//! Untrusted proof producer. Failure to find a derivation says nothing about
//! equivalence. The kernel independently replays every proposed step.
use crate::{
    compiler,
    proof_kernel::{self as kernel, Arm, Branch, Rule, Step},
    workflow::{Statement, Value, Workflow},
};

fn candidates(body: &[Statement], path: &[Branch], result: &mut Vec<(Vec<Branch>, usize, Rule)>) {
    for (index, statement) in body.iter().enumerate() {
        if let Statement::If {
            condition, yes, no, ..
        } = statement
        {
            if matches!(condition, Value::Bool(_)) {
                result.push((path.to_vec(), index, Rule::LiteralBranch));
            }
            if kernel::same_block(yes, no) {
                result.push((path.to_vec(), index, Rule::IdenticalBranches));
            }
            if index + 1 < body.len() {
                result.push((path.to_vec(), index, Rule::BranchContinuation));
            }
            for (arm, child) in [(Arm::Yes, yes), (Arm::No, no)] {
                let mut path = path.to_vec();
                path.push(Branch {
                    statement: index,
                    arm,
                });
                candidates(child, &path, result);
            }
        }
    }
}
fn normalize(endpoint: &Workflow) -> Result<(Workflow, Vec<Step>), String> {
    kernel::validate(endpoint)?;
    let mut state = endpoint.clone();
    let mut steps = Vec::new();
    loop {
        let mut next = None;
        'operations: for op in &state.operations {
            let mut addresses = Vec::new();
            candidates(&op.body, &[], &mut addresses);
            for (block, statement, rule) in addresses {
                if let Ok(reduced) = kernel::reduce(&state, &op.name, &block, statement, rule) {
                    next = Some((
                        Step {
                            operation: op.name.clone(),
                            block,
                            statement,
                            rule,
                            before_sha256: kernel::digest(&state),
                            after_sha256: kernel::digest(&reduced),
                        },
                        reduced,
                    ));
                    break 'operations;
                }
            }
        }
        let Some((step, reduced)) = next else {
            return Ok((state, steps));
        };
        if steps.len() == kernel::MAX_STEPS {
            return Err("PROOF_LIMIT: normalization exceeds step budget".into());
        }
        steps.push(step);
        state = reduced;
    }
}
pub fn derive(
    before: &Workflow,
    after: &Workflow,
) -> Result<(Vec<Step>, Vec<Step>, String), String> {
    if kernel::contract(before) != kernel::contract(after) {
        return Err("PROOF_CONTRACT: public and native contracts changed".into());
    }
    let (a, left) = normalize(before)?;
    let (b, right) = normalize(after)?;
    if kernel::alpha_form(&a) != kernel::alpha_form(&b) {
        return Err("PROOF_NOT_FOUND: no common term under the supported laws; this is not evidence of inequivalence".into());
    }
    let common = compiler::hash_bytes(&serde_json::to_vec(&kernel::alpha_form(&a)).unwrap());
    kernel::check(before, after, &left, &right, &common)?;
    Ok((left, right, common))
}
