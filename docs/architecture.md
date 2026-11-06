# Compiler architecture

Vitrifi 0.3 adds an implementation-derived pipeline: `javac` attribution and supported Spring wiring → source facts → transitive effects and independent policies → generated feature files. Explicit Spring/JDBC scenario runs supply separate runtime evidence. See the [Java/Spring guide](java-spring.md) for that workflow and its supported scope. The description below documents the original authored-language pipeline, which remains available for compatibility.

For an introduction to the language itself, start with [Reading and writing Vitrifi](reading-vitrifi.md) and the [syntax reference](language.md). This document describes how the compiler implements those concepts.

Vitrifi has two distinct languages: `.vitrifi` for architecture, and the host language for implementation. The compiler is a Rust program. It links the two through explicit bindings while keeping authored claims, extracted facts, and test evidence distinguishable.

```text
feature-local .vitrifi modules
       │ imports and exported declarations
       ▼
parser → names/types/visibility → resolved architecture graph
                                      │
ordinary host files → syntax adapters ─┤
                                      ├─ diagnostics / CLI / PR review
explicit check commands → evidence ────┘
```

## Feature files are the primary interface

A feature module describes its purpose, public types and operations, resources, requirements, effects, and dependencies. It imports other modules directly. A feature may span several host files, technical layers, or languages; physical proximity is a convention, not a requirement that every feature fit one directory.

In the checkout example, one `discounts.vitrifi` file owns the JavaScript form, TypeScript transport, and Python workflow/domain operations. Their relationships are local declarations. It imports `storage/cart.vitrifi` for a shared storage capability. The optional Ruby/Java audit feature lives in its own `audit/discount.vitrifi` file. Technical layers are boundaries inside the feature, not separate feature modules a reviewer must reconstruct.

There is no authored global manifest. A repository-wide command discovers modules, while a command with an entry follows that module's imports. A generated global graph can support analysis without becoming a second source file contributors must maintain.

For review, the relevant `.vitrifi` diff exposes the architectural change directly. Renaming an operation, adding a resource effect, relaxing a boundary, changing a requirement, and adding a check are source changes in that feature's module. A graphical view can be an optional consumer of compiler output; it is not the review workflow.

## Typed relationships before host analysis

The parser produces declarations with source locations. Resolution connects aliases and names to declaration identities. The compiler checks the kinds on either side of a relationship: effects target resources, calls target operations, requirements target promises, and boundary permissions target boundaries. Types resolve to built-ins or exported nominal declarations.

This is more than schema validation of a document. References have scope, visibility, and types. Modules compose by importing exported declarations. Invalid architectural programs fail compilation before their host bindings can be treated as coherent contracts.

The first effect rule is consistency along declared calls. A caller's declared effects must cover its declared callees' effects. This checks the architecture program; it does not prove that a host operation lacks undeclared effects.

## Explicit language-neutral bindings

An operation binds to one symbol in a host file. Tree-sitter adapters for Python, Ruby, JavaScript, TypeScript, and Java extract definitions, signatures, imports, and syntactic call expressions. The compiler can report missing definitions and invalid source rather than accepting stale names silently.

The adapter interface is language-neutral. A future compiler- or language-server-backed adapter could provide richer types and resolved targets without changing feature authoring. The initial syntax adapters cannot fully resolve dynamic dispatch, custom import paths, reflection, generated code, or framework-specific wiring.

Host signatures and Vitrifi signatures remain distinct. Vitrifi currently checks names and its own nominal types, not arbitrary cross-language type equivalence. Likewise, an authored `calls` edge is a relationship declared by the architecture program; a syntactic call expression is an observed source fact. They must not be presented as interchangeable evidence.

## Narrow host dependency enforcement

Named boundaries group host files and declare permitted imports. Resolved local host imports can be compared against that policy. The workflow in the checkout example may depend on its domain, while the domain may depend on storage. A direct workflow-to-storage host import violates the policy.

The workflow still names the storage resource in its public effect declaration. That is intentional: hiding a persistence mechanism should not hide the fact that the operation changes a cart. Importing an architectural declaration and importing a host implementation are separate relationships.

## Evidence is explicit

A `check` declaration specifies a command, working directory, timeout, and requirement references. Compilation and review do not execute those commands. `verify` runs the requested checks explicitly and reports their results.

A passing command is evidence for that run. The compiler does not establish test completeness, independently validate the `covers` mapping, or prove the requirements' prose. Claims without supporting checks remain claims. For evaluating the original research hypothesis, independent acceptance tests still matter.

## Deterministic source, practical limits

The feature language has no arbitrary code execution, environment access, loops, or dynamic imports. The compiler reads declared inputs and reports errors with source locations. Rust provides the native implementation and grammar integration; the architectural semantics are defined by Vitrifi itself.

Distributed modules improve ownership and review locality. They do not establish unlimited repository scalability. Entry-based transitive module loading reduces the authored scope of a query, but broad host-file discovery, hashing, and global relationship checks can still cost time. Incremental compilation, dependency caches, and large-repository benchmarks remain work to earn through measurement.

The prototype also does not provide general theorem proving, runtime contract injection, semantic source edits, immutable evidence, or enforcement against someone changing the feature file to weaken policy. Those changes are visible in the PR and must be reviewed alongside the code.
