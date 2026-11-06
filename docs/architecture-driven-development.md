# Exploring architecture-driven development and FP/FL

Exploration of [issue #5](https://github.com/xef5000/Vitrifi/issues/5), initially against Vitrifi 0.3 at `55a6a1b`, on 2026-11-06. The authored-target conformance slice is now implemented; see [Architecture targets](targets.md) for its actual syntax, commands, and limits. Executable workflows and algebraic rewriting below remain research proposals.

## Recommendation

The implemented **authored-target conformance check** builds on the existing Java analysis. Generated views remain implementation evidence. A small independent contract constrains public interfaces, permitted effects, dependency boundaries, and verification obligations.

Then experiment with an **executable, restricted workflow layer**. If the feature file actually executes or compiles the orchestration, that layer has one implementation. If Java still independently implements the same workflow, the synchronization problem remains.

Treat certified algebraic rewrites as a third experiment. They need a defined semantics and justified rules; equality of call graphs or effect sets is insufficient.

## Starting point before the conformance slice

The baseline repository already had much of the machinery for conformance. The last column records the gaps addressed by this implementation:

| Existing component | Reusable capability | Missing for ADD |
| --- | --- | --- |
| [`SourceAnalysis`](../src/derived_model.rs) | Native signatures, resolved/unknown calls, modeled effects | Independent resource identities and target coverage |
| [`derived::build`](../src/derived.rs) | Transitive effect/unknown propagation and policy diagnostics | Compare facts with an authored target |
| [`java::analyze`](../src/java.rs) | Javac attribution and source/dependency fingerprints | Include the target and mappings in the checked inputs |
| [`java::verify`](../src/java.rs) | Scenario-linked Spring/JDBC evidence | Require obligations from the target, even if an annotation disappears |
| [`compiler::compile`](../src/compiler.rs) | Existing DSL parsing and reference checking | Separate target linting from implementation binding checks |

The current source model contains method calls, not a complete control-flow or data-flow model. Its effect propagation uses sets. It cannot establish authorization order, exact write count, arithmetic correctness, or equivalence of arbitrary host implementations.

### A concrete experiment

In an isolated temporary copy of the Spring checkout, I changed only:

```diff
- int total = Math.max(0, cart.total() - 1000);
+ int total = Math.max(0, cart.total() - 100);
```

The real analyzer and test runner produced:

| Observation | Result |
| --- | --- |
| Source revision fingerprint | Changed |
| Derived program, including calls and effects | Identical |
| Static analysis | Passed |
| `generate --check` | Passed; generated architecture was unchanged |
| `verify --java -- … mvn test` | Failed |
| Failing scenario | `owner applies a valid coupon` |

This is an appropriate division of responsibility. A source change can preserve architecture and still break the business rule. The existing test checks the resulting total. ADD should preserve that independent behavioral check; an architecture match cannot replace it. A source fingerprint changing also invalidates old execution evidence even when the architectural view stays the same.

## The contract should constrain the implementation

Comparing the rendered target and generated file byte-for-byte would impose implementation details that the architect may not care about. Generated files contain helper operations, source locations, and presentation names. The renderer also maps several Java types, including `int` and `long`, to `Int`. Conversely, [`derived::facts`](../src/derived.rs) retains internal details useful for review that need not be contractual.

Use a typed conformance model instead. A target for the checkout could express these choices:

| Target element | Example | Meaning |
| --- | --- | --- |
| Coverage | Declared public methods of `DiscountService` | Implementation cannot escape checking by removing `@Feature` |
| Resource identity | `Carts` maps to SQL table `carts` | Renaming an annotation cannot disguise the same database resource |
| Public entry | `preview(java.lang.String) → …Cart` | Match the actual native signature and overload |
| Allowed effects | `preview`: `read Carts` | A reachable modeled write is a violation |
| Dependency boundary | Workflow may call operations in the Storage boundary | Check permitted cross-boundary call targets while allowing local helper extraction |
| Error-path obligation | `applyCoupon`: no `write Carts` attempt on exceptional exit | Requires matching runtime evidence in this adapter |

This table describes the model; the [target guide](targets.md) provides executable CLI examples and valid target syntax.

The core relation is:

```text
observed modeled effects ⊆ permitted effects
observed cross-boundary calls ⊆ permitted cross-boundary calls
declared public entries = covered implementation public entries
native signatures match
required analysis has no unresolved reachable behavior
behavioral obligations have the required evidence
```

The first two are bounds, not requirements to perform every permitted effect. Removing an unnecessary write or extracting a helper should not require an architecture amendment. A call's presence in a may-call graph does not prove that it runs on every path. “Must authorize before saving” needs control-flow reasoning, a restricted executable workflow, or execution evidence.

Use separate results for a proven modeled violation, incomplete analysis, stale evidence, and an unexercised runtime obligation. All can block acceptance without pretending they are the same finding. A successful check remains scoped to the adapter's supported model and observation boundaries.

### Artifacts and authority

The pilot keeps the generated `Checkout.Discounts.vitrifi` and adds a feature-local `Checkout.Discounts.target.vitrifi`. The target records design choices, not every method or call. The generated view is disposable output. Both remain ordinary PR files; no global manifest or review server is required.

`vitrifi conform --target …` loads the authored constraints and analyzes Java source facts. Target-aware `check`, `inspect`, `compile`, `verify`, and `evidence` discover targets before generated-marker routing, and CI can make particular targets mandatory with explicit paths. A later presentation can combine intent, facts, and status into one review view without combining their authorities.

The authoring path must lint a target before its implementation exists. Missing bindings are then a conformance failure, not a target syntax error.

The target must also survive attempts to weaken the implementation's metadata:

- Removing `@Forbid` must not remove the target's prohibition. Target constraints now supply independent static policies and expected runtime coverage; matching source annotations are required for the initial runtime integration.
- Removing `@Feature` must not remove coverage of a class selected by the target.
- Changing `@ResourceTable(value = "Carts", table = "carts")` to a different friendly name must not disguise the same table. The Java source schema now exports resource mapping facts for this comparison.
- Missing, ambiguous, and extra public entry bindings must be reported. Keep native parameter order and type identity; matching rendered `Int` is insufficient.

For the first runtime integration, required source annotations can be checked against target obligations, with a missing annotation causing failure. A target-derived runtime policy manifest would remove that duplication later. Either way, evidence must identify the target digest, implementation inputs, adapter, and runtime policy configuration; a modified target cannot reuse earlier evidence.

The implementer should be able to propose a target change with rationale. It must appear as a separate policy diff against an approved baseline. The checker can detect a changed contract and pin evidence to it; authorization to approve that change belongs to the surrounding human/CI workflow. A generator must never widen the approved target to make code pass.

### Implemented slice

The implementation adds a pure `conform(TargetFeature, DerivedProgram)` layer, precise target bindings/mappings, target-only linting, explicit CLI routing, and target-aware evidence validation. Boundaries are **source call-target restrictions**. The legacy import checker and the Java call graph do not yet establish full type/member/representation isolation.

The Rust tests and [real Java/Spring mutation driver](../scripts/test-target-workflow.py) cover:

1. A target lints before code exists; conformance reports the missing implementation.
2. Matching implementation passes, and harmless helper extraction still passes.
3. A transitive forbidden write fails after regeneration and after removing its source policy annotation.
4. Signature changes, including `int → long`, and undeclared public entries fail.
5. Marker deletion and resource remapping cannot escape coverage.
6. Unknown `Supplier` dispatch remains incomplete.
7. Generating views leaves the target untouched; changing the target invalidates evidence.
8. Success-only scenarios and unrelated traces cannot satisfy a target's error-path obligation.

## What to borrow from FP and FL

Backus's FP uses a small set of combining forms without value variables, but it includes a `while` functional and recursive definitions. The issue's “no loops” description is too strong. Its distribution law is valid in FP's partial, bottom-preserving semantics; other laws have additional conditions. See §§11–12 of [Backus's original paper](https://www.hinkali.com/Education/BackusTuringLecture1977.pdf#page=13).

FL's I/O history is explicit in the semantic model and deliberately implicit in its surface syntax, preventing ordinary source expressions from duplicating it. This is more specific than asking application code to pass a freely reusable history value. See the I/O discussion in [The FL Project](https://theory.stanford.edu/~aiken/publications/trs/FLProject.pdf#page=8).

The useful design hypothesis is a closed, typed workflow AST. Allow named inputs and single-assignment intermediate values for readability, while permitting only known forms: leaf invocation, sequencing, branching, and immutable construction/projection. Each referenced leaf must resolve through an approved capability vocabulary. Leave loops, recursion, arbitrary callbacks, dynamic dispatch, and new user-defined forms outside the first experiment.

For example, this **hypothetical syntax** could lower to that AST:

```text
workflow applyCoupon(request) {
  cart = call Carts.load(request.cartId);
  valid = call Coupons.validate(request, cart);
  receipt = call Carts.save(valid);
  return receipt;
}
```

These hypothetical leaf signatures differ from the current fixture. The design point is that the workflow body itself is executable and contains no arbitrary host statements. Business calculations remain in named-input leaf functions with normal tests and review. The workflow's generated host code must not become a second authored orchestration.

The compiler can then completely enumerate references **within that restricted layer**. It still cannot infer that an approved leaf is honestly named or correctly implemented. New leaves need reviewed ownership and contracts; a wrapper alone earns no new capability. Public data representations and leaf behavior are part of the boundary too.

### Effect sets are not execution traces

Union can compute a compositional effect bound relative to leaf summaries. It discards order, multiplicity, branch conditions, and exceptional exits. Both of these can have the same effect set:

```text
validate; write
write; validate
```

Only one places validation before the write. Even a correctly sequenced workflow cannot claim “no write on any error” when the write itself may fail after an attempt. Distinguish business rejection before persistence from infrastructure failure during persistence. Current `@NoEffectOnError` concerns exceptional exits and JDBC attempts; returning an error value is a different convention that would need explicit support.

An empty recognized JDBC effect set is also not proof that arbitrary Java is pure. It does not establish absence of all heap mutation, nondeterminism, timing sensitivity, or host errors. Start an algebra experiment with a small pure expression subset or leaves whose relevant semantics are actually established. Treat trusted summaries as explicit assumptions.

### Why the proposed rewrite needs a boundary

With `h` meaning “charge and return the input,” and `f = g = identity`:

```text
[f, g] ∘ h             result (100, 100), one charge
[f ∘ h, g ∘ h]         result (100, 100), two charges
```

A small local simulation reproduced those results: equal returned values and equal effect sets, different effect traces. The FP law has not failed; arbitrary effectful host functions are outside its assumptions.

Do not require totality for every FP law indiscriminately. Justify each rewrite under the chosen treatment of exceptions and divergence. Aiken, Williams, and Wimmers show why error semantics affect otherwise attractive transformations; their analyzed FL subset explicitly omits I/O. See [Safe, §3](https://theory.stanford.edu/~aiken/publications/papers/toplas95.pdf#page=5).

A first certificate should say precisely: these two workflow ASTs are equivalent under semantics S, these rules and preconditions, these unchanged leaf implementations/contracts and native dependencies, and these adapter/compiler/checker versions. A changed tax-calculation leaf invalidates that certificate even if the workflow shape is unchanged. Preserve effect order and multiplicity unless a separately justified rule allows changing them.

That can reduce review of a certified workflow refactor. It does not establish that the original business logic was correct or automatically eliminate review of changed leaves, policies, compiler behavior, or operational properties outside the semantics.

## Experiments that separate the hypotheses

| Experiment | Controlled comparison | What it can establish |
| --- | --- | --- |
| Generated context | Same agent and task, ordinary tools/docs versus an additional fresh feature view | Discovery, correctness, and effort benefit; available now |
| ADD conformance | Same context, with versus without a fixed reviewed target and checker feedback | Value of the target/checker loop |
| Executable workflow | Java orchestration versus a restricted workflow using the same leaves | Whether the restriction earns its language/runtime cost |
| Certified rewrites | Candidate refactors with independently checked certificates | Coverage of justified transformations and review effort saved |

Start with 6–8 checkout tasks and three independent attempts per condition as an exploratory pilot. Include changing the discount amount, adding preview functionality, signature changes, permitted helper extraction, repairing write-before-rejection, a legitimate policy amendment, unsupported dispatch, and incorrect business logic with unchanged effects.

Freeze the initial revision, task prompt, model/version, tools, time/token limits, and independent final acceptance checks. Both conditions get complete source access and useful ordinary documentation. Protect the evaluator's tests from implementer changes. Pin the approved target by digest; use predetermined approval decisions for legitimate amendments.

First measure the effect of a human-reviewed fixed target. Then compare “strong architect + smaller implementer” against both the smaller model alone and a strong model alone under comparable total budgets. Count architect work, analysis/build cost, implementer retries, policy amendments, and reviewer time. Report one-off costs and any explicitly amortized costs separately.

Measure independent acceptance pass rate, regressions, false-clean architectural results, unnecessary rejection of legitimate changes, incomplete-analysis frequency, total cost/time, files read, and repair cycles. Report missing runtime coverage separately from violations. Existing [mutation checks](../scripts/test-java-workflow.py) establish detector behavior; they do not establish that agents become more effective.

The small checkout can validate mechanics. It cannot substantiate large-codebase context savings. Repeat promising conditions on a representative multi-feature repository, rather than inflating the fixture with irrelevant files.

## Positioning and next decision

Architecture constraints as code already exist. ArchUnit checks Java architecture and can derive dependency rules from a PlantUML diagram; its configurable coverage is particularly relevant to this design. See the [official guide](https://www.archunit.org/userguide/html/000_Index.html#_plantuml_component_diagrams_as_rules). Packwerk checks package constraints in Ruby with documented static-analysis limits; its README explicitly distinguishes constant references from method calls. See [Packwerk's documentation](https://github.com/Shopify/packwerk#limitations).

Vitrifi's hypothesis to test is the combination: a feature-sized review/context surface, implementation-derived effects with explicit uncertainty, independently authored constraints, and an agent feedback loop. Smaller-model savings and reduced human review remain measurements to earn.

The next useful engineering milestone is the target conformance slice above. In parallel, the generated-context experiment can run on today's code. The workflow/algebra prototype should stay small and experimental until it demonstrates an advantage that the simpler loop cannot provide.
