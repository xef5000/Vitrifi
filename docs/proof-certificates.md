# Algebraic workflow proof certificates

Vitrifi can certify a restricted workflow refactor against an immutable Git revision. The proof is an equational derivation: a small checker replays named laws on both workflow ASTs until they meet modulo local variable renaming. Java actions can have effects, fail, or diverge; the accepted laws preserve their invocation order, arguments and multiplicity.

The first ruleset is `vitrifi.workflow-laws/1`, over the execution semantics `vitrifi.workflow/1` introduced by the [compiled workflow pilot](workflows.md). A certificate is a generated, feature-local `.proof.json` artifact. Its JSON is a wire format, not an authored architecture manifest or a second implementation.

## A concrete refactor

Flat guards in the checkout workflow can become nested branches:

```vitrifi
// Before
if owner {
} else {
  reject "Only the cart owner can apply a coupon";
}
let valid = call validCode(code);
if valid {
} else {
  reject "Coupon is invalid";
}
let receipt = call save(cart, code);
return receipt;
```

```vitrifi
// After
if owner {
  let valid = call validCode(code);
  if valid {
    let result = call save(cart, code);
    return result;
  } else {
    reject "Coupon is invalid";
  }
} else {
  reject "Only the cart owner can apply a coupon";
}
```

The checker establishes the same result and rejection message, and the same reached calls with the same values in the same order. A failed `validCode` prevents `save` in both versions. A diverging action leaves both versions with the same invocation prefix. The `receipt` → `result` rename is checked using lexical binding identities.

It refuses a changed discount calculation in Java, a changed rejection message, or an extra `validCode` call. An empty JDBC effect set does not permit dropping, duplicating or commuting an arbitrary native action.

## Produce and check a certificate

First modify a workflow on top of a committed, approved baseline and rebuild its Java service:

```sh
./target/debug/vitrifi workflow build --root examples/workflow-checkout

./target/debug/vitrifi workflow prove \
  src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.workflow.vitrifi \
  --root examples/workflow-checkout \
  --base APPROVED_COMMIT \
  --out Checkout.Discounts.proof.json

./target/debug/vitrifi workflow proof-check Checkout.Discounts.proof.json \
  --root examples/workflow-checkout \
  --base APPROVED_COMMIT
```

Use the existing Java input options (`--java-analyzer`, `--source-root`, `--classpath-file`, `JAVA_HOME` or `VITRIFI_JAVA`) as needed. Prepare the Maven classpath before running the commands. Both endpoints must compile, the baseline's committed service must match its workflow, and the current service must be freshly generated. Both must conform to the selected architecture targets. These commands invoke javac analysis with annotation processing disabled; they do not run a historical build or application code.

`prove` searches the supported laws, replays its result through the checker, and writes a new artifact. It does not edit the workflow or generated Java. The output path must be inside `--root`, end in `.proof.json`, have an existing ordinary parent directory and not already exist. `proof-check` never calls proof search and never writes source or evidence.

The recorded Git commit must be available locally; fetch the approved baseline when CI uses a shallow checkout. The checker does not fetch historical objects or execute commands from the certificate.

In CI, pass the independently selected review baseline with `--base` and required targets with repeated `--target` flags. The checker resolves the reference and requires the exact recorded commit. Without `--base`, it checks against the certificate's recorded commit and reports `base_pinned_by_caller: false`; that proves equivalence to that commit, not that a reviewer approved it. A certificate is not an authorization or a signature.

If the producer reports `PROOF_NOT_FOUND`, it has not established a common term under these laws. The workflows might still be equivalent. This first ruleset deliberately covers a small set of useful structural refactors.

## Run the checkout demonstration

With Rust, JDK 21+ and Maven installed:

```sh
cargo build
mvn install
python3 scripts/test-workflow-proofs.py --demo-dir /tmp/vitrifi-proof-demo
```

The script creates a separate Git repository containing the checkout baseline, changes its flat guards to nested branches, generates the Java service and certificate, checks it, and runs the Spring/H2 scenarios. It retains the refactor and certificate in the specified new directory for inspection. `git diff` there shows the real before/after change. Omit `--demo-dir` to run the full mutation suite in a temporary repository.

## The checked laws

`A`, `B` and `K` below are statement sequences; `c` is an already available primitive boolean or literal. There are no nested calls or effectful condition expressions in this workflow language.

| Law | Checked reduction | Preconditions |
| --- | --- | --- |
| `branch_continuation` | `if c {A} else {B}; K` → `if c {A ▷ K} else {B ▷ K}` | `K` is nonempty; at least one branch can structurally fall through; append `K` only to branches that can continue |
| `literal_branch` | `if true {A} else {B}` → `A`; `if false {A} else {B}` → `B` | Condition is syntactically a boolean literal |
| `identical_branches` | `if c {A} else {A}` → `A` | Branch syntax is identical modulo source positions |
| Local alpha-equivalence | Consistently rename local bindings | References resolve to the same lexical binding identities; public signatures and native contracts are unchanged |

`A ▷ K` means `A; K` when `A` has a structurally continuing path; otherwise it means `A`. A branch may terminate through return, explicit rejection, or an action's runtime failure/divergence. A moved continuation still executes only after normal completion of preceding statements.

Every intermediate workflow must pass the language type, scope, control-flow and rejection-effect checks. If moving a continuation would capture or collide with a branch-local binding, that rewrite is rejected. The producer can therefore miss equivalences that require a prior fresh-name transformation. Both endpoints retain exactly the same feature purpose, resources, action names/bindings/types/effect bounds, public operation signatures and policies.

The proof uses symmetry and transitivity: `before →* common ←* after`, with local alpha-equivalence at the meeting point. Intermediate states need not be normal forms. Each replay step selects an operation, a lexical branch path and a statement index, and records input/output AST hashes. The checker reconstructs that step itself. It rejects unknown laws, invalid addresses, altered hashes or a different meeting term.

## Why the laws hold

Treat a native action as an arbitrary state transition that consumes its input values and returns a value, throws, or diverges. It can mutate shared heap state and produce an ordered effect trace. Related executions use the same native transition behavior at corresponding invocations. Sequential composition propagates return, rejection and failure without executing a later continuation.

For `branch_continuation`, both versions read the same primitive condition and select the same branch. If that branch exits, both stop. If it completes normally, both execute `K` after the same preceding actions in the same state. Scope checking ensures the moved `K` refers to the same values. No law moves `K` before a validation or executes it twice along one path.

For `literal_branch`, both versions select the same statically known branch. For `identical_branches`, either selection executes the same sequence; reading an already bound primitive boolean adds no action or exceptional exit. Local alpha-equivalence changes only lexical names. These arguments compose inside enclosing branches and operation sequences, then across the replayed derivation.

The equivalence observes returned values, rejection messages, failed action/cause behavior, divergence and ordered native action invocations with their arguments. It preserves native heap/effect behavior conditional on unchanged action semantics. It abstracts from source locations, stack-trace line numbers, debug-local names, code size, timing, allocation/GC and resource exhaustion. Java lowering is part of the trusted implementation; the certificate does not prove the compiler or JVM correct.

## What is pinned

The checker reconstructs assumptions from the current checkout and the recorded Git baseline. It never trusts a certificate-provided list of paths to read arbitrary external files.

| Input | Treatment |
| --- | --- |
| Workflow endpoints | Exact SHA-256 of the actual Git source and current source |
| Native context | Exact hashes of unchanged application files, including Java leaves/helpers, other workflows, targets, tests, resources and build configuration |
| Maven parent configuration | Ancestor `pom.xml` and `.mvn/` files inside the repository are also compared |
| Generated artifacts | The selected service is reconstructed and checked at both endpoints; analyzer-generated views and `.proof.json` artifacts are excluded from the unchanged-file comparison |
| Target selection | Selected file paths are pinned; files must have unchanged Git contents; conformance is checked at both endpoints |
| Native dependencies | Ordered classpath contents, including directory members, are hashed; order and multiplicity are preserved |
| Java toolchain | Runtime version, vendor, VM name, specification version and architecture are pinned |
| Analyzer/runtime | Analyzer JAR hash and runtime classpath hashes are pinned |
| Trusted Rust implementation | Semantics/ruleset names and a source/lockfile fingerprint for the checker, parser, lowering and input interpretation are pinned |

Application context comparison is intentionally conservative: even an unrelated authored file change in the selected application prevents this first certificate from passing. Generated snapshots, proof artifacts and ordinary build directories outside source trees are excluded. Git ignore rules cannot hide changed application sources or resources from the context comparison. Every Java file physically present under the selected source root must also be included by the Java adapter; otherwise proof validation fails for incomplete source coverage. Explicitly selected ignored targets are still checked against Git.

The baseline is reanalyzed against the **same pinned current dependency set** as the candidate. This proves the two workflows under that explicit environment. It does not reconstruct whatever dependency versions happened to be installed when the old commit originally ran. Unchanged Maven files alone are insufficient evidence of unchanged downloaded dependencies; the actual classpath content hashes supply the assumptions.

Certificate hashes and rule replay protect integrity relative to trusted checker code and the caller's chosen baseline. They do not make an untrusted checker binary, native library, or modified CI configuration trustworthy. Portable certificates use content hashes and repository-relative source paths, but checking requires matching Java runtime properties and dependency bytes.

## Evidence and practical limits

The checker is a small Rust kernel in `src/proof_kernel.rs`, separate from automatic search in `src/proof_search.rs`. The kernel is not itself formally verified in a proof assistant. The test suite independently executes representative laws across boolean inputs, arbitrary action return choices, injected failure and divergence positions. The Git/Java integration suite attempts forged proofs, changed business logic, resource/build changes and stale dependency environments, and checks the transformed Java through real Spring scenarios.

Runtime evidence remains specific to the workflow source and generated Java that ran. This PR does not transfer an old test report to a certified refactor. Keep the existing business acceptance tests, runtime policies and architectural conformance checks. The certificate can reduce review of the orchestration refactor; it does not establish the correctness of the original discount rule or approve changes to native leaves and policies.

Input size, derivation length, nesting and statement expansion are bounded. Unsupported rewrites, unknown reachable Java calls, changed public contracts and incomplete target analysis fail closed. The initial ruleset has no law for commuting calls, distributing an effectful call into multiple invocations, deleting a supposedly pure call, or changing native calculations.
