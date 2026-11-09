use std::collections::BTreeMap;
use vitrifi::{
    proof_kernel::{self as kernel, Arm, Branch, Rule, Step},
    proof_search,
    workflow::{self, Statement, Value, Workflow},
};

fn source(body: &str) -> String {
    format!(
        r#"workflow feature Test.Workflow {{
      purpose "Exercise algebra over arbitrary effectful native leaves";
      emit java "demo.Service"; actions java "demo.Actions";
      resource R table "records";
      action flag() -> "boolean" {{ bind java "demo.Actions.flag()"; effects []; }}
      action text(s: "java.lang.String") -> "java.lang.String" {{ bind java "demo.Actions.text(java.lang.String)"; effects []; }}
      action touch() -> "void" {{ bind java "demo.Actions.touch()"; effects [write R]; }}
      operation run(x: "boolean", y: "boolean") -> "java.lang.String" {{
        effects [write R]; {body}
      }}
    }}"#
    )
}
fn workflow(body: &str) -> Workflow {
    let workflow = workflow::parse("Test.workflow.vitrifi", &source(body)).unwrap();
    kernel::validate(&workflow).unwrap();
    workflow
}
fn reduce(before: &Workflow, path: &[Branch], index: usize, rule: Rule) -> Workflow {
    kernel::reduce(before, "run", path, index, rule).unwrap()
}
fn proof(before: &Workflow, after: &Workflow) {
    let (a, b, hash) = proof_search::derive(before, after).unwrap();
    kernel::check(before, after, &a, &b, &hash).unwrap();
}

// Independent execution oracle. Leaves can depend on call count, mutate an
// opaque world, return booleans, fail or diverge at any invocation. Comparing
// traces includes action arguments and multiplicity, not just an effect set.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Datum {
    Bool(bool),
    Text(String),
    Void,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Exit {
    Return(Datum),
    Reject(String),
    Failed(String),
    Diverged,
}
#[derive(Debug, PartialEq, Eq)]
struct Execution {
    trace: Vec<(String, Vec<Datum>)>,
    exit: Option<Exit>,
}
fn execute(workflow: &Workflow, bits: usize, stop: usize, diverge: bool) -> Execution {
    fn value(v: &Value, env: &BTreeMap<String, Datum>) -> Datum {
        match v {
            Value::Ref(n) => env[n].clone(),
            Value::Text(s) => Datum::Text(s.clone()),
            Value::Bool(b) => Datum::Bool(*b),
        }
    }
    fn run(
        body: &[Statement],
        mut env: BTreeMap<String, Datum>,
        execution: &mut Execution,
        bits: usize,
        stop: usize,
        diverge: bool,
    ) {
        for statement in body {
            if execution.exit.is_some() {
                return;
            }
            match statement {
                Statement::Let { call, .. } | Statement::Do { call, .. } => {
                    let arguments: Vec<_> = call.arguments.iter().map(|v| value(v, &env)).collect();
                    execution
                        .trace
                        .push((call.action.clone(), arguments.clone()));
                    let index = execution.trace.len();
                    if index == stop {
                        execution.exit = Some(if diverge {
                            Exit::Diverged
                        } else {
                            Exit::Failed(call.action.clone())
                        });
                        return;
                    }
                    let result = match call.action.as_str() {
                        "flag" => Datum::Bool(bits & (1 << (index + 1)) != 0),
                        "text" => Datum::Text(format!("{index}:{arguments:?}")),
                        "touch" => Datum::Void,
                        _ => panic!("unknown test action"),
                    };
                    if let Statement::Let { name, .. } = statement {
                        env.insert(name.clone(), result);
                    }
                }
                Statement::If {
                    condition, yes, no, ..
                } => {
                    let Datum::Bool(condition) = value(condition, &env) else {
                        panic!("ill typed")
                    };
                    run(
                        if condition { yes } else { no },
                        env.clone(),
                        execution,
                        bits,
                        stop,
                        diverge,
                    );
                }
                Statement::Return { value: v, .. } => {
                    execution.exit = Some(Exit::Return(value(v, &env)))
                }
                Statement::Reject { message, .. } => {
                    execution.exit = Some(Exit::Reject(message.clone()))
                }
            }
        }
    }
    let mut result = Execution {
        trace: vec![],
        exit: None,
    };
    run(
        &workflow.operations[0].body,
        BTreeMap::from([
            ("x".into(), Datum::Bool(bits & 1 != 0)),
            ("y".into(), Datum::Bool(bits & 2 != 0)),
        ]),
        &mut result,
        bits,
        stop,
        diverge,
    );
    assert!(result.exit.is_some());
    result
}
fn executions_agree(a: &Workflow, b: &Workflow) {
    for bits in 0..64 {
        for stop in 0..7 {
            for diverge in [false, true] {
                assert_eq!(
                    execute(a, bits, stop, diverge),
                    execute(b, bits, stop, diverge),
                    "bits={bits}, stop={stop}, diverge={diverge}"
                );
            }
        }
    }
}

#[test]
fn distribution_preserves_terminal_exits_and_effect_order_for_arbitrary_leaves() {
    let before = workflow(
        r#"let decision = call flag();
        if x { do call touch(); } else { if decision {reject "no";} else {} }
        let receipt = call text("accepted"); return receipt;"#,
    );
    let after = reduce(&before, &[], 1, Rule::BranchContinuation);
    executions_agree(&before, &after);
    proof(&before, &after);
    assert!(matches!(
        after.operations[0].body.last(),
        Some(Statement::If { .. })
    ));
}

#[test]
fn literal_and_identical_branch_laws_preserve_effectful_calls() {
    for (source, rule) in [
        (
            r#"if true {do call touch();} else {reject "unused";} let r = call text("ok"); return r;"#,
            Rule::LiteralBranch,
        ),
        (
            r#"if x {do call touch();} else {do call touch();} let r = call text("ok"); return r;"#,
            Rule::IdenticalBranches,
        ),
        (
            r#"if x {} else {} let r = call text("ok"); return r;"#,
            Rule::IdenticalBranches,
        ),
        (
            r#"if false {reject "unused";} else {} let r = call text("ok"); return r;"#,
            Rule::LiteralBranch,
        ),
    ] {
        let before = workflow(source);
        let after = reduce(&before, &[], 0, rule);
        executions_agree(&before, &after);
        proof(&before, &after);
    }
}

#[test]
fn alpha_equivalence_changes_local_names_without_changing_public_contracts() {
    let a =
        workflow(r#"let first = call text("one"); let second = call text("two"); return first;"#);
    let b = workflow(
        r#"let renamed = call text("one"); let other = call text("two"); return renamed;"#,
    );
    proof(&a, &b);
    executions_agree(&a, &b);
    let wrong =
        workflow(r#"let renamed = call text("one"); let other = call text("two"); return other;"#);
    assert!(proof_search::derive(&a, &wrong).is_err());
}

#[test]
fn proof_does_not_erase_duplicate_reorder_or_change_empty_effect_calls() {
    let before =
        workflow(r#"let first = call text("one"); let second = call text("two"); return first;"#);
    for body in [
        r#"let first = call text("one"); return first;"#,
        r#"let second = call text("two"); let first = call text("one"); return first;"#,
        r#"let first = call text("changed"); let second = call text("two"); return first;"#,
        r#"let first = call text("one"); let second = call text("two"); let extra = call text("two"); return first;"#,
        r#"let first = call text("one"); let second = call text("two"); reject "failed";"#,
    ] {
        assert!(proof_search::derive(&before, &workflow(body)).is_err());
    }
}

#[test]
fn conditions_cannot_be_replaced_based_on_test_outcomes() {
    let before = workflow(r#"if x {reject "no";} else {} let r=call text("ok"); return r;"#);
    let wrong = workflow(r#"if y {reject "no";} else {} let r=call text("ok"); return r;"#);
    assert!(proof_search::derive(&before, &wrong).is_err());
    assert!(kernel::reduce(&before, "run", &[], 0, Rule::LiteralBranch).is_err());
    assert!(kernel::reduce(&before, "run", &[], 0, Rule::IdenticalBranches).is_err());
}

#[test]
fn scope_capture_is_rejected_in_both_directions() {
    let before =
        workflow(r#"if x {let r=call text("local");} else {} let r=call text("outer"); return r;"#);
    assert!(
        kernel::reduce(&before, "run", &[], 0, Rule::BranchContinuation)
            .unwrap_err()
            .contains("PROOF_ILL_TYPED")
    );
    // After reduction a branch-local value must never become a different binder.
    let before = workflow(
        r#"let a=call text("a"); if x {let b=call text("b"); return b;} else {return a;}"#,
    );
    let after = workflow(
        r#"let b=call text("a"); if x {let a=call text("b"); return a;} else {return b;}"#,
    );
    proof(&before, &after);
    executions_agree(&before, &after);
}

#[test]
fn kernel_checks_every_step_and_the_common_term_instead_of_trusting_producer() {
    let before = workflow(r#"if x {} else {reject "no";} let r=call text("ok"); return r;"#);
    let after = reduce(&before, &[], 0, Rule::BranchContinuation);
    let step = Step {
        operation: "run".into(),
        block: vec![],
        statement: 0,
        rule: Rule::BranchContinuation,
        before_sha256: kernel::digest(&before),
        after_sha256: kernel::digest(&after),
    };
    let common =
        vitrifi::compiler::hash_bytes(&serde_json::to_vec(&kernel::alpha_form(&after)).unwrap());
    kernel::check(&before, &after, &[step.clone()], &[], &common).unwrap();
    for mutation in 0..7 {
        let mut tampered = step.clone();
        match mutation {
            0 => tampered.before_sha256 = "bad".into(),
            1 => tampered.after_sha256 = "bad".into(),
            2 => tampered.operation = "missing".into(),
            3 => tampered.statement = usize::MAX,
            4 => tampered.rule = Rule::IdenticalBranches,
            5 => {
                tampered.block = vec![Branch {
                    statement: 9,
                    arm: Arm::Yes,
                }]
            }
            _ => {
                tampered.block = vec![
                    Branch {
                        statement: 0,
                        arm: Arm::No
                    };
                    65
                ]
            }
        }
        assert!(kernel::check(&before, &after, &[tampered], &[], &common).is_err());
    }
    assert!(kernel::check(&before, &after, &[step.clone(), step.clone()], &[], &common).is_err());
    assert!(kernel::check(&before, &after, &[step.clone()], &[], "bad").is_err());
    assert!(kernel::replay(&before, &vec![step; kernel::MAX_STEPS + 1]).is_err());
}

#[test]
fn contracts_and_rejection_policies_remain_review_obligations() {
    let before = workflow(r#"let r=call text("ok"); return r;"#);
    for mutation in 0..5 {
        let mut after = before.clone();
        match mutation {
            0 => after.purpose = "changed intent".into(),
            1 => after.leaves[0].binding = "demo.Actions.other()".into(),
            2 => after.operations[0].parameters[0].name = "newInput".into(),
            3 => after.operations[0].no_effect_on_rejection = after.operations[0].effects.clone(),
            _ => after.resources[0].table = "other".into(),
        }
        assert!(proof_search::derive(&before, &after).is_err());
    }
}

#[test]
fn both_endpoints_and_every_intermediate_must_typecheck() {
    let before = workflow(r#"let r=call text("ok"); return r;"#);
    let mut after = before.clone();
    let extra = after.operations[0].body[0].clone();
    after.operations[0].body.push(extra);
    assert!(kernel::replay(&after, &[]).is_err());
    let mut too_large = before.clone();
    too_large.operations[0].body =
        vec![before.operations[0].body[0].clone(); kernel::MAX_NODES + 1];
    assert!(kernel::validate(&too_large)
        .unwrap_err()
        .contains("PROOF_LIMIT"));
}

#[test]
fn proof_format_rejects_unknown_rules_and_extra_fields() {
    let base = r#"{"operation":"run","block":[],"statement":0,"rule":"literal_branch","before_sha256":"a","after_sha256":"b"}"#;
    assert!(serde_json::from_str::<Step>(base).is_ok());
    assert!(
        serde_json::from_str::<Step>(&base.replace("literal_branch", "commute_calls")).is_err()
    );
    assert!(serde_json::from_str::<Step>(
        &base.replace("\"statement\":0", "\"statement\":0,\"trusted\":true")
    )
    .is_err());
}
