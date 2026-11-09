#!/usr/bin/env python3
"""Real compiler, Spring/JDBC, mutation and differential tests for the workflow pilot.

Run after cargo build and mvn install. Mutations only touch a temporary checkout.
The reference interpreter compares ordered action traces, outcomes and values;
this is regression testing, not an algebraic equivalence certificate.
"""
import argparse
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

PACKAGE = Path("src/main/java/dev/vitrifi/example/checkout")
WORKFLOW = "Checkout.Discounts.workflow.vitrifi"


def reference(operation, owner, valid, used, fail):
    """Small-step oracle over the parsed AST; leaves have controlled test meanings."""
    trace = []
    cart = {"id": "cart-1", "ownerId": "alice", "total": 2500,
            "coupon": "SAVE10" if used else None}
    env = {"actorId": "alice" if owner else "bob", "cartId": "cart-1",
           "code": "SAVE10" if valid else "INVALID"}

    class Exit(Exception):
        def __init__(self, result):
            self.result = result

    def value(v, scope):
        return scope[v["value"]] if v["kind"] == "ref" else v["value"]

    def block(statements, scope):
        for statement in statements:
            kind = statement["kind"]
            if kind in ("let", "do"):
                call = statement["call"]
                args = [value(v, scope) for v in call["arguments"]]
                action = call["action"]
                trace.append(action)
                if action == fail:
                    raise Exit({"kind": "failed", "action": action})
                if action == "load": result = cart.copy()
                elif action == "owns": result = args[0] == args[1]["ownerId"]
                elif action == "validCode": result = args[0] == "SAVE10"
                elif action == "available": result = args[0]["coupon"] is None
                elif action == "save":
                    result = {"cartId": args[0]["id"], "coupon": args[1],
                              "total": max(0, args[0]["total"] - 1000)}
                else: raise AssertionError(action)
                if kind == "let": scope[statement["name"]] = result
            elif kind == "if":
                block(statement["yes"] if value(statement["condition"], scope)
                      else statement["no"], scope.copy())
            elif kind == "return":
                raise Exit({"kind": "returned", "value": value(statement["value"], scope)})
            elif kind == "reject":
                raise Exit({"kind": "rejected", "message": statement["message"]})
            else: raise AssertionError(kind)
    try:
        block(operation["body"], env)
        raise AssertionError("unterminated workflow")
    except Exit as result:
        return dict(result.result, trace=trace)


def differential(app, ast, directory, run):
    """Compile the emitted service against controlled native leaves, then compare."""
    source = directory / "differential/dev/vitrifi/example/checkout"
    source.mkdir(parents=True)
    for name in ("DiscountService.java", "Cart.java", "Receipt.java"):
        shutil.copy2(app / PACKAGE / name, source / name)
    (source / "CheckoutActions.java").write_text('''package dev.vitrifi.example.checkout;
import java.util.*;
public class CheckoutActions {
    public final List<String> trace = new ArrayList<>();
    private final boolean used; private final String fail;
    public CheckoutActions(boolean used, String fail) { this.used = used; this.fail = fail; }
    private void step(String action) {
        trace.add(action);
        if (action.equals(fail)) throw new IllegalStateException("controlled failure");
    }
    public Cart load(String id) { step("load"); return new Cart(id, "alice", 2500, used ? "SAVE10" : null); }
    public boolean owns(String actor, Cart cart) { step("owns"); return actor.equals(cart.ownerId()); }
    public boolean validCode(String code) { step("validCode"); return "SAVE10".equals(code); }
    public boolean available(Cart cart) { step("available"); return cart.coupon() == null; }
    public Receipt save(Cart cart, String code) { step("save"); return new Receipt(cart.id(), code, Math.max(0, cart.total() - 1000)); }
}
''')
    (source / "Harness.java").write_text('''package dev.vitrifi.example.checkout;
import java.util.*;
import com.fasterxml.jackson.databind.ObjectMapper;
public class Harness {
    public static void main(String[] args) throws Exception {
        var json = new ObjectMapper();
        for (boolean owner : new boolean[]{false, true})
        for (boolean valid : new boolean[]{false, true})
        for (boolean used : new boolean[]{false, true})
        for (String fail : new String[]{"", "load", "owns", "validCode", "available", "save"}) {
            var actions = new CheckoutActions(used, fail);
            var service = new DiscountService(actions);
            var result = new LinkedHashMap<String, Object>();
            try {
                var value = service.applyCoupon(owner ? "alice" : "bob", "cart-1", valid ? "SAVE10" : "INVALID");
                result.put("kind", "returned"); result.put("value", value);
            } catch (dev.vitrifi.workflow.Rejected rejected) {
                result.put("kind", "rejected"); result.put("message", rejected.getMessage());
            } catch (dev.vitrifi.workflow.Failed failed) {
                result.put("kind", "failed"); result.put("action", failed.action());
            }
            result.put("trace", actions.trace);
            System.out.println(json.writeValueAsString(result));
        }
    }
}
''')
    classpath = (app / "target/vitrifi-classpath.txt").read_text().strip()
    java_home = Path(os.environ["JAVA_HOME"]) / "bin" if "JAVA_HOME" in os.environ else None
    javac, java = (str(java_home / "javac"), str(java_home / "java")) if java_home else ("javac", "java")
    run([javac, "-proc:none", "-cp", classpath, *map(str, source.glob("*.java"))])
    result = run([java, "-cp", str(directory / "differential") + os.pathsep + classpath,
                  "dev.vitrifi.example.checkout.Harness"])
    actual = [json.loads(line) for line in result.stdout.splitlines()]
    operation = next(o for o in ast["operations"] if o["name"] == "applyCoupon")
    expected = [reference(operation, *case) for case in itertools.product(
        (False, True), (False, True), (False, True), ("", "load", "owns", "validCode", "available", "save"))]
    assert actual == expected, (actual, expected)
    print("PASS: 48 generated-Java executions match reference values, exits and ordered action traces", flush=True)


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vitrifi", type=Path, default=repo / "target/debug/vitrifi")
    parser.add_argument("--analyzer", type=Path, default=repo / "integrations/java/target/vitrifi-java-0.3.0.jar")
    parser.add_argument("--maven", default="mvn")
    parser.add_argument("--maven-repo", type=Path)
    args = parser.parse_args()
    binary, analyzer = args.vitrifi.resolve(), args.analyzer.resolve()
    maven = str(Path(args.maven).resolve()) if os.sep in args.maven else args.maven
    runner = [maven, "-o", "-B", "-ntp"]
    if args.maven_repo: runner += ["-Dmaven.repo.local=" + str(args.maven_repo.resolve())]

    def run(command, cwd=None):
        result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=180)
        assert result.returncode == 0, result.stdout + result.stderr
        return result

    with tempfile.TemporaryDirectory(prefix="vitrifi-compiled-workflow-") as temporary:
        root = Path(temporary).resolve()
        shutil.copy2(repo / "pom.xml", root / "pom.xml")
        app = root / "examples/workflow-checkout"
        shutil.copytree(repo / "examples/workflow-checkout", app,
                        ignore=shutil.ignore_patterns("target", ".vitrifi"))
        workflow, java, actions, target = [app / PACKAGE / name for name in
            (WORKFLOW, "DiscountService.java", "CheckoutActions.java", "Checkout.Discounts.target.vitrifi")]
        originals = {p: p.read_text() for p in (workflow, java, actions, target)}

        def command(*arguments, error=None):
            result = subprocess.run([str(binary), *arguments, "--root", str(app),
                "--java-analyzer", str(analyzer), "--json"], text=True, capture_output=True, timeout=180)
            value = json.loads(result.stdout)
            if error:
                assert result.returncode != 0 and error.lower() in json.dumps(value).lower(), (arguments, value)
            else:
                assert result.returncode == 0, (arguments, value, result.stderr)
            return value

        def restore():
            for path, source in originals.items(): path.write_text(source)

        java.unlink()
        command("workflow", "build", "--check", error="do not accept --check")
        assert not java.exists()
        lint = command("workflow", "lint")
        ast = lint["features"][0]
        run([*runner, "generate-resources"], app)
        command("workflow", "build")
        assert java.read_text() == originals[java]
        command("workflow", "check")
        print("PASS: lint without Java, bootstrap generation, leaf checks and target conformance", flush=True)
        differential(app, ast, root, run)

        source = originals[workflow]
        workflow.write_text(source.replace("let owner =", "let early = call save(cart, code);\n    let owner ="))
        command("workflow", "build", error="WORKFLOW_REJECTION_EFFECT")
        assert java.read_text() == originals[java]
        restore()
        print("PASS: write-before-rejection fails before generated output is changed", flush=True)

        actions.write_text(originals[actions].replace('return "SAVE10".equals(code);',
            'carts.save("cart-1", 0, code); return "SAVE10".equals(code);'))
        command("workflow", "build", error="WORKFLOW_LEAF_EFFECT")
        assert java.read_text() == originals[java]
        restore()
        print("PASS: hidden native leaf writes cannot hide behind effects []", flush=True)

        actions.write_text(originals[actions].replace("public boolean validCode", "public int validCode")
                          .replace('return "SAVE10".equals(code);', 'return 1;'))
        command("workflow", "build", error="JAVA_COMPILE")
        restore()
        actions.write_text(originals[actions].replace("return carts.load(cartId);",
            "java.util.function.Supplier<Cart> fetch = () -> carts.load(cartId); return fetch.get();"))
        command("workflow", "build", error="WORKFLOW_ANALYSIS_GAP")
        restore()
        print("PASS: native signature drift and unsupported dispatch fail closed", flush=True)
        ambiguous = app / PACKAGE / "OtherActions.java"
        ambiguous.write_text("""package dev.vitrifi.example.checkout;
            @org.springframework.stereotype.Component
            public class OtherActions extends CheckoutActions {
                public OtherActions(CartRepository carts) { super(carts); }
                @Override public boolean validCode(String code) { return true; }
            }
            """)
        target.unlink()
        command("workflow", "build", error="WORKFLOW_DISPATCH")
        ambiguous.unlink()
        restore()
        print("PASS: generated Spring dispatch is checked even without an independent target", flush=True)
        shadow = app / "src/main/java/dev/vitrifi/workflow/Failed.java"
        shadow.parent.mkdir(parents=True)
        shadow.write_text("""package dev.vitrifi.workflow;
            public final class Failed extends RuntimeException {
                public Failed(String action, Throwable cause) { super(action); throw new Rejected(action); }
            }
            """)
        command("workflow", "build", error="WORKFLOW_RUNTIME_SHADOW")
        shadow.unlink()
        print("PASS: source cannot replace workflow failure semantics with a rejecting constructor", flush=True)



        actions.write_text(originals[actions].replace('return "SAVE10".equals(code);', 'return valid(code);')
            .replace("    public boolean available", '    private boolean valid(String code) { return "SAVE10".equals(code); }\n    public boolean available'))
        command("workflow", "build")
        assert java.read_text() == originals[java]
        restore()
        print("PASS: extracting a native helper preserves the workflow without regeneration changes", flush=True)

        java.write_text(originals[java] + "// manual change\n")
        command("workflow", "check", error="WORKFLOW_STALE")
        command("check", "--java", error="WORKFLOW_STALE")
        command("verify", "--java", error="WORKFLOW_STALE")
        command("workflow", "build")
        java.write_text("// authored class\n")
        command("workflow", "build", error="Refusing to overwrite")
        assert java.read_text() == "// authored class\n"
        restore()
        workflow.unlink()
        command("check", "--java", error="WORKFLOW_ORPHAN")
        restore()
        print("PASS: stale/tampered outputs, authored overwrites and orphan generated classes are rejected", flush=True)

        outside = root / "outside.java"
        outside.write_text("do not overwrite")
        java.unlink()
        java.symlink_to(outside)
        command("workflow", "build", error="Symlinked")
        assert outside.read_text() == "do not overwrite"
        java.unlink()
        restore()
        target.write_text(originals[target].replace("dependencies [Actions, Storage]", "dependencies [Storage]"))
        command("workflow", "build", error="TARGET_DEPENDENCY")
        restore()
        print("PASS: output symlinks and independent target boundary violations are rejected", flush=True)

        # Ordinary generated architecture views still agree with the emitted service.
        command("generate")
        command("check", "--java")
        verified = run([str(binary), "workflow", "verify", "--root", str(app),
            "--java-analyzer", str(analyzer), "--json", "--", *runner, "test"])
        result = json.loads(verified.stdout)
        assert result["status"] == "passed", result
        invocations = result["runtime"]["invocations"]
        rejections = [i for i in invocations if i["error_type"] == "dev.vitrifi.workflow.Rejected"]
        assert len(rejections) == 3, rejections
        assert all(not any(e["action"] == "write" for e in i["events"]) for i in rejections)
        failures = [i for i in invocations if i["error_type"] == "dev.vitrifi.workflow.Failed"]
        assert failures and any(e["action"] == "write" for e in failures[0]["events"])
        accepted = [i for i in invocations if ".applyCoupon(" in i["method"] and i["outcome"] == "returned"]
        assert accepted and all([e["action"] for e in i["events"]] == ["read", "write"] for i in accepted)
        print("PASS: real JDBC traces distinguish rejection, acceptance and infrastructure failure", flush=True)

        workflow.write_text(originals[workflow] + "// changed reviewed input\n")
        command("evidence", "--java", error="stale")
        restore()
        command("evidence", "--java")
        actions.write_text(originals[actions].replace("- 1000", "- 900"))
        command("evidence", "--java", error="stale")
        print("PASS: workflow and native leaf changes invalidate previously passing evidence", flush=True)


if __name__ == "__main__":
    main()
