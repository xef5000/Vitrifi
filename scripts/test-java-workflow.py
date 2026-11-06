#!/usr/bin/env python3
"""Exercise real javac, Spring, JDBC and the CLI in an isolated temporary checkout.

Build with cargo build and mvn install first. JAVA_HOME must select a full JDK.
No source in the working checkout is modified by these mutation tests.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vitrifi", type=Path, default=repo / "target/debug/vitrifi")
    parser.add_argument("--analyzer", type=Path, default=repo / "integrations/java/target/vitrifi-java-0.3.0.jar")
    parser.add_argument("--maven", default="mvn")
    parser.add_argument("--maven-repo", type=Path)
    args = parser.parse_args()
    binary, analyzer = args.vitrifi.resolve(), args.analyzer.resolve()
    if not binary.is_file() or not analyzer.is_file():
        parser.error("Build the Rust binary and install the Maven reactor first")
    maven = str(Path(args.maven).resolve()) if os.sep in args.maven else args.maven
    runner = [maven, "-o", "-B", "-ntp"]
    if args.maven_repo:
        runner.append("-Dmaven.repo.local=" + str(args.maven_repo.resolve()))
    runner.append("test")

    with tempfile.TemporaryDirectory(prefix="vitrifi-java-workflow-") as directory:
        checkout = Path(directory)
        shutil.copy2(repo / "pom.xml", checkout / "pom.xml")
        shutil.copy2(repo / ".gitignore", checkout / ".gitignore")
        app = checkout / "examples/spring-checkout"
        shutil.copytree(repo / "examples/spring-checkout", app,
                        ignore=shutil.ignore_patterns("target", ".vitrifi"))
        # Exercise generated-only compatibility separately from authored-target
        # acceptance tests in test-target-workflow.py.
        for target in app.rglob("*.target.vitrifi"):
            target.unlink()
        (app / "target").mkdir()
        shutil.copy2(repo / "examples/spring-checkout/target/vitrifi-classpath.txt",
                     app / "target/vitrifi-classpath.txt")
        # A standalone Maven build resolves reactor artifacts from the local
        # repository. Prepare that exact classpath before fingerprinting it;
        # changing dependencies during verification must still invalidate evidence.
        subprocess.run([*runner[:-1], "generate-resources"], cwd=app,
                       check=True, capture_output=True, text=True)
        java = app / "src/main/java/dev/vitrifi/example/checkout/DiscountService.java"
        tests = app / "src/test/java/dev/vitrifi/example/checkout/DiscountServiceTest.java"
        original, original_tests = java.read_text(), tests.read_text()

        def command(command, expected=0, extra=()):
            result = subprocess.run([str(binary), command, "--java", "--root", str(app),
                                     "--java-analyzer", str(analyzer), *extra],
                                    text=True, capture_output=True)
            try:
                value = json.loads(result.stdout)
            except ValueError:
                raise AssertionError(result.stdout + result.stderr) from None
            assert result.returncode == expected, (command, result.returncode, value, result.stderr)
            return value

        def git(*arguments):
            result = subprocess.run(["git", "-c", "core.fsmonitor=false", "-c",
                                     "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null",
                                     "-C", str(checkout), *arguments],
                                    capture_output=True, text=True)
            assert result.returncode == 0, (arguments, result.stderr)

        baseline = command("generate")
        assert baseline["status"] == "passed", baseline
        assert command("check")["status"] == "passed"
        verified = command("verify", extra=("--", *runner))
        assert verified["runtime"]["status"] == "passed", verified
        print("PASS: generated source model and actual Spring/H2 scenario evidence", flush=True)

        git("init", "-q")
        git("add", ".")
        git("-c", "user.name=Vitrifi Test", "-c", "user.email=vitrifi@example.invalid",
            "commit", "-qm", "baseline")
        java.write_text(original.replace("preview(String cartId)", "preview(String cartId, int revision)"))
        changed = command("check", expected=1)
        assert changed["status"] == "stale", changed
        review = command("review", extra=("--base", "HEAD"))
        assert any("preview" in item["fact"] for item in review["changes"]), review
        command("generate")
        command("check")
        print("PASS: host-only signature change updates generation and architectural review", flush=True)

        java.write_text(original.replace("return carts.load(cartId);",
                                         'carts.save(cartId, 0, "bad");\n        return carts.load(cartId);'))
        forbidden = command("generate", expected=1)
        assert any(d["code"] == "FORBIDDEN_EFFECT" for d in forbidden["diagnostics"]), forbidden
        assert command("check", expected=1)["status"] == "failed"
        print("PASS: regeneration cannot approve a forbidden implementation write", flush=True)

        java.write_text(original.replace("return carts.load(cartId);",
                                         "java.util.function.Supplier<Cart> fetch = () -> carts.load(cartId);\n        return fetch.get();"))
        unknown = command("generate", expected=1)
        assert unknown["status"] == "incomplete" and unknown["unresolved"], unknown
        print("PASS: dynamic dispatch remains incomplete instead of being called verified", flush=True)

        java.write_text(original.replace("Cart cart = carts.load(cartId);",
                                         "Cart cart = carts.load(cartId);\n        carts.save(cart.id(), cart.total(), cart.coupon());"))
        command("generate")
        rejection = command("verify", expected=1, extra=("--", *runner))
        assert rejection["runtime"]["status"] == "failed", rejection
        assert any(v["kind"] == "no_effect_on_error" for v in rejection["runtime"]["violations"]), rejection
        print("PASS: a write before rejection fails actual policy evidence even when tests catch the rejection", flush=True)

        java.write_text(original)
        command("generate")
        reduced_tests = original_tests
        for name in ["rejectsAnotherOwner", "rejectsInvalidCoupon", "rejectsReplacement"]:
            reduced_tests = reduced_tests.replace("@Test\n    void " + name, "// Error path intentionally omitted\n    void " + name)
        tests.write_text(reduced_tests)
        untested = command("verify", expected=1, extra=("--", *runner))
        assert any(p["kind"] == "no_effect_on_error" and p["status"] == "untested"
                   for p in untested["runtime"]["policy_coverage"]), untested
        print("PASS: success-only scenarios cannot satisfy an error-path policy", flush=True)

        absent = command("verify", expected=1, extra=("--", sys.executable, "-c", "pass"))
        assert absent["status"] == "incomplete" and absent["runtime"] is None, absent
        print("PASS: a zero exit status without runtime evidence cannot pass verification", flush=True)


if __name__ == "__main__":
    main()
