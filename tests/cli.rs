//! Public CLI acceptance tests: feature source is compiled, never evaluated.
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_vitrifi");

#[test]
fn source_compile_errors_remain_machine_readable() {
    let fixture = Fixture::new();
    fixture.write("feature.vitrifi", "feature Broken { operation apply() -> Missing { bind python \"./app.py\" symbol \"apply\"; } }");
    fixture.write("app.py", "def apply(): pass\n");
    let result = document(&fixture.cli(&["source", "feature.vitrifi", "apply"]), 1);
    assert_eq!(result["status"], "compile_failed");
}

#[test]
fn corrupt_evidence_is_an_error_instead_of_a_panic() {
    let fixture = Fixture::new();
    fixture.write(".vitrifi/evidence.json", "42");
    let result = document(&fixture.cli(&["evidence"]), 2);
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("Invalid evidence"));
}

struct Fixture {
    root: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn path(&self) -> &Path {
        self.root.path()
    }
    fn write(&self, path: &str, source: &str) {
        let destination = self.path().join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, source).unwrap();
    }
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .arg("--root")
            .arg(self.path())
            .arg("--json")
            .current_dir(self.path())
            .output()
            .unwrap()
    }
    fn feature_with_check(&self, command: &[&str], timeout: u64) {
        self.write(
            "feature.vitrifi",
            &format!(
                r#"
feature Fixture {{
  requirement Covered "The operation preserves the promised behavior.";
  requirement Unchecked "A claim without linked evidence.";
  check Behavior {{
    run {};
    covers [Covered];
    timeout {timeout};
  }}
}}
"#,
                serde_json::to_string(command).unwrap()
            ),
        );
    }
    fn git(&self, args: &[&str]) -> String {
        let result = Command::new("git")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "user.name=Vitrifi Test",
                "-c",
                "user.email=vitrifi@example.invalid",
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                self.path().join("absent-global-gitconfig"),
            )
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }
}

fn document(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "Invalid JSON ({err}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
fn requirement<'a>(value: &'a Value, name: &str) -> &'a Value {
    value["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == name)
        .unwrap()
}

#[test]
fn check_emits_structured_diagnostics_and_failing_exit_code() {
    let fixture = Fixture::new();
    fixture.write(
        "feature.vitrifi",
        r#"
feature Fixture {
  resource Cart;
  operation apply() -> Unit {
    bind python "./app.py" symbol "apply";
    ensures [Cart];
  }
}
"#,
    );
    fixture.write("app.py", "def apply():\n    pass\n");
    let result = document(&fixture.cli(&["check", "feature.vitrifi"]), 1);
    assert_eq!(result["status"], "failed");
    let diagnostic = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "TYPE_MISMATCH")
        .unwrap();
    assert_eq!(diagnostic["location"]["path"], "feature.vitrifi");
    assert!(diagnostic["location"]["line"].as_u64().unwrap() > 1);
}

#[test]
fn init_creates_local_feature_source_and_preserves_existing_content() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path().join("checkout")).unwrap();
    let created = document(&fixture.cli(&["init", "checkout/discounts.vitrifi"]), 0);
    assert!(created["created"]
        .as_str()
        .unwrap()
        .ends_with("checkout/discounts.vitrifi"));
    let source = fs::read_to_string(fixture.path().join("checkout/discounts.vitrifi")).unwrap();
    assert!(source.contains("feature Example"));
    let rejected = document(&fixture.cli(&["init", "checkout/discounts.vitrifi"]), 2);
    assert!(rejected["error"]
        .as_str()
        .unwrap()
        .contains("Cannot create"));
    assert_eq!(
        source,
        fs::read_to_string(fixture.path().join("checkout/discounts.vitrifi")).unwrap()
    );
    assert!(!fixture.path().join("vitrifi.json").exists());
}

#[test]
fn check_and_compile_never_execute_declared_commands() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&[BIN, "init", "should-not-exist.vitrifi", "--root", "."], 5);
    assert_eq!(
        document(&fixture.cli(&["check", "feature.vitrifi"]), 0)["status"],
        "passed"
    );
    let compiled = document(&fixture.cli(&["compile", "feature.vitrifi"]), 0);
    assert!(compiled["compiled"]
        .as_str()
        .unwrap()
        .ends_with(".vitrifi/program.json"));
    assert!(!fixture.path().join("should-not-exist.vitrifi").exists());
    assert!(!fixture.path().join(".vitrifi/evidence.json").exists());
}

#[test]
fn verify_records_passing_evidence_only_for_linked_requirements() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&[BIN, "--version"], 5);
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 0);
    assert_eq!(result["status"], "passed");
    assert_eq!(result["fresh"], true);
    assert_eq!(result["results"][0]["exit_code"], 0);
    assert!(result["results"][0]["output"]
        .as_str()
        .unwrap()
        .contains("vitrifi"));
    assert_eq!(
        requirement(&result, "Fixture::Covered")["status"],
        "covered_by_passing_checks"
    );
    assert_eq!(
        requirement(&result, "Fixture::Unchecked")["status"],
        "declared_unchecked"
    );
    let loaded = document(&fixture.cli(&["evidence"]), 0);
    assert_eq!(loaded["workspace_revision"], result["workspace_revision"]);
    assert_eq!(loaded["fresh"], true);
}

#[test]
fn missing_runner_fails_its_requirement_and_verification() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&["./this-runner-does-not-exist"], 5);
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 1);
    assert_eq!(result["status"], "failed");
    assert_eq!(result["results"][0]["status"], "failed");
    assert!(result["results"][0]["exit_code"].is_null());
    assert_eq!(
        requirement(&result, "Fixture::Covered")["status"],
        "check_failed"
    );
}

#[test]
fn changed_test_input_marks_saved_evidence_and_linked_requirements_stale() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&[BIN, "--version"], 5);
    fixture.write("test-cases.txt", "original acceptance input\n");
    document(&fixture.cli(&["verify", "feature.vitrifi"]), 0);
    fixture.write("test-cases.txt", "modified acceptance input\n");
    let loaded = document(&fixture.cli(&["evidence"]), 1);
    assert_eq!(loaded["status"], "stale");
    assert_eq!(loaded["recorded_status"], "passed");
    assert_eq!(requirement(&loaded, "Fixture::Covered")["status"], "stale");
    assert_eq!(
        requirement(&loaded, "Fixture::Unchecked")["status"],
        "declared_unchecked"
    );
}

#[test]
fn a_command_mutating_source_cannot_produce_current_passing_evidence() {
    let fixture = Fixture::new();
    fixture.feature_with_check(
        &[BIN, "init", "generated.vitrifi", "--root", ".", "--json"],
        5,
    );
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 1);
    assert!(fixture.path().join("generated.vitrifi").exists());
    assert_eq!(result["results"][0]["status"], "passed");
    assert_eq!(result["status"], "stale");
    assert_eq!(result["fresh"], false);
    assert_eq!(requirement(&result, "Fixture::Covered")["status"], "stale");
}

#[cfg(unix)]
#[test]
fn timed_out_check_has_explicit_failure_evidence() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&["/bin/sleep", "5"], 1);
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 1);
    assert_eq!(result["status"], "failed");
    assert_eq!(result["results"][0]["status"], "timeout");
    assert_eq!(
        requirement(&result, "Fixture::Covered")["status"],
        "check_failed"
    );
}

#[test]
fn zero_checks_never_produces_a_successful_test_claim() {
    let fixture = Fixture::new();
    fixture.write(
        "feature.vitrifi",
        "feature Fixture { requirement Pending \"Still unchecked.\"; }",
    );
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 1);
    assert_eq!(result["status"], "not_run");
    assert_eq!(result["results"], json!([]));
    assert_eq!(
        requirement(&result, "Fixture::Pending")["status"],
        "declared_unchecked"
    );
}

#[test]
fn pr_review_shows_requirement_changes_and_untracked_modules_but_ignores_formatting() {
    let fixture = Fixture::new();
    fixture.write(
        "checkout/discounts.vitrifi",
        "feature Checkout.Discounts { requirement Owner \"Only the owner can modify the cart.\"; }",
    );
    fixture.write(
        "storage/cart.vitrifi",
        "feature Storage.Cart { export resource Carts; }",
    );
    fixture.git(&["init", "-q"]);
    fixture.git(&["add", "checkout/discounts.vitrifi", "storage/cart.vitrifi"]);
    fixture.git(&["commit", "-q", "-m", "Feature contracts"]);
    let base = fixture.git(&["rev-parse", "HEAD"]).trim().to_owned();

    fixture.write("checkout/discounts.vitrifi", "// A normal PR edits the feature contract.\nfeature Checkout.Discounts {\n  requirement Owner \"Owners and administrators can modify the cart.\";\n}\n");
    fixture.write("storage/cart.vitrifi", "// Formatting and locations are irrelevant.\n\nfeature Storage.Cart {\n\n  export resource Carts;\n}\n");
    fixture.write(
        "audit/discounts.vitrifi",
        "feature Audit.Discounts { requirement Recorded \"Accepted discounts are recorded.\"; }",
    );

    let result = document(&fixture.cli(&["review", "--base", "HEAD"]), 0);
    assert_eq!(result["base"], base);
    let changes = result["changes"].as_array().unwrap();
    let owner = changes
        .iter()
        .find(|c| {
            c["file"] == "checkout/discounts.vitrifi" && c["declaration"] == "requirement Owner"
        })
        .unwrap();
    assert_eq!(owner["change"], "modified");
    assert_eq!(
        owner["before"]["body"]["text"],
        "Only the owner can modify the cart."
    );
    assert_eq!(
        owner["after"]["body"]["text"],
        "Owners and administrators can modify the cart."
    );
    assert!(!changes.iter().any(|c| c["file"] == "storage/cart.vitrifi"));
    assert!(changes
        .iter()
        .any(|c| c["file"] == "audit/discounts.vitrifi"
            && c["declaration"] == "requirement Recorded"
            && c["change"] == "added"));

    let scoped = document(
        &fixture.cli(&["review", "checkout/discounts.vitrifi", "--base", "HEAD"]),
        0,
    );
    assert_eq!(scoped["changes"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["changes"][0]["file"], "checkout/discounts.vitrifi");
    let dotted = document(
        &fixture.cli(&["review", "./checkout/discounts.vitrifi", "--base", "HEAD"]),
        0,
    );
    assert_eq!(dotted["changes"], scoped["changes"]);
}

#[test]
fn failed_compilation_prevents_verification_commands() {
    let fixture = Fixture::new();
    fixture.feature_with_check(&[BIN, "init", "should-not-exist.vitrifi", "--root", "."], 5);
    let path = fixture.path().join("feature.vitrifi");
    let broken = fs::read_to_string(&path)
        .unwrap()
        .replace("covers [Covered]", "covers [UnknownRequirement]");
    fs::write(path, broken).unwrap();
    let result = document(&fixture.cli(&["verify", "feature.vitrifi"]), 1);
    assert_eq!(result["status"], "compile_failed");
    assert!(!fixture.path().join("should-not-exist.vitrifi").exists());
    assert!(!fixture.path().join(".vitrifi/evidence.json").exists());
}
