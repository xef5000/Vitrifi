//! CLI routing and evidence identity checks. Real javac/Spring mutations run separately.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
use tempfile::TempDir;
use vitrifi::{compiler, evidence, targets};

const TARGET: &str = "Feature.target.vitrifi";
const CONTRACT: &str = r#"target feature Example {
  purpose "Preview without writes";
  cover java "example.App";
  resource Carts table "carts";
  boundary App java ["example.App"];
  operation preview() -> "int" {
    bind java "example.App.preview()";
    effects [read Carts];
    dependencies [];
    forbid [write Carts];
  }
}"#;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let result = Self { _temp: temp, root };
        result.write(TARGET, CONTRACT);
        result
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn adapter(&self, inventory: bool, policy: bool) {
        self.write(
            "src/main/java/App.java",
            "package example; class App { public int preview() { return 1; } }",
        );
        self.write(".tools/analyzer.jar", "test analyzer");
        self.write("target/classpath.txt", "");
        let rule = json!({"kind":"forbid","effect":{"action":"write","resource":"Carts"}});
        let model = json!({"diagnostics":[],"operations":[{
            "id":"src/main/java/App.java::example.App.preview()", "path":"src/main/java/App.java",
            "symbol":"example.App.preview()","language":"java","line":1,"signature":"int example.App.preview()",
            "parameters":[],"returns":"int","feature":{"name":"Example","purpose":"Preview"},
            "calls":[],"direct_effects":[{"action":"read","resource":"Carts"}],
            "policies":if policy {vec![rule]} else {vec![]}
        }],"inventory":if inventory {json!({"version":1,"classes":[{
            "name":"example.App","path":"src/main/java/App.java","public_methods":["example.App.preview()"],"unsupported_api":[]
        }],"resources":[{"owner":"example.App","path":"src/main/java/App.java","table":"carts","resource":"Carts"}]})} else {Value::Null}});
        let literal = serde_json::to_string(&model)
            .unwrap()
            .replace('\'', "'\\''");
        self.write(
            ".tools/java",
            &format!("#!/bin/sh\nprintf '%s\\n' '{literal}'\n"),
        );
        fs::set_permissions(
            self.root.join(".tools/java"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str]) -> (bool, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_vitrifi"))
            .args([
                "--root",
                self.root.to_str().unwrap(),
                "--json",
                "--java-analyzer",
                ".tools/analyzer.jar",
                "--classpath-file",
                "target/classpath.txt",
            ])
            .args(args)
            .env("VITRIFI_JAVA", self.root.join(".tools/java"))
            .output()
            .unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), value)
    }
}

#[test]
fn lint_works_before_implementation_and_conformance_reports_missing_source() {
    let fixture = Fixture::new();
    let (passed, lint) = fixture.run(&["lint", "--target", TARGET]);
    assert!(passed, "{lint}");
    assert_eq!(lint["status"], "passed");
    let (passed, conform) = fixture.run(&["conform", "--target", TARGET]);
    assert!(!passed);
    assert_eq!(conform["diagnostics"][0]["code"], "TARGET_ANALYSIS_FAILED");
    fixture.write(TARGET, &CONTRACT.replace("effects [read Carts];", ""));
    let (passed, lint) = fixture.run(&["lint", "--target", TARGET]);
    assert!(!passed);
    assert_eq!(lint["status"], "failed");
}

#[test]
fn mixed_directory_check_and_verify_cannot_ignore_authored_policy() {
    let fixture = Fixture::new();
    fixture.adapter(true, true);
    assert!(fixture.run(&["conform", "--target", TARGET]).0);
    let authored = fs::read(fixture.root.join(TARGET)).unwrap();
    let (passed, generation) = fixture.run(&["generate"]);
    assert!(passed);
    assert_eq!(generation["target_conformance"], "not_checked");
    assert_eq!(fs::read(fixture.root.join(TARGET)).unwrap(), authored);
    assert!(fixture.run(&["check"]).0);

    fixture.adapter(true, false);
    let (passed, check) = fixture.run(&["check", "--java"]);
    assert!(!passed);
    assert!(check["conformance"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "TARGET_POLICY_ANNOTATION"));
    let (passed, verify) = fixture.run(&["verify", "--", "sh", "-c", "touch should-not-exist"]);
    assert!(!passed);
    assert_eq!(verify["executed"], false);
    assert!(!fixture.root.join("should-not-exist").exists());
    assert!(!fixture.run(&["generate", "--target", TARGET]).0);
}

#[test]
fn older_analyzer_cannot_claim_target_conformance() {
    let fixture = Fixture::new();
    fixture.adapter(false, true);
    let (passed, report) = fixture.run(&["conform"]);
    assert!(!passed);
    assert!(report["conformance"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "TARGET_INVENTORY"));
}

#[test]
fn automatic_selection_cannot_pass_if_another_target_appears_during_analysis() {
    let fixture = Fixture::new();
    fixture.adapter(true, true);
    let adapter = fs::read_to_string(fixture.root.join(".tools/java")).unwrap();
    let added = fixture
        .root
        .join("Added.target.vitrifi")
        .to_str()
        .unwrap()
        .replace('\'', "'\\''");
    fixture.write(
        ".tools/java",
        &adapter.replacen(
            "#!/bin/sh\n",
            &format!("#!/bin/sh\nprintf '%s\\n' 'target feature Added {{}}' > '{added}'\n"),
            1,
        ),
    );
    let (passed, report) = fixture.run(&["conform"]);
    assert!(!passed, "{report}");
    assert_eq!(report["status"], "stale");
    // An explicit selection intentionally does not claim unrelated targets.
    assert!(fixture.run(&["conform", "--target", TARGET]).0);
}

#[test]
fn evidence_requires_selected_target_identity_and_tracks_target_contents() {
    let fixture = Fixture::new();
    fixture.adapter(true, true);
    let selected = targets::load(&fixture.root, &[TARGET.into()]).unwrap();
    let mut report = json!({"status":"passed","inputs":selected.inputs,
        "workspace_revision":compiler::workspace_digest(&fixture.root).unwrap()});
    // A previously passing generated-only session does not prove the new target.
    evidence::save(&fixture.root, "java-evidence.json", &report).unwrap();
    let (passed, stale) = fixture.run(&["evidence", "--target", TARGET]);
    assert!(!passed);
    assert_eq!(stale["status"], "stale");
    report["target"] = json!({"revision":selected.revision,"files":selected.files,"entries":[]});
    evidence::save(&fixture.root, "java-evidence.json", &report).unwrap();
    assert!(fixture.run(&["evidence", "--target", TARGET]).0);
    fixture.write(
        TARGET,
        &CONTRACT.replace("Preview without writes", "Revised reviewed policy"),
    );
    let (passed, stale) = fixture.run(&["evidence", "--target", TARGET]);
    assert!(!passed);
    assert_eq!(stale["status"], "stale");
}

#[test]
fn target_selection_preserves_root_aliases_but_rejects_internal_symlinks_and_ignored_arguments() {
    let fixture = Fixture::new();
    let aliases = tempfile::tempdir().unwrap();
    let alias = aliases.path().join("checkout");
    std::os::unix::fs::symlink(&fixture.root, &alias).unwrap();
    let target = alias.join(TARGET);
    let (passed, report) = fixture.run(&[
        "lint",
        "--root",
        alias.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
    ]);
    assert!(passed, "{report}");
    std::os::unix::fs::symlink(TARGET, fixture.root.join("Link.target.vitrifi")).unwrap();
    assert!(
        !fixture
            .run(&[
                "lint",
                "--root",
                alias.to_str().unwrap(),
                "--target",
                alias.join("Link.target.vitrifi").to_str().unwrap(),
            ])
            .0
    );
    assert!(
        !fixture
            .run(&["check", "Unrelated.vitrifi", "--target", TARGET])
            .0
    );
    assert!(!fixture.run(&["lint", TARGET, "ignoredMember"]).0);
}

#[test]
fn multiple_explicit_targets_are_linted_and_missing_selected_files_fail() {
    let fixture = Fixture::new();
    fixture.write("Other.target.vitrifi", CONTRACT);
    assert!(
        !fixture
            .run(&[
                "lint",
                "--target",
                TARGET,
                "--target",
                "Other.target.vitrifi"
            ])
            .0
    );
    assert!(
        !fixture
            .run(&["lint", "--target", "Missing.target.vitrifi"])
            .0
    );
}
