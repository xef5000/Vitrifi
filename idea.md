# Toward an Agent-Native Way to Build Software

**Status:** Exploratory design notebook  
**Stage:** An idea becoming a possible proof of concept  
**Working description:** A programming environment where intent, contracts, abstraction boundaries, implementation, and verification are connected parts of the same system.

This document is not a language specification, implementation plan, or prompt for a coding agent. It records a direction worth investigating, possible mechanisms, unresolved tensions, and experiments that could help determine whether the idea is useful.

The examples are hypothetical. Their syntax is illustrative, not a proposed grammar. No prototype or benchmark results are implied.

---

## 1. The central idea

What would software development look like if the programming environment were designed around agents as first-class readers and editors?

Not simply an agent operating a human editor. Not just better autocomplete. Not a collection of Markdown files explaining an otherwise conventional repository.

The deeper proposal is to make a program expose several connected things:

- What capabilities it provides.
- What each capability promises.
- What concepts an implementation is allowed to use.
- How implementations refine higher-level intentions.
- Which other parts of the system depend on those promises.
- What evidence exists that the program satisfies them.
- How to inspect more detail without reading everything.

An agent would primarily work with **features, contracts, and relationships**, rather than discovering architecture by repeatedly opening files.

The user would describe a desired change. The environment would help locate the relevant capabilities, surface unresolved decisions, expose the appropriate contracts, and check the resulting implementation against explicit constraints.

A new language might be the best way to support this. It is not yet clear that a new language is necessary.

The project could ultimately become:

1. A new language.
2. A structured layer over an existing language.
3. A compiler-backed development environment.
4. A combination of these.

The foundational idea is:

> A program should expose not only how to execute it, but also how to understand it, change it, and check those changes.

---

## 2. The problem being explored

### 2.1 Files are useful containers, but questionable units of understanding

In a file-oriented workflow, the agent may need to reconstruct a feature from fragments:

- A UI component.
- An API handler.
- A domain object.
- A database table.
- A migration.
- An authorization rule.
- A stylesheet.
- Several tests.
- A design note explaining why the behavior exists.

These pieces can be well organized individually while the feature remains difficult to see as a whole.

The question “Where is checkout discounting implemented?” is not necessarily answered by a directory or a single symbol.

This project asks whether the feature itself should be a first-class navigable object.

### 2.2 Architecture often exists partly in people’s heads

A developer familiar with a system may already know:

- Which abstractions are stable.
- Which modules should not depend on each other.
- Which field is an implementation detail.
- Why a strange-looking branch exists.
- Which apparently local changes have system-wide consequences.

An agent needs some way to recover that knowledge.

Comments and documentation can help, but manually maintained descriptions can drift from code. Conversely, code alone may not reveal the original requirement or rationale.

The goal is not to eliminate documentation. It is to distinguish:

- Facts the environment can derive.
- Rules someone must declare.
- Explanations someone must preserve.
- Claims that still need verification.

### 2.3 Reading more code is not always the same as understanding more

Large context windows are useful. The project should take advantage of them.

However, being able to read an entire subsystem does not mean that doing so is always the best way to modify it.

An agent may need a broad architectural map and the implementation of one operation—not every implementation in the map.

The intended model is therefore:

> Broad context at a high level, with precise access to deeper detail.

This is not a commitment to minimizing tokens at all costs. If a change requires more implementation context, the environment should provide it.

### 2.4 Verification is part of the development interface

A compiler gives an agent concrete feedback. So do tests, assertions, architecture checks, and runtime diagnostics.

The question is whether more of the intended structure of a program can participate in that feedback loop.

Could the environment say not only:

> “This value has the wrong type.”

but also:

> “This workflow is reaching into a representation it is not allowed to know about.”

or:

> “This implementation introduces an effect absent from its public contract.”

or:

> “This change modifies the requirement that its own tests are supposed to verify.”

---

## 3. Working hypotheses—not established conclusions

Several hypotheses motivate the project.

### Hypothesis A: Explicit types help agents reason locally

Types can communicate inputs, outputs, and relationships without requiring the implementation to be read.

Whether a particular type system improves agent success is an empirical question. More complex types could also introduce difficulty, verbose errors, or unhelpful proof obligations.

The goal is useful mechanical feedback, not maximum type-system sophistication.

### Hypothesis B: Behavioral contracts are better navigation surfaces than signatures alone

A signature describes only part of an operation.

A useful contract may also describe effects, authorization, failure behavior, invariants, and assumptions.

The hypothesis is that this richer surface helps an agent avoid unnecessary implementation reading while preserving correctness.

### Hypothesis C: Feature-oriented views improve intent-driven changes

A feature view may reduce the effort needed to discover all the pieces relevant to a user request.

This depends on how accurately the view is maintained and whether the feature boundaries actually match the changes people make.

### Hypothesis D: Some abstraction discipline can become compiler-checkable

The compiler probably cannot discover universally good abstractions in arbitrary code.

But a language could require enough explicit structure to check a useful, formal version of abstraction discipline.

### Hypothesis E: Structured context improves the agent’s editing loop

Contract-first inspection, dependency queries, revision-aware context, and structured diagnostics may help agents make better changes.

This needs comparison against competent existing workflows—not only against an intentionally weak “read every file” baseline.

### Hypothesis F: These benefits might justify a new language

This is the least settled hypothesis.

If most of the benefit comes from semantic tooling over an existing language, that is a successful outcome too.

---

## 4. Three connected parts of the project

It is useful to separate three concerns.

### 4.1 The language

The language describes executable behavior and whatever constraints are made native:

- Types.
- Interfaces.
- State ownership.
- Effects.
- Preconditions and postconditions.
- Abstraction boundaries.
- Implementation relationships.

A new language could make these concepts coherent rather than attaching them through unrelated conventions.

But introducing a language also introduces substantial costs: tooling, interoperability, debugging, learning, packaging, and adoption.

### 4.2 The codebase model

The codebase model describes how the program fits together.

It may contain relationships such as:

```text
feature ──implements──> requirement
feature ──owns───────> operation
operation ──calls────> operation
operation ──reads────> state
operation ──writes───> state
UI element ──invokes─> operation
implementation ──refines──> contract
test ──checks────────> requirement
```

This model should connect implementation facts with authored requirements without pretending they are the same kind of information.

### 4.3 The agent interface

The agent interface determines how an agent interacts with that model:

- Discover a feature.
- Read its public contract.
- Expand a particular implementation.
- Find callers and dependents.
- Inspect relevant invariants.
- Propose a change.
- Run checks.
- Receive structured diagnostics.
- Review changes to behavior and policy.

These three parts should support one another, but the prototype should not assume that all three must be invented from scratch.

---

## 5. Organizing code around intent

### 5.1 Features as first-class views

Consider the request:

> “Let customers apply discount codes during checkout.”

A possible feature view is:

```text
Checkout.Discounts
    purpose
    public operations
    UI components and bindings
    domain rules
    persistence ownership and dependencies
    authorization requirements
    failure behavior
    acceptance scenarios
    implementation relationships
```

The feature is the starting point for understanding the change.

This does not imply placing everything in a single enormous file.

A feature could instead be a structured view over code and configuration that remain physically separate.

### 5.2 The editor analogy

The inspiration from a store editor or website builder is about the **unit of interaction**.

A user thinks:

- Change the checkout experience.
- Add a customer preference.
- Change how discounts work.
- Add an order status.

They do not initially think:

- Modify a route.
- Add a column.
- Change a serializer.
- Update a component.
- Adjust an authorization predicate.

Those implementation details still matter. The proposal is to connect them to the intent they serve, so they do not have to be rediscovered for every change.

This is not necessarily a no-code system. It is an attempt to make code navigable through product concepts.

### 5.3 Features do not erase execution boundaries

A feature might include both browser and server behavior, but those remain different execution environments.

Conceptual grouping must not blur:

- Client/server trust boundaries.
- Data-access permissions.
- Deployment boundaries.
- Network failure.
- Transaction boundaries.
- Secret handling.

A feature view should make those distinctions easier to see.

### 5.4 Ownership and participation are different

An operation may be owned by one component but participate in several features.

For example:

- Identity owns authentication.
- Checkout uses identity.
- Account settings also uses identity.

The feature model should distinguish “owns” from “depends on” and “appears in this view.”

Otherwise, organizing by feature could create duplication or multiple conflicting owners for shared behavior.

### 5.5 Where OOP fits

The attractive parts of OOP here are:

- Encapsulated state.
- Operations over that state.
- Public interfaces.
- Private representations.
- Invariants.

Features provide a different organizing axis:

> Objects organize state and behavior. Features organize the changes users want to make.

A feature may coordinate several objects. It does not need to become a giant object.

Composition looks like a useful starting point. Whether inheritance deserves a central role remains open.

---

## 6. Contracts as the default reading surface

### 6.1 From “file interface” to semantic contract

The original intuition is that reading a file should initially reveal something like an interface rather than every implementation detail.

That intuition can be generalized:

- A module exposes a module contract.
- A feature exposes a feature contract.
- A type exposes its public representation boundary.
- An operation exposes its behavioral contract.

Files may remain useful containers, but contracts would attach to semantic entities rather than depend entirely on file boundaries.

### 6.2 What a contract might contain

| Element | What it communicates |
|---|---|
| Purpose | Why the capability exists |
| Signature | Inputs, outputs, and type relationships |
| Effects | What it may read, change, invoke, or contact |
| Authorization | Who may invoke it under which conditions |
| Preconditions | Assumptions required before execution |
| Postconditions | Obligations after execution |
| Invariants | Rules preserved across operations |
| Failure behavior | Expected failures and their state implications |
| Dependencies | Relevant calls, data, services, and events |
| Dependents | Callers and consumers that may be affected |
| Verification status | Which claims are checked, tested, or unverified |
| Provenance | The source revision and origin of the information |
| Rationale | Decisions that cannot be recovered from code alone |

Not every operation needs every field. A small pure helper should not require a page of ceremony.

### 6.3 An illustrative contract view

```text
feature: Checkout.Discounts
operation: applyCoupon
revision: abc123

signature:
  (Actor, CartId, CouponCode)
    -> Result<CartView, CouponError>

execution:
  server

declared effects:
  read Coupons
  read Carts
  read Clock
  write Carts

authorization requirement:
  actor must own the cart

behavioral requirements:
  - rejected coupon => this operation performs no cart writes
  - accepted coupon => resulting total is non-negative

known direct calls:
  Coupons.validate
  Carts.reprice

known callers:
  CheckoutApi.applyCoupon

UI bindings:
  CartPage.couponForm

verification:
  authorization: check required
  rejection behavior: acceptance test specified
  non-negative total: assertion specified
  no proof claimed in this example

unresolved policy:
  whether coupons may stack

implementation:
  available on request
```

This is a presentation sketch, not a proposed schema.

The distinction between “declared effects” and “effects inferred from the body” matters. One is a policy or promise; the other is evidence about the implementation. A checker would compare them.

### 6.4 Derive facts; author requirements

The environment should derive facts such as:

- Signatures.
- Symbol references.
- Known direct calls.
- Field accesses.
- Source locations.
- Some dependency relationships.

Someone must still author requirements such as:

- A rejected coupon must not cause a write.
- A customer may only modify their own cart.
- Discounts must not stack.
- Existing customers must retain a particular behavior.

The compiler cannot reliably recover intended requirements merely by observing the current implementation.

### 6.5 Claims need verification status

A contract should not present every statement as equally trustworthy.

Possible statuses include:

- Declared but unchecked.
- Statically checked.
- Enforced by a runtime assertion.
- Covered by named tests.
- Formally proved under stated assumptions.
- Dependent on a trusted external component.
- Unknown or outside the checker’s supported subset.

A passing test is not a proof. A runtime assertion is not a guarantee that the program will never attempt the forbidden behavior. A static guarantee has a scope and assumptions.

These distinctions belong in the interface.

### 6.6 Names are helpful, but not complete contracts

`sort()` is understandable without studying quicksort.

But the name alone does not establish:

- Whether sorting mutates the input.
- Whether it returns a new collection.
- Whether equal elements retain their relative order.
- What assumptions the comparator must satisfy.
- Whether the comparator may have effects.
- How failures are handled.

The goal is to preserve the usefulness of the name while exposing the details callers actually need.

A contract should hide the algorithm, not hide important behavior.

### 6.7 Comments and documentation still have a role

The project should not assume that comments are useless to humans or that every explanation can be generated.

A useful distinction is:

> Derive descriptions of what the code structurally does where possible. Preserve authored explanations of why the behavior or constraint exists.

Important rationale could attach to a feature, requirement, symbol, or abstraction boundary rather than exist only as nearby prose.

Markdown could remain valuable for broad design discussion. It simply should not be the sole representation of mechanically checkable architecture.

---

## 7. Can abstraction become compiler-checkable?

This is one of the most interesting and least settled parts of the project.

### 7.1 The analogy with types

Instead of asking a compiler to infer the intended abstraction of arbitrary code, design a language in which the programmer expresses enough structure to make a useful check possible.

The compiler would not answer:

> “Is this beautiful, well-designed code?”

It might answer:

> “Does this implementation obey its declared abstraction boundary?”

That is a narrower claim, but potentially a valuable one.

### 7.2 The original integer-level idea

A simple starting point is:

```text
main is level 1

A function at level N may call:
  - functions at level N
  - functions at level N + 1
```

Larger numbers represent more implementation detail.

For a call from `f` to `g`:

```text
level(g) - level(f) ∈ {0, 1}
```

This is a concrete constraint that a checker could enforce if the levels and call relationships are available.

The attraction is that implementation detail becomes an explicit structure rather than an informal convention.

### 7.3 Why the rule alone is insufficient

If the only requirements are the adjacency rule and `main = 1`, assigning every function level 1 satisfies them.

The checker needs additional anchors:

- Declared boundaries.
- Known representation ownership.
- Operations classified as implementation mechanisms.
- Explicit “implements” or “refines” relationships.
- Restrictions on which concepts may appear in a body.

The levels could be inferred from those constraints. The integers cannot create the meaning by themselves.

### 7.4 Call depth is not abstraction depth

Consider:

```text
main → checkout → orderItems → sort
main → sort
```

A reusable operation may be meaningful in several contexts.

It is not necessarily more abstract or less abstract because a caller is closer to `main`.

Likewise, tests, command-line tools, background workers, and web requests may all be entry points into the same system.

`main` can be a useful root for one view of a program without establishing a universal semantic level for every definition.

### 7.5 Candidate models

Several models remain worth exploring.

#### Model A: Global integer levels

Every function or component has one global level.

Advantages:

- Simple to explain.
- Simple diagnostics.
- Straightforward initial experiment.

Risks:

- Artificial restrictions on reusable utilities.
- Arbitrary anchors.
- Fragile numbering.
- Pressure to add wrappers merely to satisfy adjacency.

#### Model B: Relative levels inside features

Each feature has its own hierarchy.

Advantages:

- Better fit for local reasoning.
- Less dependence on a single application root.

Risks:

- Cross-feature relationships become more complicated.
- A shared operation may need different interpretations in different views.

#### Model C: Named abstraction vocabularies

A function declares the concepts it may use rather than a universal number.

Advantages:

- The rule expresses what the body may know.
- It can address data access as well as calls.
- It supports multiple conceptual dimensions.

Risks:

- Vocabulary design may become burdensome.
- The rules may be harder to explain than numbered levels.

#### Model D: A hybrid

Named boundaries provide the actual constraints. Levels are inferred or displayed as a navigation aid.

This appears promising, but should not be treated as a settled decision.

### 7.6 Vocabulary as a possible mechanism

A hypothetical declaration might look like:

```text
abstraction Checkout:
    public types:
        Cart
        PaymentAuthorization
        Order

    public operations:
        validate
        authorizePayment
        recordOrder
```

A workflow could then say:

```text
function checkout(cart: Cart) -> Order
    vocabulary Checkout
{
    validate(cart)
    authorization = authorizePayment(cart)
    return recordOrder(cart, authorization)
}
```

Its body speaks in checkout concepts.

This would be rejected:

```text
function checkout(cart: Cart) -> Order
    vocabulary Checkout
{
    validate(cart)
    socket.write(encodePaymentPacket(cart))
}
```

A possible diagnostic:

```text
Abstraction violation

checkout directly uses:
  Transport.socket.write

Allowed vocabulary:
  Checkout

The transport representation is not visible here.
Use an exposed operation or an explicit implementation boundary.
```

The important point is not the exact syntax. It is that the compiler checks an explicit vocabulary rather than guessing whether the code feels high-level.

### 7.7 The exposed contract and implementation vocabulary are different

An operation needs to expose one conceptual interface while its implementation uses lower-level mechanisms.

For example:

```text
Checkout.authorizePayment
    exposed through: Checkout
    implemented using: PaymentGateway
```

The caller sees payment authorization. The implementation knows how to communicate with a gateway.

This suggests two distinct properties:

```text
exposed abstraction:
  what callers may know

implementation vocabulary:
  what the body may use
```

A single integer may be an incomplete representation of both.

### 7.8 Abstraction hiding must not hide effects

A workflow can avoid knowing about sockets while still acknowledging that it performs an external payment operation.

The system must not conclude that the workflow is pure just because network access is hidden behind an interface.

There may need to be a relationship between:

- Abstract effects visible to callers.
- Concrete effects used by implementations.

For example:

```text
abstract effect:
  authorize payment

implementation effects:
  send authenticated network request
  read gateway response
```

How these effects are related and checked is an open design problem.

The critical distinction is:

> Hiding a mechanism is not the same as denying that its effects exist.

### 7.9 Calls are not the only possible abstraction leaks

A function can violate a boundary without calling an obviously low-level function.

It might:

- Read a private database representation.
- Manipulate packet bytes.
- Depend on a serialization format.
- Reproduce an internal algorithm inline.
- Use a supposedly generic helper to reach hidden state.

This raises a difficult question: what about loops, indexing, arithmetic, and primitive operations?

If a high-level function may freely implement any algorithm inline, a call-layer checker does not enforce conceptual abstraction very deeply.

One possible direction is restricting certain bodies to domain-specific operations and combinators. Another is accepting a narrower guarantee: protect named representation and capability boundaries, without claiming to recognize every kind of mixed abstraction.

That narrower guarantee may be sufficient for a useful prototype.

### 7.10 Meaningful abstraction is not guaranteed by a wrapper

An agent could respond to a violation by introducing:

```text
doHighLevelThing()
```

and moving the forbidden code into it.

If the new boundary is unconstrained, the checker has enforced formatting rather than architecture.

Possible safeguards include:

- Explicit ownership of abstraction boundaries.
- Reviewable changes to allowed dependencies.
- Contracts on refinement points.
- Separate visibility for policy changes.
- Tests or proofs for important behavioral claims.

Even then, semantic quality is not fully mechanical. A misleadingly named but formally permitted interface can still be a bad design.

### 7.11 Compile time, load time, or runtime?

The original intuition included computing levels automatically from contracts, possibly at runtime.

This needs clarification.

If the goal is to reject invalid programs before execution, the natural place is compilation or static analysis:

1. Read contracts and declared boundaries.
2. Infer constraints where possible.
3. Check calls, data access, and effects.
4. Report conflicts or unresolved relationships.
5. Produce an abstraction graph.

Runtime checking could supplement this for dynamically loaded components or other situations where targets are not known earlier.

But runtime observation only shows what happened during observed executions. It should not be presented as proof about all possible executions.

### 7.12 Callbacks, recursion, and dynamic dispatch

A serious design must account for:

- Recursion.
- Same-level cycles.
- Higher-order functions.
- Callbacks.
- Interface dispatch.
- Plugins.
- Reflection.
- Foreign-language code.

In particular, the direction of control flow is not always the same as the direction of architectural dependency.

An infrastructure component may invoke a callback without needing to know the caller’s concrete implementation.

The proposed checker would likely need constraints on interfaces and callback parameters, not merely a list of direct calls.

A prototype can support a restricted subset, provided the limits are explicit.

---

## 8. A queryable program graph and progressive context

### 8.1 The codebase as a graph

The proposed semantic model should include more than a call graph.

Relevant relationships might include:

- Ownership.
- Public exposure.
- Implementation refinement.
- Calls.
- Reads and writes.
- Event publication and consumption.
- UI bindings.
- Schema usage.
- Authorization requirements.
- Acceptance criteria.
- Verification evidence.

A change can propagate through shared state or events even when there is no direct call relationship.

### 8.2 Known, possible, and observed relationships

The graph should distinguish:

- Statically known direct calls.
- Possible dispatch targets.
- Declared dependencies.
- Observed runtime calls.
- Unknown or unresolved relationships.

An observed call list is not necessarily complete. A static graph may also be incomplete in the presence of unsupported mechanisms.

Unknown relationships should be visible, not silently omitted.

### 8.3 Levels of context

A possible reading hierarchy is:

1. **System map**  
   Features, ownership, shared rules, execution environments.

2. **Feature contract**  
   Behavior, dependencies, state, authorization, acceptance criteria.

3. **Operation contract**  
   Signature, effects, obligations, failures, callers.

4. **Implementation context**  
   Body, private invariants, relevant dependency contracts, local tests.

5. **Execution evidence**  
   Diagnostics, traces, failed assertions, test results.

The agent should be able to move between these views deliberately.

### 8.4 Illustrative queries

```text
inspect feature Checkout.Discounts

inspect contract Checkout.applyCoupon

expand Checkout.applyCoupon
    include implementation
    include private invariants
    include relevant tests

find dependents of Checkout.applyCoupon

find readers and writers of Cart.discount

assess impact of proposed change

verify proposed change
```

These are sketches of an interaction model, not an API proposal.

### 8.5 Contract-first must not become contract-only

A contract may be enough for a caller, but not enough for someone changing the implementation.

An editing context may need:

- Internal invariants.
- Compatibility constraints.
- Dependency contracts.
- Concurrency assumptions.
- Existing failure behavior.
- Relevant test fixtures.
- Known unresolved issues.

The environment should help assemble that context. It should not hide necessary information merely to keep the response short.

### 8.6 Every view needs a revision

A contract, implementation body, dependency graph, and test result should identify the revision they describe.

Otherwise, an agent can reason correctly about a stale system and still produce an invalid change.

The editing protocol should detect when the inspected state and the target state differ.

---

## 9. Editing with intent

A possible workflow is:

```text
intent
  → explicit decisions and requirements
  → affected feature and dependency view
  → proposed implementation change
  → structural and behavioral checks
  → reviewable result
```

### 9.1 Intent is an entry point, not an executable specification

“Support discount codes” leaves many questions unanswered:

- May discounts stack?
- When do they expire?
- Which items are eligible?
- What happens when the cart changes?
- What happens when the request is repeated?
- How are taxes and rounding handled?

The agent should surface these decisions, propose explicit assumptions where appropriate, and distinguish accepted requirements from guesses.

The project is not based on the compiler understanding arbitrary English.

It is based on translating intent into inspectable structures and checkable claims.

### 9.2 Changes should have an explicit scope

A proposed change might identify:

```text
intent:
  allow a customer to apply one discount code to a cart

affected capability:
  Checkout.Discounts

expected implementation changes:
  operation
  UI binding
  persistence representation
  acceptance scenarios

expected preserved behavior:
  authorization
  cart ownership
  existing pricing rules
```

The scope is a working hypothesis, not a guarantee that no other area is affected.

### 9.3 Specification changes are different from implementation changes

An agent should not silently fix a failing check by weakening the requirement that the check represents.

Legitimate changes to requirements and tests must remain possible. They should simply be visible as such.

A review should distinguish:

- Implementation changed to satisfy the existing contract.
- Contract intentionally changed.
- Acceptance test updated to reflect an approved behavior change.
- Architectural restriction relaxed.
- Unsupported behavior moved behind a trusted boundary.

This is especially important if agents can edit both implementation and verification artifacts.

### 9.4 Verification should report evidence, not just success

Possible verification layers include:

- Parsing and name resolution.
- Type checking.
- Abstraction-boundary checks.
- Effect checks.
- Contract consistency checks.
- Unit tests.
- Integration tests.
- Property-based checks.
- Runtime assertions.
- Formal proofs for selected obligations.

A result should explain what was checked and what was not.

“Build succeeded” should not be allowed to imply “the requested behavior is correct.”

---

## 10. Errors, failures, and causal diagnostics

The original desire is straightforward: when something fails, the agent should be able to find where it came from.

That suggests treating diagnostic quality as part of the environment rather than as an afterthought.

### 10.1 Expected failures and unexpected defects

Expected domain outcomes might be represented explicitly:

```text
CouponExpired
CouponNotApplicable
Unauthorized
```

Unexpected defects still need useful diagnostic information.

These are different categories. A rejected coupon is not necessarily a crashed program.

### 10.2 Beyond a local stack trace

A feature may cross:

- Browser code.
- An API boundary.
- A server workflow.
- A database.
- A queue.
- A background worker.
- An external service.

The desired diagnostic object is therefore broader than a single process stack.

It might include:

- Stack information where available.
- Stable symbol identities.
- Source revision.
- Relevant causal relationships across asynchronous work.
- The violated contract or assertion.
- Safe diagnostic context.
- Links back into the semantic codebase view.

### 10.3 Diagnostics have limits

A trace does not automatically identify the root cause.

Some failures occur before diagnostics can be recorded. External systems may provide incomplete information. Capturing every intermediate value would be expensive and could expose sensitive data.

The ambition should be strong, reliable diagnostic support—not an absolute promise that every possible failure produces a complete explanation.

Redaction, sampling, access control, and source mapping need to be designed alongside observability.

---

## 11. Source of truth, storage, and change management

### 11.1 Text files can remain the canonical representation

An initial prototype does not need a program database or a new source-control system.

It could use:

- Ordinary source files.
- A small amount of authored metadata.
- A generated semantic index.
- Contract views derived from the current revision.

This would test the interaction model without first replacing the entire development stack.

### 11.2 A structured codebase is a separate research direction

A more ambitious system might store definitions and relationships directly, with files as an import/export or presentation format.

That could support stable symbol identity and structural queries more naturally.

It would also raise difficult questions about:

- Diffs.
- Merges.
- Branches.
- Offline work.
- Reproducibility.
- Portability.
- Tool independence.

This should not be a prerequisite for the first experiment.

### 11.3 Avoid a second manually maintained codebase

The architecture description should not become a duplicate implementation that agents must keep synchronized by hand.

A reasonable division is:

- Code provides executable definitions.
- Authored metadata provides intent and policy.
- Analysis derives structural facts.
- Verification produces evidence.
- Views combine these without obscuring their origins.

LLM-generated summaries can be helpful, but should not silently become authoritative facts.

### 11.4 Edits should be revision-aware and reviewable

Symbol-aware changes could reduce dependence on fragile text locations.

However, structural editing does not eliminate conflicts or semantic incompatibilities.

A proposed edit should still identify:

- The base revision.
- The entities being changed.
- The requirements being preserved or modified.
- The resulting verification evidence.

---

## 12. Side ideas worth exploring

These are related opportunities, not commitments for the first prototype.

### 12.1 Intent history

Attach a durable explanation of why a capability or constraint was introduced.

This could provide more useful context than an isolated comment or a commit message that is difficult to connect to the current code.

The challenge is preventing historical explanations from becoming misleading after later changes.

### 12.2 Behavioral diffs

A review could emphasize:

- New public operations.
- Changed effects.
- Relaxed preconditions.
- Strengthened postconditions.
- New data access.
- Changed authorization.
- Altered failure behavior.

The ordinary text diff would remain available.

### 12.3 Context packages

The environment could assemble a task-specific context package containing contracts, relevant implementations, invariants, and tests.

Such a package should include its selection rationale and known gaps. Otherwise, the agent may mistake omitted information for irrelevant information.

### 12.4 Abstraction navigation as a user interface

A visual environment might let a human or agent zoom from:

```text
application
  → capability
    → contract
      → implementation
        → execution
```

This is the code-oriented counterpart to the website-builder inspiration.

A command-line prototype could test the semantics before a visual editor is built.

### 12.5 Progressive guarantees

Different parts of a codebase might adopt different levels of checking.

For example:

- Typed interface only.
- Interface plus effects.
- Representation boundary enforcement.
- Runtime contracts.
- Selected proof obligations.

The danger is confusing partially checked code with fully checked code. The verification level would need to remain visible.

### 12.6 Change-impact analysis

The graph might help identify affected tests, consumers, schemas, and deployment components.

This should be conservative about unknown relationships. It should not promise exact impact analysis when the model is incomplete.

### 12.7 Cross-tier programming

A feature-oriented system might make browser/server/database relationships more explicit.

This is attractive, but it could easily expand the project into an entire application framework.

It is worth exploring only after the smaller contract-and-context hypothesis has been tested.

### 12.8 Persistent data and migrations

Types and contracts describe code at a revision. Persisted data may have been produced by older revisions.

A mature design would need to address that mismatch.

Migration planning, compatibility, rollback, and mixed-version deployments are important research topics, but not problems to casually claim are solved by a compiler.

---

## 13. A possible proof of concept

The first prototype should answer a focused question:

> Does contract-first, feature-aware navigation—with a small amount of enforceable abstraction structure—help an agent make correct changes to a typed codebase?

It should not attempt to prove that a new general-purpose language is already necessary.

### 13.1 Start with a small but genuinely cross-cutting application

A useful fixture could contain:

- A cart.
- Discount application.
- A minimal UI.
- A server operation.
- Persistence.
- Authorization.
- A few meaningful failure cases.

The application should be large enough that a feature spans several implementation locations, but small enough that the project remains an experiment rather than an application-development effort.

### 13.2 Use one existing typed host language initially

The host language is an experimental substrate.

Keeping it fixed helps separate the value of:

- Structured context.
- Feature organization.
- Contract views.
- Abstraction checks.

from the value of changing syntax or programming languages.

### 13.3 Build a small semantic index

Initial entities might include:

```text
features
types
operations
requirements
tests
state representations
```

Initial relationships might include:

```text
owns
calls
reads
writes
implements
checks
```

Some relationships may be manually declared at first.

The prototype should label that fact rather than imply that all architectural knowledge has been inferred.

### 13.4 Provide contract-first inspection

The agent should be able to:

1. Inspect the feature map.
2. Read an operation contract.
3. Request one implementation.
4. Find callers and important state dependencies.
5. Obtain revision-aware verification results.

A terminal interface or small tool API is sufficient.

A graphical editor is not needed to test the central idea.

### 13.5 Implement one narrow abstraction check

A reasonable first rule could be:

> Code assigned to a workflow vocabulary may use exposed domain operations but may not directly access designated transport or persistence representations.

This would test a meaningful boundary without claiming to solve all conceptual abstraction.

An integer-level checker could also be implemented as a comparison, especially to explore how often adjacency creates awkward wrappers.

The prototype should clearly state whether it checks:

- Imports.
- Direct calls.
- Member access.
- Possible dynamic targets.
- Transitive effects.
- Some subset of these.

Unknown or unsupported cases should be visible.

### 13.6 Demonstrate a complete editing loop

A useful demonstration would show:

1. The user requests a feature change.
2. The agent discovers the relevant feature.
3. The agent reads contracts before implementations.
4. The environment surfaces an unresolved policy decision.
5. The agent inspects the necessary implementation.
6. A deliberately incorrect low-level access produces an abstraction diagnostic.
7. The agent revises the change.
8. Behavioral checks run.
9. The final review shows code, contract, and policy changes separately.

This would be a stronger demonstration than a static example of nicely organized code.

### 13.7 Defer the expensive parts

The initial prototype probably should not include:

- A production runtime.
- A complete new type system.
- A universal program database.
- A replacement for version control.
- A full visual builder.
- General theorem proving.
- Comprehensive distributed tracing.
- Automatic interpretation of arbitrary product requirements.

These may become relevant later. Building them immediately would make it difficult to learn which part of the original idea is useful.

### 13.8 Introduce a language only where the experiment exposes a need

If the host language cannot reliably express or enforce a valuable constraint, a small language layer becomes easier to justify.

Possible intermediate steps include:

- An annotation system.
- A compiler extension.
- A restricted embedded language.
- A small language that translates to the host language.
- A dedicated language for contracts and refinement boundaries.

A full new language should be a conclusion supported by the experiments, not an obligation inherited from the initial excitement.

---

## 14. How to evaluate the idea

### 14.1 Use a competent baseline

Compare several conditions on the same underlying application:

**A. Conventional development tools**  
Files, search, normal symbol navigation, types, tests, and useful project documentation.

**B. Contract and feature views**  
The same language and implementation, with the proposed semantic navigation and context interface.

**C. Contract views plus abstraction enforcement**  
The same environment, with the additional checker.

This helps distinguish the benefit of better context from the benefit of stronger constraints.

A new language could become a later comparison.

### 14.2 Test different kinds of change

Useful tasks include:

- Adding a feature.
- Modifying an existing policy.
- Fixing a bug whose cause is not at the failure location.
- Changing a public contract.
- Updating a persistence representation.
- Handling a callback or asynchronous workflow.
- Making a change that should not affect unrelated behavior.

Include cases where the feature view is helpful and cases where the defect crosses feature boundaries.

### 14.3 Measure outcomes, not just compiler success

Possible measurements:

- Correct behavior on independent acceptance checks.
- Regressions.
- Missed requirements.
- Unauthorized or unintended effects.
- Time and model cost.
- Number of inspection and repair steps.
- Review effort.
- Annotation and maintenance burden.
- Frequency of unnecessary wrappers.
- Frequency with which the agent needs to read most implementations anyway.

Small token counts are not success if the resulting changes are wrong.

### 14.4 Preserve independent evaluation

An agent should not be able to make the experiment look successful merely by changing the tests used to judge it.

Use independent acceptance checks and review requirement changes explicitly.

Where possible, keep models, budgets, tasks, and execution conditions comparable across experimental conditions.

### 14.5 What would weaken the idea?

Evidence against the current direction might include:

- Contract views omit too much useful context.
- Agents repeatedly need the entire implementation anyway.
- The metadata costs more to maintain than it saves.
- The abstraction checker mainly generates boilerplate.
- A conventional symbol-aware environment performs just as well.
- The system makes incorrect claims look more authoritative.
- Useful changes require frequent escapes from the restrictions.
- Humans find the resulting codebase harder to understand or review.

These would not necessarily invalidate every part of the project.

They might indicate that the useful contribution is a smaller tooling improvement rather than a new programming paradigm.

---

## 15. Open questions

The following questions are intentionally unanswered.

### About the central abstraction

- What exactly is being checked: dependency direction, representation access, conceptual vocabulary, or a combination?
- Is one global hierarchy meaningful, or are abstractions necessarily local and multidimensional?
- Can contracts expose enough meaning to make inferred levels useful?
- How should generic utilities participate without becoming universal escape hatches?
- How much mixed abstraction should be illegal, and how much should remain a review concern?

### About contracts

- Which parts should be mandatory?
- How can small functions avoid excessive ceremony?
- How are behavioral assumptions expressed without making ordinary programming feel like theorem proving?
- How should contradictory or incomplete contracts be surfaced?
- When a contract changes, how are dependent assumptions invalidated?
- How should external services and libraries be represented when their behavior cannot be fully checked?

### About features and intent

- Who defines the feature boundaries?
- Can the environment suggest them without making those suggestions authoritative?
- What happens when a change spans several features?
- How are shared capabilities represented?
- How do technical maintenance tasks fit when they do not correspond to a product feature?
- How much ambiguity should an agent resolve autonomously?

### About language design

- Which guarantees genuinely require a new language?
- Should the abstraction system be part of the type system or a separate checker?
- How are effect and abstraction constraints expressed for higher-order functions?
- How should recursion, dynamic loading, reflection, and foreign code work?
- Which constraints are inferred, and which require explicit declarations?
- What does a controlled escape hatch look like, and how does it affect the guarantee?

### About storage and tooling

- Should text remain canonical?
- How are stable identities maintained through renaming, splitting, and merging?
- How are structural edits reviewed?
- Can the graph remain trustworthy during partial edits or failed builds?
- How are stale context and concurrent changes detected?
- Which graph relationships are guaranteed complete, and which are best-effort?

### About trust and verification

- Who can modify architectural policy?
- How are requirement changes distinguished from attempts to satisfy a checker?
- What evidence is sufficient before a change is accepted?
- How are trusted components and unchecked boundaries displayed?
- How can the environment avoid giving false confidence?
- What happens when the specification itself is wrong?

### About the human experience

- Is the result still pleasant for humans to read and edit?
- Does the system teach a useful mental model, or require learning too much metadata?
- Can developers use ordinary tools when needed?
- Is the feature view an aid or a new source of lock-in?
- Would a human-only team benefit from the same structure?

---

## 16. Relevant ideas to investigate

This project should not assume that every ingredient is new.

Research directions include:

- Design by contract.
- Abstract data types and representation hiding.
- Module systems.
- Effect systems.
- Capability systems.
- Refinement types.
- Architectural dependency checking.
- Domain modeling and feature-oriented organization.
- Language-server protocols and semantic indexes.
- Structured editors and program databases.
- Incremental compilation and analysis.
- Formal refinement and proof-carrying evidence.
- Distributed tracing and causal debugging.
- Semantic and behavioral code review.

Names raised as possible starting points include Eiffel, Koka, Unison, ArchUnit, and the Language Server Protocol.

These are research leads, not a claim that any one of them implements the proposed system.

The possible contribution is likely to be in the combination:

> An environment that treats contracts, abstraction boundaries, feature views, and verification as the primary interface through which an agent develops software.

---

## 17. What should remain true as the idea evolves?

The implementation may change substantially. Several principles seem worth preserving.

### Make structure explicit where it can be checked

Do not ask the model to repeatedly infer rules that the environment could represent directly.

### Keep claims separate from evidence

A declared requirement, a passing test, a runtime assertion, and a proof are different things.

### Hide implementation detail without hiding relevant behavior

A caller should not need to study the algorithm, but should understand the operation’s important effects and obligations.

### Preserve access to deeper context

Abstraction should guide inspection, not prevent it.

### Treat uncertainty as information

Unknown dependencies, incomplete contracts, and unsupported checks should remain visible.

### Make policy changes reviewable

An agent should not silently redefine the rules to make its implementation pass.

### Keep the system understandable to humans

Agent-native should not mean human-hostile.

### Earn the complexity

Every additional declaration, compiler rule, storage mechanism, or language feature should justify its cost.

---

## 18. The next concrete step

Before designing a complete language, build one small experience that demonstrates the intended way of working.

The agent should be able to start from a product capability, inspect a meaningful contract, expand only the implementation it needs, make a cross-cutting change, receive an abstraction violation when it crosses a declared boundary, and produce a reviewable result with explicit verification evidence.

That experiment would help answer the most important early questions:

1. Is the contract a useful default reading surface?
2. Can feature views be maintained without becoming duplicate documentation?
3. Which abstraction restrictions are helpful rather than ceremonial?
4. Does the agent actually make better changes?
5. What remains impossible or awkward without changing the language?

The long-term ambition is substantial, but the first useful experiment can be small.

The project begins with the idea that source code should not be the only interface to a program.

A program could also provide a structured account of its purpose, promises, boundaries, and evidence—and let both humans and agents work through that account without losing access to the implementation underneath.
