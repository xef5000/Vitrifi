# Vitrifi

**Architecture targets checked against code, with generated views for review.**

Vitrifi 0.3 generates feature-local `.vitrifi` files from Java source and supported Spring wiring. Small annotations declare feature identity, resource names, and policies; the compiler derives signatures, dependencies, and recognized SQL effects. Reviewers can inspect the architectural consequences in the same PR as the implementation change.

The Rust core combines a `javac` analysis adapter with explicit Spring/JDBC runtime verification. Unknown dependencies remain visible and prevent a complete check. Generated facts cannot grant themselves permission to violate an authored policy.

Feature-local `.target.vitrifi` files now express independent design constraints: exact public Java interfaces, permitted effects, dependency boundaries, and runtime policies. `conform` compares those targets to source facts. The generated `.vitrifi` view still follows the implementation; regenerating it cannot widen a target. Start with the [target language and workflow guide](docs/targets.md).

Open the [interactive explainer](site/index.html) in a browser for before/after examples, a guide to generated syntax, and the boundaries of each check. It is a small static site that works directly from disk; see [site/README.md](site/README.md).

## Java/Spring: design, conform, generate, verify

With JDK 21+, Maven, and Rust installed, run from this repository:

```sh
cargo build
mvn install
mvn -f examples/spring-checkout/pom.xml generate-resources
./target/debug/vitrifi lint --root examples/spring-checkout
./target/debug/vitrifi conform --root examples/spring-checkout
./target/debug/vitrifi generate --root examples/spring-checkout
./target/debug/vitrifi check --java --root examples/spring-checkout
./target/debug/vitrifi verify --java --root examples/spring-checkout -- mvn test
./target/debug/vitrifi review --java --root examples/spring-checkout --base origin/main
```

The second Maven command prepares the standalone test classpath before Vitrifi fingerprints it. A dependency or classpath change during verification invalidates the evidence.

`lint` needs only the target file and can run before implementation exists. `conform` checks the source against it; `check` also checks generated freshness; `verify` runs instrumented scenarios. These commands discover targets automatically. In CI, select each required file with `--target path/to/Feature.target.vitrifi` so deleting a target fails the check. Use repeated flags for several targets in the same Java application.

The [Spring checkout example](examples/spring-checkout/README.md) uses a real service, interface-injected repository, `JdbcTemplate`, and H2 database. `@Forbid` rejects a reachable forbidden effect; `@NoEffectOnError` checks actual JDBC attempts on exercised error paths. Changing the host implementation is enough to change the generated architectural model.

Start with the [Java/Spring guide](docs/java-spring.md) for annotations, generated-file review, test evidence, supported patterns, and limits. The generated view remains a file beside the feature; no web server or global authored manifest is required.

## Earlier authored workflow

The v0.2 authored language remains available with Python, Ruby, JavaScript, TypeScript, and Java syntax adapters. Its checks validate declared relationships, binding existence, and resolved import boundaries. It does not provide the implementation agreement of the new Java/Spring workflow. Generation and runtime instrumentation for the other languages are future adapters.

## Learn to read Vitrifi

For the current Java/Spring workflow, read [Architecture targets](docs/targets.md) for the authored constraints and the [Java/Spring guide](docs/java-spring.md) for generated facts and runtime evidence. Target files deliberately describe public contracts and bounds, leaving helper methods and call lists to generation.

For the original authored syntax, start with [Reading and writing Vitrifi](docs/reading-vitrifi.md). It walks through a feature, explains types, resources, operations, and boundaries, and shows how to follow a promise into its implementation and tests.

Use the [syntax reference](docs/language.md) for the exact grammar, required fields, defaults, and compiler rules. The [checkout exercises](examples/checkout/README.md) let you try changes and see the resulting diagnostics.

## Start with a feature file

The checkout example has three feature files. The discount feature spans the browser, transport, workflow, and domain; it imports a shared storage capability:

```text
examples/checkout/
  discounts.vitrifi       # complete discount feature across JS, TS, and Python
  storage/cart.vitrifi    # shared storage interface and resources
  audit/discount.vitrifi  # optional audit feature across Ruby and Java
```

A reviewer opens `discounts.vitrifi` to follow `submitCoupon → handleCoupon → applyDiscount → applyCoupon` and the call into shared storage. Here is the domain-operation portion of that feature:

```text
use "./storage/cart.vitrifi" as Store;

feature Checkout.Discounts {
  purpose "Apply a valid coupon without changing a rejected cart.";

  export type Receipt;
  export type Rejected;
  export requirement RejectNoWrite "A rejected request performs no cart writes.";
  boundary Domain ["./domain/*.py"] allows [Store.Storage];

  export operation applyCoupon(actor: String, cart: String, code: String) -> Result<Receipt, Rejected> {
    bind python "./domain/discounts.py" symbol "apply_coupon";
    effects [read Store.Carts, read Store.Coupons, write Store.Carts];
    ensures [RejectNoWrite];
    calls [Store.load, Store.loadCoupon, Store.save];
  }
}
```

`Store.Carts` must resolve to an exported resource. `Store.load` must resolve to an exported operation. `Result` requires two type arguments. The Python binding must identify an existing definition. Calling an operation whose declared effects exceed the caller's declaration is a compiler error. The requirement's behavioral truth still needs separate evidence.

## Run it

Build with a Rust toolchain:

```sh
cargo build
cargo run -- check --root examples/checkout
cargo run -- inspect examples/checkout/discounts.vitrifi
cargo run -- verify --root examples/checkout
```

The built command is `target/debug/vitrifi`; `cargo build --release` produces `target/release/vitrifi`. The tool itself does not require a Python interpreter. The checkout example's explicit behavioral check invokes Python because its runnable domain implementation is Python.

To check a single feature and its imported feature modules:

```sh
cargo run -- check examples/checkout/discounts.vitrifi
```

For a repository containing only authored feature files, check all `.vitrifi` files below its root:

```sh
cargo run -- check --root /path/to/repository
```

Paths in `use`, `bind`, boundary patterns, and check working directories are relative to the file that declares them. `--root` establishes the permitted repository root; it defaults to the current working directory. A single-feature entry can only refer to files inside that root.

## Review architecture in the PR

Open the changed `.vitrifi` file first. Its diff shows changes to exposed types, requirements, effects, dependencies, or boundary permissions. Follow imports when a change crosses a feature boundary. Source code and acceptance tests remain ordinary reviewable files.

```sh
cargo run -- review --root examples/checkout --base HEAD
cargo run -- check --root examples/checkout --json
```

`review` compares feature declarations against a Git revision. It helps expose contract and policy changes; it does not decide whether weakening a requirement is justified. The ordinary Git diff remains the primary review surface. No generated file must be edited to register a feature.

`compile` can emit the resolved model for tools:

```sh
cargo run -- compile --root examples/checkout --out /tmp/checkout.vitrifi.json
```

JSON here is compiler output, not the authored language or source of truth. A future editor or graph view can consume that model without becoming necessary to review the feature.

## What the earlier authored workflow checks

- Parsing, declaration names, imports, export visibility, and reference kinds.
- Nominal feature types and the arity of `List<T>`, `Option<T>`, and `Result<T, E>`.
- Host binding existence using Tree-sitter syntax parsers for all five languages.
- Consistency of declared calls and resource effects.
- Permitted dependencies between declared file boundaries for resolved host imports.
- Explicit behavioral check commands, invoked only by `verify`.

The language's types describe the architectural interface. The prototype does not prove equivalence between those signatures and every host-language type or exception convention. Declared call relationships are not inferred runtime call targets, and declared effects are not a proof about a host body. Dynamic dispatch, framework wiring, reflection, custom module paths, and cross-service execution remain analysis gaps.

The Python checkout path has seven acceptance checks. The JavaScript/TypeScript part of the discount feature and the separate Ruby/Java audit feature provide integration seams; the fixture is not a deployed distributed application. Their requirements remain authored claims unless a check supplies evidence.

## Why a dedicated language

The feature file has a small, explicit vocabulary: types, operations, requirements, resources, effects, calls, boundaries, and checks. Its compiler can reject invalid relationships without executing arbitrary feature-file code. Imports support separate ownership and reusable interfaces across a repository.

Rust is the compiler implementation language. `.target.vitrifi` holds independent architecture constraints for Java/Spring; generated `.vitrifi` files show implementation facts. The earlier adapters retain the original authored `.vitrifi` language. The host application keeps its own language and toolchain.

Distributed modules establish a path toward incremental analysis; they are not a claim of unlimited scale. The prototype loads an entry's transitive feature imports, or discovers all modules for a repository-wide check. Large-repository performance and incremental caches need separate measurement and work.

See [Reading and writing Vitrifi](docs/reading-vitrifi.md), the [syntax reference](docs/language.md), and [compiler architecture](docs/architecture.md). The original [idea notebook](idea.md) records the broader research direction.

## VS Code syntax highlighting

The [VS Code extension](editors/vscode/README.md) recognizes both `.vitrifi` and `.target.vitrifi` files and provides highlighting, comments, bracket pairing, folding, and declaration snippets. Install the generated `editors/vscode/dist/vitrifi-0.3.0.vsix` with **Extensions: Install from VSIX…**.

Its TextMate grammar can also be used as the basis for a future [GitHub Linguist contribution](https://github.com/github-linguist/linguist/blob/main/CONTRIBUTING.md#adding-a-language). GitHub registration is separate from installing the editor extension and requires an accepted grammar license, samples, and sufficient public usage.

## Develop

```sh
cargo test
cargo run -- check --root examples/checkout
cargo run -- verify --root examples/checkout
```
