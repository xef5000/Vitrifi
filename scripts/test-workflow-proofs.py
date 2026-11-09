#!/usr/bin/env python3
"""Exercise certificate production/checking against real Git, javac and Spring.

Build Rust and install the Maven reactor first. All mutations use a temporary
repository. --demo-dir retains a complete reviewable refactor and certificate.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

PACKAGE = Path("src/main/java/dev/vitrifi/example/checkout")
WORKFLOW = "Checkout.Discounts.workflow.vitrifi"


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vitrifi", type=Path, default=repo / "target/debug/vitrifi")
    parser.add_argument("--analyzer", type=Path, default=repo / "integrations/java/target/vitrifi-java-0.3.0.jar")
    parser.add_argument("--maven", default="mvn")
    parser.add_argument("--maven-repo", type=Path)
    parser.add_argument("--demo-dir", type=Path)
    args = parser.parse_args()
    binary, analyzer = args.vitrifi.resolve(), args.analyzer.resolve()
    maven = str(Path(args.maven).resolve()) if os.sep in args.maven else args.maven
    runner = [maven, "-o", "-B", "-ntp"]
    if args.maven_repo:
        runner += ["-Dmaven.repo.local=" + str(args.maven_repo.resolve())]

    def run(command, cwd=None):
        result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=180)
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    with tempfile.TemporaryDirectory(prefix="vitrifi-proofs-") as temporary:
        root = (args.demo_dir or Path(temporary)).resolve()
        if args.demo_dir:
            root.mkdir(parents=True, exist_ok=False)
        shutil.copy2(repo / "pom.xml", root / "pom.xml")
        shutil.copy2(repo / ".gitignore", root / ".gitignore")
        app = root / "examples/workflow-checkout"
        shutil.copytree(repo / "examples/workflow-checkout", app,
                        ignore=shutil.ignore_patterns("target", ".vitrifi"))
        workflow = app / PACKAGE / WORKFLOW
        service = app / PACKAGE / "DiscountService.java"
        actions = app / PACKAGE / "CheckoutActions.java"
        target = app / PACKAGE / "Checkout.Discounts.target.vitrifi"
        original = workflow.read_text()
        original_actions, original_target = actions.read_text(), target.read_text()
        # Resource changes also belong to the unchanged native context, including binaries.
        resource = app / "src/main/resources/sample.bin"
        resource.parent.mkdir(parents=True)
        resource.write_bytes(b"\x00\xffnative resource\x80")
        git = ["git", "-c", "core.fsmonitor=false", "-c", "commit.gpgsign=false",
               "-c", "core.hooksPath=/dev/null", "-c", "user.name=Vitrifi Test",
               "-c", "user.email=vitrifi@example.invalid"]
        run([*git, "init", "-q"], root)
        run([*runner, "generate-resources"], app)
        # A classpath directory exercises content hashing and ordered classpath pins.
        dependency = root / "dependency"
        dependency.mkdir()
        (dependency / "resource.txt").write_text("dependency-v1")
        cp = app / "target/vitrifi-classpath.txt"
        if not args.demo_dir:
            cp.write_text(cp.read_text().strip() + os.pathsep + str(dependency))
        original_cp = cp.read_text()
        run([*git, "add", "."], root)
        run([*git, "commit", "-qm", "approved workflow baseline"], root)
        base = run([*git, "rev-parse", "HEAD"], root).strip()

        def command(*arguments, error=None, extra=()):
            result = subprocess.run([str(binary), *arguments, "--root", str(app),
                "--java-analyzer", str(analyzer), "--json", *extra], capture_output=True, text=True, timeout=180)
            value = json.loads(result.stdout)
            if error:
                assert result.returncode != 0 and error.lower() in json.dumps(value).lower(), (arguments, value)
            else:
                assert result.returncode == 0, (arguments, value, result.stderr)
            return value

        def prove(name="refactor.proof.json", error=None):
            return command("workflow", "prove", str(PACKAGE / WORKFLOW),
                           "--base", base, "--out", name, error=error)

        def check(name="refactor.proof.json", error=None, extra=()):
            return command("workflow", "proof-check", name, "--base", base, error=error, extra=extra)

        # Flat guards become nested branches; the same native leaves stay unchanged.
        start = original.index("    if owner {")
        end = original.index("\n  operation preview", start)
        nested = original[:start] + '''    if owner {
      let valid = call validCode(code);
      if valid {
        let unused = call available(cart);
        if unused {
          let result = call save(cart, code);
          return result;
        } else {
          reject "A coupon is already applied";
        }
      } else {
        reject "Coupon is invalid";
      }
    } else {
      reject "Only the cart owner can apply a coupon";
    }
  }
''' + original[end:]
        workflow.write_text(nested)
        prove(error="WORKFLOW_STALE")
        assert not (app / "refactor.proof.json").exists()
        command("workflow", "build")
        produced = prove()
        checked = check()
        assert produced["claim"] == checked["claim"] == "workflow_equivalent"
        assert produced["checked_steps"] >= 3
        certificate = json.loads((app / "refactor.proof.json").read_text())
        assert certificate["context"]["environment"]["classpath"]
        assert certificate["context"]["targets"]
        assert not any(str(root) in key for key in certificate["context"]["files"])
        prove("reproduced.proof.json")
        assert (app / "reproduced.proof.json").read_bytes() == (app / "refactor.proof.json").read_bytes()
        prove(error="PROOF_OUTPUT_EXISTS")
        print("PASS: flat-to-nested checkout produces a replayable certificate with unchanged effectful Java leaves", flush=True)

        if args.demo_dir:
            command("generate")
            command("check", "--java")
            # Run actual Spring/H2 acceptance, rejection and infrastructure tests too.
            command("workflow", "verify", extra=("--", *runner, "test"))
            print(f"Demo retained at {root}; base commit {base}")
            print(f"Certificate: {app / 'refactor.proof.json'}")
            return

        # Forge every significant assumption; the checker never invokes proof search.
        def forged(name, mutate, error):
            value = copy.deepcopy(certificate)
            mutate(value)
            (app / name).write_text(json.dumps(value))
            check(name, error=error)

        forged("rule.proof.json", lambda c: c["before_steps"][0].update(rule="commute_calls"), "PROOF_FORMAT")
        forged("chain.proof.json", lambda c: c["before_steps"][0].update(after_sha256="0" * 64), "PROOF_CHAIN")
        forged("common.proof.json", lambda c: c.update(common_term_sha256="0" * 64), "PROOF_COMMON_HASH")
        forged("steps.proof.json", lambda c: c.update(before_steps=[]), "PROOF_NOT_EQUIVALENT")
        forged("version.proof.json", lambda c: c.update(semantics="vitrifi.workflow/99"), "PROOF_VERSION")
        forged("checker.proof.json", lambda c: c.update(implementation_sha256="0" * 64), "PROOF_VERSION")
        forged("context.proof.json", lambda c: c["context"]["files"].clear(), "PROOF_CONTEXT_STALE")
        forged("trusted.proof.json", lambda c: c.update(verified=True), "PROOF_FORMAT")
        print("PASS: forged laws, chains, endpoints, versions and context cannot manufacture a proof", flush=True)

        workflow.write_text(nested + "// changed bytes\n")
        check(error="PROOF_STALE")
        workflow.write_text(nested)
        actions.write_text(original_actions.replace("- 1000", "- 900"))
        check(error="PROOF_CONTEXT_CHANGED")
        prove("changed-leaf.proof.json", error="PROOF_CONTEXT_CHANGED")
        assert not (app / "changed-leaf.proof.json").exists()
        actions.write_text(original_actions)
        target.write_text(original_target.replace("without writes on rejection", "changed policy rationale"))
        check(error="PROOF_CONTEXT_CHANGED")
        target.write_text(original_target)
        resource.write_bytes(b"changed binary resource")
        check(error="PROOF_CONTEXT_CHANGED")
        resource.write_bytes(b"\x00\xffnative resource\x80")
        parent_pom = root / "pom.xml"
        original_pom = parent_pom.read_text()
        parent_pom.write_text(original_pom + "\n<!-- changed build -->\n")
        check(error="PROOF_CONTEXT_CHANGED")
        parent_pom.write_text(original_pom)
        extra_source = app / PACKAGE / "Unmarked.java"
        extra_source.write_text("package dev.vitrifi.example.checkout; class Unmarked {}")
        check(error="PROOF_CONTEXT_CHANGED")
        extra_source.unlink()
        # Baseline root ignore rules are fixed; add an ignored source using the existing target/ exclusion.
        hidden = app / "src/main/java/target/Hidden.java"
        hidden.parent.mkdir()
        hidden.write_text("class Hidden {}")
        check(error="PROOF_CONTEXT_CHANGED")
        hidden.unlink()
        hidden.parent.rmdir()
        print("PASS: leaf, target, resources, parent build and native source-set (including ignored files) changes invalidate the certificate", flush=True)

        (dependency / "resource.txt").write_text("dependency-v2")
        check(error="PROOF_CONTEXT_STALE")
        (dependency / "resource.txt").write_text("dependency-v1")
        entries = original_cp.strip().split(os.pathsep)
        cp.write_text(os.pathsep.join(reversed(entries)))
        check(error="PROOF_CONTEXT_STALE")
        cp.write_text(original_cp)
        check(extra=("--target", str(PACKAGE / "Checkout.Discounts.target.vitrifi")))
        forged("selection.proof.json", lambda c: c["context"].update(targets=[]), "PROOF_CONTEXT_STALE")
        forged("java.proof.json", lambda c: c["context"]["environment"]["java_runtime"].update({"java.runtime.version": "changed"}), "PROOF_CONTEXT_STALE")
        alternative_analyzer = root / "alternative-analyzer.jar"
        shutil.copy2(analyzer, alternative_analyzer)
        with zipfile.ZipFile(alternative_analyzer, "a") as archive:
            archive.writestr("proof-test-resource.txt", "changed artifact bytes")
        check(extra=("--java-analyzer", str(alternative_analyzer)), error="PROOF_CONTEXT_STALE")
        print("PASS: dependency contents/order, analyzer, target selection and Java runtime are reconstructed and pinned", flush=True)

        # Altering a rejection message has identical effect bounds but changes semantics.
        workflow.write_text(nested.replace('reject "Coupon is invalid";', 'reject "different message";'))
        command("workflow", "build")
        prove("message.proof.json", error="PROOF_NOT_FOUND")
        workflow.write_text(nested.replace("let valid = call validCode(code);", "let extra = call validCode(code);\n      let valid = call validCode(code);"))
        command("workflow", "build")
        prove("duplication.proof.json", error="PROOF_NOT_FOUND")
        workflow.write_text(nested)
        command("workflow", "build")
        check()
        print("PASS: equal effect sets do not certify changed rejection messages or duplicated empty-effect calls", flush=True)

        command("workflow", "prove", str(PACKAGE / WORKFLOW), "--base", base,
                "--out", "../outside.proof.json", error="PROOF_PATH")
        (app / "alias.proof.json").symlink_to(app / "refactor.proof.json")
        check("alias.proof.json", error="Symlinked")
        run([*git, "add", "."], root)
        run([*git, "commit", "-qm", "refactored workflow"], root)
        command("workflow", "proof-check", "refactor.proof.json", "--base", "HEAD", error="PROOF_BASELINE")
        # It is valid against the recorded immutable base, even after HEAD advances.
        assert command("workflow", "proof-check", "refactor.proof.json")["base_pinned_by_caller"] is False
        print("PASS: safe paths, immutable baseline and caller-selected review base are enforced", flush=True)
        hidden.parent.mkdir(exist_ok=True)
        hidden.write_text("class Hidden {}")
        run([*git, "add", "-f", str(hidden.relative_to(root))], root)
        run([*git, "commit", "-qm", "tracked source outside adapter coverage"], root)
        command("workflow", "prove", str(PACKAGE / WORKFLOW), "--base", "HEAD",
                "--out", "coverage.proof.json", error="PROOF_SOURCE_COVERAGE")
        hidden.unlink()
        service.write_text(service.read_text() + "// tampered generated baseline\n")
        run([*git, "add", "-u"], root)
        run([*git, "commit", "-qm", "stale generated baseline"], root)
        command("workflow", "prove", str(PACKAGE / WORKFLOW), "--base", "HEAD",
                "--out", "stale-baseline.proof.json", error="PROOF_BASELINE_STALE")
        print("PASS: ignored baseline Java sources and stale committed services cannot earn certificates", flush=True)



if __name__ == "__main__":
    main()
