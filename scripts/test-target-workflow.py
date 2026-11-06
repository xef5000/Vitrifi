#!/usr/bin/env python3
"""Exercise authored targets against real javac, Spring and JDBC in a temporary app.

Build with cargo build and mvn install first. JAVA_HOME must select a full JDK.
All implementation/target mutations are isolated from the working checkout.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


JAVA_PACKAGE = Path("src/main/java/dev/vitrifi/example/checkout")
TARGET_NAME = "Checkout.Discounts.target.vitrifi"
SERVICE_NAME = "dev.vitrifi.example.checkout.DiscountService"


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vitrifi", type=Path, default=repo / "target/debug/vitrifi")
    parser.add_argument("--analyzer", type=Path,
                        default=repo / "integrations/java/target/vitrifi-java-0.3.0.jar")
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

    with tempfile.TemporaryDirectory(prefix="vitrifi-target-workflow-") as directory:
        checkout = Path(directory).resolve()
        shutil.copy2(repo / "pom.xml", checkout / "pom.xml")
        shutil.copy2(repo / ".gitignore", checkout / ".gitignore")
        app = checkout / "examples/spring-checkout"
        shutil.copytree(repo / "examples/spring-checkout", app,
                        ignore=shutil.ignore_patterns("target", ".vitrifi"))
        target = app / JAVA_PACKAGE / TARGET_NAME
        java = app / JAVA_PACKAGE / "DiscountService.java"
        repository = app / JAVA_PACKAGE / "JdbcCartRepository.java"
        tests = app / "src/test/java/dev/vitrifi/example/checkout/DiscountServiceTest.java"
        original, original_repository = java.read_text(), repository.read_text()
        original_target, original_tests = target.read_text(), tests.read_text()

        def execute(arguments, expected=0, env=None):
            result = subprocess.run([str(binary), *arguments], text=True,
                                    capture_output=True, env=env, timeout=180)
            try:
                value = json.loads(result.stdout)
            except ValueError:
                raise AssertionError(result.stdout + result.stderr) from None
            assert result.returncode == expected, (arguments, result.returncode, value, result.stderr)
            return value

        def command(name, expected=0, extra=(), explicit_target=True):
            arguments = [name, "--json", "--root", str(app),
                         "--java-analyzer", str(analyzer)]
            if explicit_target:
                arguments.extend(["--target", str(target)])
            arguments.extend(extra)
            return execute(arguments, expected)

        def diagnose(value, *terms):
            records = value.get("conformance", value).get("diagnostics", [])
            text = json.dumps(records).lower()
            assert records and any(term.lower() in text for term in terms), value

        def restore():
            java.write_text(original)
            repository.write_text(original_repository)
            target.write_text(original_target)
            tests.write_text(original_tests)

        def append_member(source, member):
            head, end = source.rsplit("}", 1)
            return head + member + "}\n" + end.lstrip("\n")

        # Target authoring works before implementation and without a Java toolchain.
        design = checkout / "design-only"
        design.mkdir()
        design_target = design / TARGET_NAME
        design_target.write_text(original_target)
        lint_env = dict(os.environ, VITRIFI_JAVA=str(checkout / "missing-java"))
        lint = execute(["lint", "--json", "--root", str(design), "--target", str(design_target),
                        "--java-analyzer", str(checkout / "missing-analyzer.jar")], env=lint_env)
        assert lint["status"] == "passed", lint
        print("PASS: target linting needs neither an implementation nor a Java analyzer", flush=True)

        # Resolve the standalone Maven classpath before analysis fingerprints it.
        prepared = subprocess.run([*runner, "generate-resources"], cwd=app,
                                  text=True, capture_output=True, timeout=180)
        assert prepared.returncode == 0, prepared.stdout + prepared.stderr
        matching = command("conform", extra=("--java",))
        assert matching["status"] == "passed", matching
        generated = command("generate", explicit_target=False)
        assert generated["status"] == "passed" and target.read_text() == original_target, generated
        checked = command("check", explicit_target=False)
        assert checked["status"] == "passed" and checked["target"]["revision"], checked
        print("PASS: matching source conforms, generation preserves the target, and check discovers it", flush=True)

        extracted = original.replace("return carts.load(cartId);", "return loadForPreview(cartId);")
        java.write_text(append_member(extracted, "\n    private Cart loadForPreview(String cartId) {\n"
                                      "        return carts.load(cartId);\n    }\n"))
        assert command("conform")["status"] == "passed"
        assert target.read_text() == original_target
        print("PASS: private helper extraction preserves conformance without a target rewrite", flush=True)

        restore()
        forbid = '    @Forbid(action = "write", resource = "Carts")\n'
        assert forbid in original
        java.write_text(original.replace(forbid, ""))
        missing_policy = command("conform", expected=1)
        assert missing_policy["status"] == "failed", missing_policy
        diagnose(missing_policy, "policy", "annotation", "forbid")
        blocked = command("verify", expected=1, extra=("--", sys.executable, "-c",
                          "raise SystemExit('preflight must prevent this command')"))
        assert blocked["status"] == "failed" and blocked["executed"] is False, blocked
        print("PASS: deleting a source policy fails conformance and prevents test execution", flush=True)

        restore()
        java.write_text("\n".join(line for line in original.splitlines()
                                  if not line.startswith("@Feature(")) + "\n")
        missing_feature = command("conform", expected=1)
        assert missing_feature["status"] == "failed", missing_feature
        diagnose(missing_feature, "feature", "marker")
        print("PASS: removing the feature marker cannot remove target coverage", flush=True)

        restore()
        assert '@ResourceTable(value = "Carts"' in original_repository
        repository.write_text(original_repository.replace('@ResourceTable(value = "Carts"',
                                                          '@ResourceTable(value = "Other"'))
        remapped = command("conform", expected=1)
        assert remapped["status"] == "failed", remapped
        diagnose(remapped, "resource", "mapping", "table")
        print("PASS: renaming the same database resource cannot escape the target", flush=True)

        restore()
        alias_mapping = app / JAVA_PACKAGE / "AliasMapping.java"
        alias_mapping.write_text('''package dev.vitrifi.example.checkout;
import dev.vitrifi.api.ResourceTable;
@ResourceTable(value = "Carts", table = "carts")
final class AliasMapping {}
''')
        repository.write_text(original_repository
            .replace('@ResourceTable(value = "Carts", table = "carts")',
                     '@ResourceTable(value = "Other", table = "public.carts")')
            .replace("FROM carts", "FROM public.carts")
            .replace("UPDATE carts", "UPDATE public.carts"))
        java.write_text(original.replace("return carts.load(cartId);",
            'carts.save(cartId, 0, "alias");\n        return carts.load(cartId);'))
        target.write_text(original_target
            .replace('resource Carts table "carts";',
                     'resource Carts table "carts";\n  resource Other table "public.carts";')
            .replace("effects [read Carts, write Carts];",
                     "effects [read Carts, write Carts, read Other, write Other];")
            .replace("effects [read Carts];", "effects [read Carts, read Other, write Other];"))
        # The source policy still forbids Carts, while the SQL maps the same H2
        # table to Other. Regenerating that source view must not approve it.
        assert command("generate", explicit_target=False)["status"] == "passed"
        alias = command("conform", expected=1)
        diagnose(alias, "alias", "table")
        blocked = command("verify", expected=1, extra=("--", sys.executable, "-c",
            "raise SystemExit('ambiguous aliases must fail before execution')"))
        assert blocked["status"] == "failed", blocked
        alias_mapping.unlink()
        print("PASS: qualified/unqualified table aliases cannot grant a forbidden write", flush=True)

        restore()
        counted = append_member(original, "\n    public int count() { return 1; }\n")
        java.write_text(counted)
        extra_entry = command("conform", expected=1)
        assert extra_entry["status"] == "failed", extra_entry
        diagnose(extra_entry, "public", "entry", "coverage")
        target.write_text(append_member(original_target,
            '\n  operation count() -> "int" {\n'
            f'    bind java "{SERVICE_NAME}.count()";\n'
            '    effects [];\n    dependencies [];\n  }\n'))
        assert command("conform")["status"] == "passed"
        java.write_text(counted.replace("public int count()", "public long count()"))
        native_drift = command("conform", expected=1)
        assert native_drift["status"] == "failed", native_drift
        diagnose(native_drift, "signature", "return", "type")
        print("PASS: coverage rejects extra public methods and native int-to-long drift is detected", flush=True)

        restore()
        java.write_text(original.replace("return carts.load(cartId);",
            "java.util.function.Supplier<Cart> fetch = () -> carts.load(cartId);\n        return fetch.get();"))
        unknown = command("conform", expected=1)
        assert unknown["status"] == "incomplete", unknown
        diagnose(unknown, "unknown", "unresolved")
        print("PASS: unsupported Supplier dispatch remains incomplete", flush=True)

        restore()
        foreign = app / "src/main/java/forbidden/Value.java"
        foreign.parent.mkdir(parents=True)
        foreign.write_text("package forbidden; public record Value(int value) {}\n")
        java.write_text(original.replace("return carts.load(cartId);",
            "new forbidden.Value(1).value();\n        return carts.load(cartId);"))
        missing_boundary = command("conform", expected=1)
        diagnose(missing_boundary, "boundary")
        target.write_text(original_target.replace("  operation applyCoupon",
            '  boundary Foreign java ["forbidden.Value"];\n\n  operation applyCoupon'))
        denied_boundary = command("conform", expected=1)
        diagnose(denied_boundary, "unpermitted", "dependency")
        target.write_text(target.read_text().replace("dependencies [Storage];",
                                                     "dependencies [Storage, Foreign];"))
        assert command("conform")["status"] == "passed"
        foreign.unlink()
        print("PASS: intrinsic record constructors and accessors still require boundary permission", flush=True)

        restore()
        unrelated = app / JAVA_PACKAGE / "Unrelated.java"
        unrelated.write_text('''package dev.vitrifi.example.checkout;
import dev.vitrifi.api.Feature;
import dev.vitrifi.api.NoEffectOnError;
import java.util.function.Supplier;
@Feature(value = "Unrelated", purpose = "Independent unknown feature")
public class Unrelated {
    @NoEffectOnError(action = "write", resource = "Carts")
    public Cart load(Supplier<Cart> supplier) { return supplier.get(); }
}
''')
        assert command("conform")["status"] == "passed"
        selected = command("verify", extra=("--", *runner, "test"))
        assert selected["status"] == "passed" and not selected["evidence_gaps"], selected
        unrelated.unlink()
        print("PASS: selected target verification does not require an unrelated feature's graph or policies", flush=True)

        restore()
        java.write_text(original.replace(forbid, "").replace("return carts.load(cartId);",
            'carts.save(cartId, 0, "bad");\n        return carts.load(cartId);'))
        regenerated = command("generate", explicit_target=False)
        assert regenerated["status"] == "passed" and target.read_text() == original_target, regenerated
        forbidden = command("conform", expected=1)
        assert forbidden["status"] == "failed", forbidden
        diagnose(forbidden, "effect", "write")
        print("PASS: a regenerated write still fails the target after its source prohibition is removed", flush=True)

        restore()
        command("generate", explicit_target=False)
        verified = command("verify", extra=("--", *runner, "test"))
        assert verified["status"] == "passed" and verified["fresh"] is True, verified
        assert verified["runtime"]["status"] == "passed", verified
        assert verified["target"]["revision"] == matching["target"]["revision"], verified
        assert len(verified["target"]["entries"]) == 2, verified
        saved = command("evidence", explicit_target=False)
        assert saved["status"] == "passed" and saved["target"] == verified["target"], saved
        target.write_text(original_target + "\n// A reviewed target revision needs fresh evidence.\n")
        stale = command("evidence", expected=1)
        assert stale["status"] == "stale" and stale["fresh"] is False, stale
        print("PASS: actual Spring/H2 evidence carries the target digest and target edits make it stale", flush=True)

        restore()
        java.write_text(original.replace("Cart cart = carts.load(cartId);",
            "Cart cart = carts.load(cartId);\n        carts.save(cart.id(), cart.total(), cart.coupon());"))
        assert command("conform")["status"] == "passed"
        rejection = command("verify", expected=1, extra=("--", *runner, "test"))
        assert rejection["status"] == "failed" and rejection["runtime"]["status"] == "failed", rejection
        assert any(v["kind"] == "no_effect_on_error" for v in rejection["runtime"]["violations"]), rejection
        print("PASS: target runtime obligations catch a write before a caught rejection", flush=True)

        restore()
        reduced_tests = original_tests
        for name in ("rejectsAnotherOwner", "rejectsInvalidCoupon", "rejectsReplacement"):
            declaration = "@Test\n    void " + name
            assert declaration in reduced_tests, name
            reduced_tests = reduced_tests.replace(declaration, "// Error path intentionally omitted\n    void " + name)
        tests.write_text(reduced_tests)
        untested = command("verify", expected=1, extra=("--", *runner, "test"))
        assert untested["status"] != "passed" and untested["target"]["revision"], untested
        assert any(p["kind"] == "no_effect_on_error" and p["status"] == "untested"
                   for p in untested["runtime"]["policy_coverage"]), untested
        print("PASS: success-only scenarios cannot satisfy a target's error-path obligation", flush=True)

        restore()
        absent = command("verify", expected=1, extra=("--", sys.executable, "-c", "pass"))
        assert absent["status"] == "incomplete" and absent["runtime"] is None, absent
        unrelated_script = """import json, os
from pathlib import Path
report = {"schema_version": 1, "session_id": os.environ["VITRIFI_SESSION_ID"],
          "complete": True, "status": "passed", "violations": [], "gaps": [],
          "scenarios": [{"name": "unrelated", "outcome": "passed"}],
          "invocations": [{"feature": "Other.Feature", "method": "other.Service.run()",
                           "scenario": "unrelated", "outcome": "returned", "events": [], "policies": []}],
          "policy_coverage": []}
Path(os.environ["VITRIFI_TRACE_FILE"]).write_text(json.dumps(report))
"""
        unrelated = command("verify", expected=1,
                            extra=("--", sys.executable, "-c", unrelated_script))
        assert unrelated["status"] == "incomplete" and unrelated["evidence_gaps"], unrelated
        print("PASS: missing traces and unrelated passing sessions cannot verify the target", flush=True)


if __name__ == "__main__":
    main()
