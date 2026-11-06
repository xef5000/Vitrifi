# Compiled workflows: the Java/Spring pilot

A `.workflow.vitrifi` file is executable orchestration. The compiler emits a Spring service from it. Edit the workflow to change sequencing, branches and rejection paths; edit Java actions to change calculations or persistence. There is one authored orchestration, and the generated Java is committed for normal builds and PR review.

This pilot implements `vitrifi.workflow/1`. It is deliberately small: synchronous calls, single-assignment bindings, boolean branches, return and explicit business rejection. It does not produce algebraic proof certificates.

## Which file should I read?

| File | Author | Meaning |
| --- | --- | --- |
| `Checkout.Discounts.workflow.vitrifi` | Developer | Executable ordering, branches, typed Java action bindings and effect bounds |
| `CheckoutActions.java` | Developer | Native leaf calculations and persistence; checked against the action declarations |
| `DiscountService.java` | Compiler | Spring service emitted from the workflow; never hand-edit it |
| `Checkout.Discounts.target.vitrifi` | Architect/developer | Independent public interface, dependency boundaries and policy obligations |
| `Checkout.Discounts.vitrifi` | Java analyzer | Implementation-derived architectural view, including the generated service and its native callees |

Start with the workflow's `operation`, follow each `call` to its `action`, and follow the `bind` into Java when reviewing a calculation. Compare the independent target if the change expands permissions. The generated view is useful for inspecting transitive dependencies, rather than just direct calls.

The [complete checkout workflow](../examples/workflow-checkout/src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.workflow.vitrifi) and [Java leaves](../examples/workflow-checkout/src/main/java/dev/vitrifi/example/checkout/CheckoutActions.java) demonstrate this split. The earlier [Spring example](../examples/spring-checkout) retains ordinary authored Java orchestration for comparison.

## Commands

From the repository root, with Rust, JDK 21+ and Maven:

```sh
cargo build
mvn install
mvn -f examples/workflow-checkout/pom.xml generate-resources

./target/debug/vitrifi workflow lint --root examples/workflow-checkout
./target/debug/vitrifi workflow build --root examples/workflow-checkout
./target/debug/vitrifi workflow check --root examples/workflow-checkout
./target/debug/vitrifi workflow verify --root examples/workflow-checkout -- mvn test

./target/debug/vitrifi generate --root examples/workflow-checkout
./target/debug/vitrifi check --java --root examples/workflow-checkout
```

`lint` parses and checks the closed workflow language without Java. `build` renders a temporary source overlay, runs javac analysis on it and the real Java leaves, and checks discovered or explicitly selected targets. It writes generated services only after these checks pass. This also works when the generated service does not exist yet; application code and annotation processors are not executed during compilation.

`check` checks freshness, Java leaves and target conformance without writing. `verify` does those checks, then runs the explicitly supplied instrumented scenario command. It records hashes of workflow files, Java sources, target files and analyzer/dependency inputs, plus the discovered workflow selection. Changed inputs invalidate the evidence. The ordinary Java `check`, `conform`, `generate` and `verify` paths also reject stale workflow services and invalid leaves. `evidence --java` inspects recorded evidence.

Commands discover feature-local `*.workflow.vitrifi` files below `--root`; there is no manifest. An optional file after the subcommand selects an explicit workflow. Paths are relative to `--root`. Use an explicit workflow and `--target` in CI so removing a required file fails the check:

```sh
./target/debug/vitrifi workflow check \
  src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.workflow.vitrifi \
  --root examples/workflow-checkout \
  --target src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.target.vitrifi
```

Java toolchain selection uses the existing `--java-analyzer`, `--classpath-file`, `--source-root`, `JAVA_HOME` and `VITRIFI_JAVA` options. By default emitted classes live under `src/main/java`, following their fully qualified name. Generation refuses authored files, files owned by another workflow, path traversal and symlink outputs. Renaming/removing a workflow does not automatically delete its old service: checks report the orphan for explicit cleanup.

## Declarations

```vitrifi
workflow feature Checkout.Discounts {
  purpose "Apply a coupon without changing a rejected cart";
  emit java "dev.vitrifi.example.checkout.DiscountService";
  actions java "dev.vitrifi.example.checkout.CheckoutActions";
  resource Carts table "carts";

  action load(cartId: "java.lang.String") -> "dev.vitrifi.example.checkout.Cart" {
    bind java "dev.vitrifi.example.checkout.CheckoutActions.load(java.lang.String)";
    effects [read Carts];
  }

  operation preview(cartId: "java.lang.String") -> "dev.vitrifi.example.checkout.Cart" {
    effects [read Carts];
    let cart = call load(cartId);
    return cart;
  }
}
```

- `workflow feature` identifies one feature per file. `purpose` is required human explanation; it is not an executable requirement.
- `emit java` names the generated public Spring service. `actions java` names one source-visible concrete action class injected into that service. Different workflows can use different classes, including across packages.
- `resource` maps a friendly identifier to a SQL table. Source `@ResourceTable` mappings must agree. Ambiguous qualified/unqualified aliases are rejected.
- `action` declares a native leaf. Parameters and results use exact Java type strings, including concrete generic types. Its `bind` uses the fully qualified method with **erased** parameter types, as in architecture targets. The action must resolve to one public source method on the declared actions class.
- `effects` is an upper bound on recognized transitive `read`/`write` effects. A leaf's effects must fit every operation that calls it. An empty list means no modeled JDBC effect is reachable, conditional on this adapter's coverage. It is not a Java purity assertion.
- `operation` generates a public service method. Its input/output types must agree with calls and returned values. Every path must return or reject. Operations cannot return `void` in this pilot.
- `no_effect_on_rejection [write Carts];` is optional and comes directly after operation `effects`. The compiler rejects any explicit rejection path that could follow that effect. It also emits a runtime annotation so instrumented scenarios check actual JDBC attempts.

Feature header fields and declarations can be interleaved. An action body has `bind` followed by `effects`. An operation body has `effects`, optionally `no_effect_on_rejection`, then statements. `//` and `/* ... */` comments are accepted. Identifiers are case-sensitive; generated Java identifiers cannot use Java reserved words. Resource and operation names are local to the workflow.

## Statements and values

| Syntax | Execution |
| --- | --- |
| `let cart = call load(cartId);` | Invoke once and bind the successful result |
| `do call audit(cart);` | Invoke a `void` action once |
| `if owner { ... } else { ... }` | Execute exactly one branch; both blocks are required |
| `return receipt;` | Return the bound value immediately |
| `reject "Coupon is invalid";` | End immediately with business rejection |

Values are previously bound variables, strings and booleans (`true`, `false`). A branch condition must have native `boolean` type. Calls accept only these values, so there are no nested calls or hidden evaluation effects in arguments. A binding cannot be reassigned or shadow an existing binding. Branch-local bindings do not escape their block. Single assignment does **not** make an underlying Java object immutable.

Calculations, record construction and projection belong in the Java leaves for now. The workflow has no operators, numeric literals, null literal, loops, recursion, callbacks, imports or user-defined combining forms. A missing `else`, wrong argument type, unknown action, unreachable statement or unterminated path is a compilation error. Depth and input size are bounded.

## Order, failures and rejection

The execution state consists of the current statement, value bindings and an ordered action trace. A sequence executes in source order. Each reached call happens exactly once; an unselected branch performs no calls. `return`, `reject` and action failure stop all remaining statements. No optimization reorders, duplicates or removes calls.

An operation has three observable outcomes:

| Outcome | Java behavior | Policy consequence |
| --- | --- | --- |
| `Success(value)` | Returns its declared value | Does not exercise a rejection-path policy |
| `Rejected(message)` | Throws final `dev.vitrifi.workflow.Rejected` | Exercises `NoEffectOnRejection` |
| `Failed(action, cause)` | Wraps a leaf's `Exception` in final `dev.vitrifi.workflow.Failed` | Does not exercise a rejection-path policy |

A leaf that itself throws `Rejected` is wrapped in `Failed`; only an explicit workflow `reject` creates a business rejection at the workflow boundary. Java `Error` propagates as a fatal host failure, never a business rejection. Application sources cannot replace these reserved runtime outcome classes. This distinction does not roll back writes. A write attempt can occur before an infrastructure failure. Native leaves may also diverge; the compiler does not prove their termination.

The new repeatable `@NoEffectOnRejection(action="write", resource="Carts")` applies only to an invocation that throws the exact final rejection type. Runtime reports retain `outcome: "threw"` and use `error_type` to distinguish it. Success-only or infrastructure-failure-only scenarios leave that policy **untested**. The existing `@NoEffectOnError` remains broader: it applies to every exceptional exit, including business rejection and infrastructure failure. Targets accept `no_effect_on_rejection` as an independent obligation and require its source annotation.

For rejection-path analysis, an action contributes its declared effect bound. Both sides of an `if` are considered possible. Only continuing paths rejoin; effects on a branch ending in `return` do not contaminate another branch's later rejection. A write on a continuing branch followed by a later rejection fails. The analysis is conservative: it does not prove correlations between boolean tests or remove branches for literal conditions.

## Try a mistake

In the checkout operation, move `let receipt = call save(cart, code);` above the owner check. `workflow build` reports `WORKFLOW_REJECTION_EFFECT` and leaves the previous generated service untouched. Merely moving a write into `validCode` also fails: its `effects []` is compared to the Java analyzer's transitive effects, not trusted as an annotation.

Changing the discount from 1000 to 900 in Java keeps the same permitted effect graph. That change requires business tests and review; architectural checks cannot tell which discount is correct. Unsupported dispatch, such as a `Supplier` hiding a repository call, blocks a complete check instead of earning a clean result.

## What this establishes

The closed workflow gives complete control-flow enumeration at the orchestration layer. Its guarantees are conditional on the Java adapter and modeled resource boundaries. Native heap behavior, reflection, asynchronous work, arbitrary framework advice, transactions and external systems are outside this pilot's supported semantics. Unknown reachable calls prevent a complete leaf check. The current boundary model still restricts source call targets, not all type or representation dependencies.

The [mutation and differential suite](../scripts/test-compiled-workflow.py) compares a reference AST interpreter to generated Java over 48 combinations of input branches and action failures. It checks outcomes, returned values, order and multiplicity, in addition to real Spring/JDBC scenarios. These tests validate the implementation; they do not prove general host-code equivalence or justify algebraic rewrites.

A future proof step needs a versioned semantics, an explicitly justified set of rewrite rules and an independent certificate checker that pins leaf implementations and dependencies. This pilot supplies a small executable language on which to evaluate that idea. Equal effect sets alone remain insufficient for equivalence.
