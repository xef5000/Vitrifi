# Architecture targets

An architecture target is an independently authored contract for a feature. It defines the public API, permitted resource effects, allowed dependency boundaries, and policies that the implementation must satisfy. Vitrifi compares it with facts obtained from Java source analysis, then checks runtime obligations against instrumented scenarios.

Keep the target beside the feature's implementation:

| File | What it contains | Who changes it |
| --- | --- | --- |
| `Checkout.Discounts.target.vitrifi` | Reviewed architectural constraints | The feature's author or reviewer |
| `Checkout.Discounts.vitrifi` | Generated signatures, calls, effects, policies, and analysis gaps | `vitrifi generate` |
| Java source and tests | Implementation and behavioral assertions | The feature's author |

Targets describe decisions that should survive implementation refactors. They do not list every helper or reproduce every call. Generation never rewrites a target, and updating the generated snapshot does not grant the implementation new permissions.

This target workflow currently supports Java/Spring. Its syntax is a separate, constrained form of Vitrifi; the earlier [language reference](language.md) describes ordinary authored `feature` modules. Targets are declarative contracts, not executable orchestration or generated Java programs.

## Read a complete target

The checked-in [checkout target](../examples/spring-checkout/src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.target.vitrifi) contains:

```vitrifi
target feature Checkout.Discounts {
  purpose "Apply one valid coupon to the owner's cart without writes on rejection";

  cover java "dev.vitrifi.example.checkout.DiscountService";
  resource Carts table "carts";

  boundary Workflow java [
    "dev.vitrifi.example.checkout.DiscountService",
    "dev.vitrifi.example.checkout.CouponRejected",
    "dev.vitrifi.example.checkout.Cart",
    "dev.vitrifi.example.checkout.Receipt"
  ];
  boundary Storage java ["dev.vitrifi.example.checkout.JdbcCartRepository"];

  operation applyCoupon(actorId: "java.lang.String", cartId: "java.lang.String", code: "java.lang.String") -> "dev.vitrifi.example.checkout.Receipt" {
    bind java "dev.vitrifi.example.checkout.DiscountService.applyCoupon(java.lang.String,java.lang.String,java.lang.String)";
    effects [read Carts, write Carts];
    dependencies [Storage];
    no_effect_on_error [write Carts];
  }

  operation preview(cartId: "java.lang.String") -> "dev.vitrifi.example.checkout.Cart" {
    bind java "dev.vitrifi.example.checkout.DiscountService.preview(java.lang.String)";
    effects [read Carts];
    dependencies [Storage];
    forbid [write Carts];
  }
}
```

Read it from the outside inward:

1. `Checkout.Discounts` is the feature identity. `purpose` explains its business intent to a reviewer; the compiler does not prove the prose.
2. `cover` selects `DiscountService` independently of its annotations. Every covered public method needs a target operation.
3. `Carts` names the SQL table whose reads and writes matter here. `Workflow` and `Storage` group source classes for dependency checks.
4. Each `operation` declares a public signature and binds it to an exact Java method. Its body states the permissions and obligations for that entry point, including its reachable implementation calls.

For `preview`, the implementation may read `Carts` and reach `Storage`. A write is prohibited. For `applyCoupon`, reads and writes are allowed, but an invocation that throws must not have attempted a write. Calls within the operation's own boundary are implicitly permitted.

The generated sibling shows how the implementation reaches those effects. Start with the target to understand the intended limits, then follow the generated calls or the Java binding when reviewing the implementation.

## Syntax and declarations

Each `.target.vitrifi` file contains exactly one `target feature Name { ... }`. Feature names may be dotted. Declarations and operation fields end with `;`; operation and feature blocks end with `}`. Lists use commas without a trailing comma. Strings use double quotes and JSON-style escapes. Both `//` and `/* ... */` comments are supported.

| Construct | What to define |
| --- | --- |
| `purpose "...";` | Required nonempty prose explaining the feature's intent. |
| `cover java "package.Class";` | At least one exact fully qualified source class whose public API the target covers. Repeat for additional classes. Globs are not accepted here. |
| `resource Name table "table";` | A resource identity for any referenced effect or policy. A simple table name or `schema.table` is supported; comparison trims and lowercases the table identity. An effect-free target can omit resources. |
| `boundary Name java ["package.Class", ...];` | Nonempty class-name patterns that group reachable source call targets. Every covered class must belong to exactly one boundary. |
| `operation name(...) -> "native.Type" { ... }` | At least one public entry contract. Every operation requires a binding, an effects list, and a dependencies list. |
| `bind java "package.Class.method(erased.Types)";` | The canonical Java method identity, including overload parameter types. Its owner must be covered by this target. |
| `effects [read Resource, write Resource];` | An upper bound on modeled JDBC effects reachable from the entry. Use `effects [];` when none are allowed. |
| `dependencies [Boundary];` | Other boundaries that reachable source calls may enter. Use `dependencies [];` to allow only the entry's own boundary. |
| `forbid [write Resource];` | An optional prohibition checked statically and through the runtime policy integration. |
| `no_effect_on_error [write Resource];` | An optional obligation requiring execution evidence for an exceptional exit without that JDBC effect attempt. |

The supported effect actions in targets are `read` and `write`. Every resource and dependency reference must resolve within the target. Resource, boundary, and operation names share one declaration namespace. Each operation field appears at most once; put several effects in its list when needed.

### Native types and bindings

Target parameters and returns use quoted Java type literals. There are no target-local `type` declarations in this implementation. Use fully qualified reference types, Java primitives, arrays, and supported concrete generic types:

```vitrifi
operation find(ids: "java.util.List<java.lang.String>") -> "java.util.List<example.Cart>" {
  bind java "example.CartService.find(java.util.List)";
  effects [read Carts];
  dependencies [Storage];
}
```

This operation belongs inside a target that covers `example.CartService` and declares `Carts` and the relevant boundaries. The signature keeps generic arguments; the binding uses Java's erased parameter types. Native type comparison distinguishes `int` from `long` and `List<String>` from `List<Integer>`. The generated view's broader architectural types, such as `Int`, do not replace this comparison.

Parameter order and native types must match. Parameter names are descriptive and are not compared with Java source names. The operation name is also a local architectural name; `bind` identifies the Java method. `"void"` is accepted for a return type. Wildcard generic arguments are supported, but unbound type variables and type-use annotations are outside the current target type syntax.

### Coverage and dependency boundaries

Coverage closes the selected public API: adding a public method without a corresponding target operation fails conformance. Removing a selected class or method also fails. The analyzer inventories these classes even if `@Feature` disappears. Public static methods are included in API coverage, although the Spring runtime integration cannot provide arbitrary static-entry interception. Constructors are not public entry operations. Nested classes need separate `cover` declarations.

Boundary patterns match fully qualified source class names, for example:

```vitrifi
boundary Storage java ["example.storage.*"];
```

These are class-name globs, not directory paths. `*` can match dots, so this pattern also matches deeper package names. Use exact class names for tighter control. Overlapping boundaries are rejected when a source class matches more than one. Every source class reached through the analyzed call graph must map to exactly one boundary.

Dependency permissions apply transitively. If `preview` reaches a storage method that calls an operation in a third boundary, that third boundary also needs permission from `preview`. Extracting a private helper into an already allowed boundary does not require another public operation or a new permission.

These checks constrain source call targets. They do not establish isolation of field access, type references, implemented interfaces, or object representations. A boundary name is not a Java visibility modifier.

### Keep source metadata connected

The current runtime adapter still uses source annotations, so conformance requires them to agree with the target:

- Targeted entries must retain a matching `@Feature` on their class or package.
- `resource Carts table "carts";` requires a corresponding `@ResourceTable(value = "Carts", table = "carts")` source mapping.
- `forbid [write Carts];` requires `@Forbid(action = "write", resource = "Carts")` on the entry.
- `no_effect_on_error [write Carts];` requires `@NoEffectOnError(action = "write", resource = "Carts")` on the entry.

Deleting a policy annotation therefore fails conformance instead of deleting the target obligation. Renaming the friendly resource to `Other` while retaining table `carts` also fails. The target's purpose is reviewer-facing prose; it is not a behavioral proof or an exact-text assertion against the annotation's purpose.

## Author, check, and verify

First follow the [Java/Spring setup](java-spring.md#build-and-generate) to build the CLI and analyzer and prepare the application's classpath. From this repository's root, select the checkout target explicitly:

```sh
discount_target=src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.target.vitrifi

./target/debug/vitrifi lint --root examples/spring-checkout \
  --target "$discount_target"

./target/debug/vitrifi conform --root examples/spring-checkout \
  --target "$discount_target"

./target/debug/vitrifi generate --root examples/spring-checkout

./target/debug/vitrifi check --root examples/spring-checkout \
  --target "$discount_target"

./target/debug/vitrifi verify --root examples/spring-checkout \
  --target "$discount_target" -- mvn test

./target/debug/vitrifi evidence --root examples/spring-checkout \
  --target "$discount_target"
```

| Command | What a successful result establishes |
| --- | --- |
| `lint --target` | Target syntax and references are valid. It needs no Java implementation or analyzer, so it can run before code exists. |
| `conform --target` | Fresh source analysis satisfies the selected targets' static constraints. It does not require a current generated snapshot or run tests. |
| `generate` | Implementation views are regenerated. Target conformance is not checked and target files are not rewritten. |
| `check --target` | Static conformance passes and generated snapshots match current analysis. |
| `verify --target -- COMMAND...` | Conformance passes, the explicit command succeeds, and fresh runtime evidence meets the required coverage and policies. The command runs from `--root`; conformance failure prevents execution. Snapshot freshness is not required. |
| `evidence --target` | Saved evidence remains valid for the selected targets and current inputs. It does not rerun the tests. |
| `inspect --target` | Shows targets, fresh analysis, and conformance together for diagnosis. |
| `compile --target` | Emits the machine-readable analysis artifact after successful conformance. |

Paths passed to `--target` are relative to `--root` unless absolute, and must remain inside that root. Target commands select Java analysis without requiring `--java`; analyzer, classpath, and source-root options from the Java guide still apply.

`effects` and `dependencies` are permissions, not execution requirements. A permitted write need not occur. The static model does not prove write count, call order, branch conditions, authorization logic, or arithmetic results. For example, changing a discount from 1,000 cents to 100 cents may preserve every architectural fact while breaking a business assertion in the tests.

Static conformance reports `failed` for known violations and `incomplete` for required analysis it cannot establish, such as unresolved reachable dispatch. Both return a nonzero exit status. `passed` describes the static constraints; a `no_effect_on_error` policy remains `runtime_required` until verified by scenarios.

### Runtime obligations

Tests must use the existing [Spring/JDBC evidence integration](java-spring.md#verify-behavior-against-the-real-implementation). Target-aware verification requires a fresh completed session with passing scenarios, no violations or instrumentation gaps, and:

- A recorded invocation of **every selected target entry**, even an entry with no policies.
- Matching policy coverage and invocation evidence for every target policy.
- An invocation that **throws** for each `no_effect_on_error` obligation, with that policy recorded as passed.

A success-only test suite cannot satisfy the error-path obligation. A test may catch the expected exception; Vitrifi records the feature invocation's exceptional exit. A successful shell command, missing trace, or passing trace for an unrelated feature cannot substitute for the selected entries' evidence.

Verification assumes the supplied command builds and runs the selected application's current implementation. Matching traces and source fingerprints do not prove which bytecode a deliberately misconfigured runner executed; a runner that reuses stale classes can violate that assumption. Use a build/test command that compiles the current source and keep that command under normal review.

`no_effect_on_error [write Carts]` means no matching JDBC execution attempt on an exceptional exit, including attempts that fail or are later rolled back. It does not undo writes, prove a final committed database state, or apply to errors returned as ordinary values. A passing trace establishes this result for the captured scenarios, not every possible path.

## CI selection and PR review

Without explicit target paths, `lint` and `conform` discover `.target.vitrifi` files beneath `--root`. `check`, `inspect`, `compile`, `verify`, and `evidence` also select the target workflow when such files are discovered.

Use explicit paths in CI so deleting a target cannot silently remove it from discovery. Repeat `--target` for several feature-local contracts:

```sh
vitrifi check --root /path/to/application \
  --target src/main/java/example/checkout/Checkout.target.vitrifi \
  --target src/main/java/example/billing/Billing.target.vitrifi
```

Explicit selection checks those targets. Targets currently have no imports; each declares its own referenced resources and boundaries. Selected targets must have distinct feature names, nonoverlapping class coverage and method bindings, and consistent resource-to-table identities. Use each independently built application's module as `--root`, with its own classpath.

Evidence records the selected target files and a content digest alongside implementation inputs. Editing a target, including its prose or comments, changes that digest and makes earlier target evidence stale. Rerun verification after an accepted target change.

Review reports authored target changes separately from implementation facts:

```sh
./target/debug/vitrifi review --java --root examples/spring-checkout \
  --base origin/main --json
```

The `target_changes` entries identify added, modified, and removed target files with before/after hashes and `authored_target` provenance. Review the source diff for those files: conformance checks the currently selected contract, but does not approve widening its permissions. Approval and protection of the accepted target remain responsibilities of the repository's review and CI process.

## Current scope

The target adapter checks declared public native signatures, resource mappings, supported transitive JDBC effect bounds, and source call-target boundaries. Inherited public APIs beyond `java.lang.Object`, nonconcrete covered types, synthesized public methods, native methods, and declared class or method type parameters produce explicit coverage gaps. Concrete generic types in signatures are supported, as shown above. Signature checking does not establish equivalence of `throws` clauses, modifiers, or nullability.

Analysis and runtime checks inherit the [Java integration's limits](java-spring.md#scope-of-the-current-guarantee): bounded Spring resolution and SQL support, synchronous feature proxy entry, and instrumented JDBC execution. Unknown behavior reachable from a targeted entry prevents a passing conformance result. Reflection, asynchronous work, uninstrumented external clients, and other behavior outside the observation boundary need additional analysis or adapters.

Targets make architectural choices independently checkable. Ordinary implementation review and behavioral tests still establish the business rules those choices alone cannot express.
