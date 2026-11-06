//! Rust Java-workflow acceptance tests with a deterministic adapter executable.
//! The fixture emits source facts; real javac attribution is covered by Java tests.
#![cfg(unix)]
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use vitrifi::{
    compiler,
    derived::{self, PolicyStatus},
    derived_model::*,
    evidence, java,
};

const SOURCE: &str = "src/main/java/App.java";
const ENTRY: &str = "src/main/java/App.java::example.App#load()";
const HELPER: &str = "src/main/java/App.java::example.App#save()";

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let fixture = Self {
            _directory: directory,
            root,
        };
        fixture.write(
            SOURCE,
            b"package example; class App { int load() { return 1; } }\n",
        );
        fixture.write(".tools/analyzer.jar", b"adapter fixture");
        fixture.write("target/a.jar", b"dependency A");
        fixture.write("target/b.jar", b"dependency B");
        fixture.classpath(&["target/a.jar", "target/b.jar"]);
        fixture.model(vec![entry()]);
        fixture
    }
    fn path(&self) -> &Path {
        &self.root
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let target = self.root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
    fn classpath(&self, entries: &[&str]) {
        let cp = std::env::join_paths(entries.iter().map(|p| self.root.join(p))).unwrap();
        self.write("target/classpath.txt", cp.as_encoded_bytes());
    }
    fn model(&self, operations: Vec<SourceOperation>) {
        let data = serde_json::to_string(&SourceAnalysis {
            operations,
            diagnostics: vec![],
            inventory: None,
        })
        .unwrap();
        // Single-quote shell escaping keeps all model content literal, including
        // apostrophes, $, backticks, backslashes and embedded JSON escapes.
        let literal = format!("'{}'", data.replace('\'', "'\\''"));
        let script=format!("#!/bin/sh\nif [ \"$1\" = \"-version\" ] || [ \"$1\" = \"--version\" ]; then\n  printf '%s\\n' 'openjdk version fixture-21' >&2\n  exit 0\nfi\nprintf '%s\\n' {literal}\n");
        self.write(".tools/java", script.as_bytes());
        fs::set_permissions(
            self.root.join(".tools/java"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fn options(&self) -> java::Options {
        java::Options {
            analyzer: ".tools/analyzer.jar".into(),
            classpath_file: "target/classpath.txt".into(),
            source_root: "src/main/java".into(),
            java: self.root.join(".tools/java"),
        }
    }
    fn analyze(&self) -> java::Analysis {
        java::analyze(self.path(), &self.options()).unwrap()
    }
    fn output(&self, analysis: &java::Analysis) -> PathBuf {
        self.root
            .join(java::output_paths(analysis).unwrap()["Example.Read"].clone())
    }
    fn record_evidence(&self, analysis: &java::Analysis) {
        evidence::save(self.path(),"java-evidence.json",&json!({"status":"passed","inputs":analysis.inputs,"revision":analysis.revision,"workspace_revision":compiler::workspace_digest(self.path()).unwrap()})).unwrap();
        assert_eq!(java::evidence_status(self.path()).unwrap()["fresh"], true);
    }
}
fn effect() -> ResourceEffect {
    ResourceEffect {
        action: "write".into(),
        resource: "Carts".into(),
    }
}
fn entry() -> SourceOperation {
    SourceOperation {
        id: ENTRY.into(),
        path: SOURCE.into(),
        symbol: "example.App#load()".into(),
        language: "java".into(),
        line: 1,
        signature: "public int example.App#load()".into(),
        parameters: vec![],
        returns: "int".into(),
        feature: Some(FeatureMarker {
            name: "Example.Read".into(),
            purpose: "Read the customer's cart; never execute $(commands) or `commands`".into(),
        }),
        calls: vec![],
        direct_effects: vec![],
        policies: vec![SourcePolicy::Forbid { effect: effect() }],
    }
}
fn helper() -> SourceOperation {
    let mut op = entry();
    op.id = HELPER.into();
    op.symbol = "example.App#save()".into();
    op.signature = "void example.App#save()".into();
    op.returns = "void".into();
    op.feature = None;
    op.policies.clear();
    op
}
fn resolved(target: &str) -> SourceCall {
    SourceCall {
        expression: "save()".into(),
        line: 1,
        column: 42,
        target: Some(target.into()),
        resolution: CallResolution::Resolved,
        reason: "source-resolved method".into(),
    }
}

#[test]
fn generation_preserves_authored_text_and_non_utf8_files() {
    let fixture = Fixture::new();
    let analysis = fixture.analyze();
    let output = fixture.output(&analysis);
    let authored = b"feature HumanAuthored { purpose \"keep this\"; }\n";
    fs::write(&output, authored).unwrap();
    assert!(java::generate(fixture.path(), &analysis, false).is_err());
    assert_eq!(fs::read(&output).unwrap(), authored);
    let invalid = [0xff, 0xfe, 0x00, 0x42];
    fs::write(&output, invalid).unwrap();
    assert!(java::generate(fixture.path(), &analysis, false).is_err());
    assert_eq!(fs::read(&output).unwrap(), invalid);
}

#[test]
fn source_signature_change_marks_generated_view_stale_and_updates_native_facts() {
    let fixture = Fixture::new();
    let before = fixture.analyze();
    assert_eq!(
        java::generate(fixture.path(), &before, false).unwrap()["status"],
        "passed"
    );
    let first = fs::read_to_string(fixture.output(&before)).unwrap();
    // Both int and long map to architectural Int: the native signature must
    // still survive generation and semantic review.
    fixture.write(
        SOURCE,
        b"package example; class App { long load() { return 1L; } }\n",
    );
    let mut changed = entry();
    changed.signature = "public long example.App#load()".into();
    changed.returns = "long".into();
    fixture.model(vec![changed]);
    let after = fixture.analyze();
    let stale = java::generate(fixture.path(), &after, true).unwrap();
    assert_eq!(stale["status"], "stale");
    assert_eq!(stale["stale"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(fixture.output(&after)).unwrap(), first);
    java::generate(fixture.path(), &after, false).unwrap();
    let second = fs::read_to_string(fixture.output(&after)).unwrap();
    assert_ne!(first, second);
    assert!(second.contains("public long example.App#load()"));
    assert_ne!(
        derived::facts(&before.program),
        derived::facts(&after.program)
    );
    assert_eq!(
        java::generate(fixture.path(), &after, true).unwrap()["status"],
        "passed"
    );
}

#[test]
fn regeneration_never_accepts_a_new_write_by_widening_the_source_policy() {
    let fixture = Fixture::new();
    let before = fixture.analyze();
    java::generate(fixture.path(), &before, false).unwrap();
    let mut root = entry();
    root.calls.push(resolved(HELPER));
    let mut save = helper();
    save.direct_effects.push(effect());
    fixture.write(
        SOURCE,
        b"package example; class App { int load() { save(); return 1; } void save() {} }\n",
    );
    fixture.model(vec![root, save]);
    let after = fixture.analyze();
    assert_eq!(java::static_status(&after), "failed");
    assert_eq!(
        after.program.policy_results[0].status,
        PolicyStatus::Violated
    );
    assert_eq!(
        derived::facts(&before.program)[&format!("policy {ENTRY}")],
        derived::facts(&after.program)[&format!("policy {ENTRY}")]
    );
    assert_eq!(
        java::generate(fixture.path(), &after, false).unwrap()["status"],
        "failed"
    );
    let view = fs::read_to_string(fixture.output(&after)).unwrap();
    assert!(view.contains("violated"));
    assert!(view.contains("effects [write Carts];"));
    assert_eq!(
        java::generate(fixture.path(), &after, true).unwrap()["status"],
        "failed"
    );
}

#[test]
fn unknown_effects_reachable_through_a_source_helper_prevent_a_clean_result() {
    let fixture = Fixture::new();
    let mut root = entry();
    root.calls.push(resolved(HELPER));
    let mut helper = helper();
    helper.calls.push(SourceCall {
        expression: "plugin.run()".into(),
        line: 2,
        column: 5,
        target: None,
        resolution: CallResolution::Unknown,
        reason: "ambiguous dispatch".into(),
    });
    fixture.model(vec![root, helper]);
    let analysis = fixture.analyze();
    assert_eq!(java::static_status(&analysis), "incomplete");
    assert_eq!(
        analysis.program.policy_results[0].status,
        PolicyStatus::Unknown
    );
    let report = java::generate(fixture.path(), &analysis, false).unwrap();
    assert_eq!(report["status"], "incomplete");
    assert!(fs::read_to_string(fixture.output(&analysis))
        .unwrap()
        .contains("Analysis gap"));
}

#[test]
fn generated_cli_routes_evidence_and_only_compiles_successful_analysis() {
    let fixture = Fixture::new();
    let analysis = fixture.analyze();
    java::generate(fixture.path(), &analysis, false).unwrap();
    let run = |arguments: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_vitrifi"))
            .args(arguments)
            .arg("--root")
            .arg(fixture.path())
            .args([
                "--java-analyzer",
                ".tools/analyzer.jar",
                "--classpath-file",
                "target/classpath.txt",
            ])
            .env("VITRIFI_JAVA", fixture.path().join(".tools/java"))
            .output()
            .unwrap()
    };
    let compiled = run(&["compile"]);
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert!(fixture.path().join(".vitrifi/program.json").is_file());
    fixture.record_evidence(&analysis);
    assert!(run(&["evidence"]).status.success());

    let target = fixture.path().join("compiled.json");
    fs::write(&target, "existing model").unwrap();
    let mut invalid = entry();
    invalid.direct_effects.push(effect());
    fixture.model(vec![invalid]);
    let rejected = run(&["compile", "--out", "compiled.json"]);
    assert!(!rejected.status.success());
    assert_eq!(fs::read_to_string(target).unwrap(), "existing model");
    let report: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(report["status"], "failed");
}

#[test]
fn classpath_order_changes_revision_and_stales_evidence_even_under_ignored_target() {
    let fixture = Fixture::new();
    let before = fixture.analyze();
    fixture.record_evidence(&before);
    let workspace = compiler::workspace_digest(fixture.path()).unwrap();
    fixture.classpath(&["target/b.jar", "target/a.jar"]);
    assert_eq!(
        compiler::workspace_digest(fixture.path()).unwrap(),
        workspace
    );
    let report = java::evidence_status(fixture.path()).unwrap();
    assert_eq!(report["status"], "stale");
    assert!(!report["fresh"].as_bool().unwrap());
    let after = fixture.analyze();
    assert_ne!(before.revision, after.revision);
    assert_eq!(
        derived::facts(&before.program),
        derived::facts(&after.program)
    );
}

#[test]
fn dependency_content_change_stales_evidence_without_a_source_edit() {
    let fixture = Fixture::new();
    let before = fixture.analyze();
    fixture.record_evidence(&before);
    let workspace = compiler::workspace_digest(fixture.path()).unwrap();
    fixture.write("target/a.jar", b"updated dependency A");
    assert_eq!(
        compiler::workspace_digest(fixture.path()).unwrap(),
        workspace
    );
    assert_eq!(
        java::evidence_status(fixture.path()).unwrap()["status"],
        "stale"
    );
    assert_ne!(before.revision, fixture.analyze().revision);
}

#[test]
fn new_classpath_directory_member_stales_evidence() {
    let fixture = Fixture::new();
    fixture.write("target/classes/A.class", b"class A");
    fixture.classpath(&["target/classes"]);
    let before = fixture.analyze();
    fixture.record_evidence(&before);
    let workspace = compiler::workspace_digest(fixture.path()).unwrap();
    fixture.write("target/classes/B.class", b"new class B");
    assert_eq!(
        compiler::workspace_digest(fixture.path()).unwrap(),
        workspace
    );
    assert_eq!(
        java::evidence_status(fixture.path()).unwrap()["status"],
        "stale"
    );
    assert_ne!(before.revision, fixture.analyze().revision);
}

#[test]
fn deleted_feature_reports_its_obsolete_generated_file_without_deleting_it() {
    let fixture = Fixture::new();
    let before = fixture.analyze();
    java::generate(fixture.path(), &before, false).unwrap();
    let output = fixture.output(&before);
    let original = fs::read(&output).unwrap();
    let mut unowned = entry();
    unowned.feature = None;
    fixture.model(vec![unowned]);
    let after = fixture.analyze();
    let result = java::generate(fixture.path(), &after, true).unwrap();
    assert_eq!(result["status"], "stale");
    assert_eq!(result["obsolete"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(output).unwrap(), original);
}
