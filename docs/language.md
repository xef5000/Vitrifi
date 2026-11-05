# Vitrifi language reference

This describes the syntax and compiler behavior of the current prototype. Start with [Reading and writing Vitrifi](reading-vitrifi.md) for a guided walkthrough. The [checkout example](../examples/checkout/README.md) contains complete feature files and their implementations.

Vitrifi describes architecture: named concepts, interfaces, promises, dependencies, and implementation locations. The compiler resolves and checks these relationships. The application still runs in its host language. Vitrifi has no executable expressions, variables, conditionals, loops, or function bodies.

Examples below are declaration or field fragments unless they show a complete `feature` block. A fragment's referenced names must be declared in that feature or exported by an imported module; a binding must point to an existing source definition.

## Vocabulary at a glance

| Construct | What it describes | Where it belongs |
| --- | --- | --- |
| `use` | Another feature module and its local alias | Before the feature block |
| `feature` | One product feature or shared capability | Exactly one per file |
| `purpose` | Why a feature or operation exists | Feature or operation body |
| `question` | An unresolved design question | Feature body |
| `type` | A named kind of value crossing an interface | Feature body |
| `resource` | State or a capability an operation can affect | Feature body |
| `requirement` | A named behavioral statement | Feature body |
| `boundary` | A group of implementation files and its permitted imports | Feature body |
| `operation` | A typed interface linked to an implementation definition | Feature body |
| `check` | A command supplying evidence for named requirements | Feature body |
| `export` | Makes a declaration available to importing features | Before `type`, `resource`, `requirement`, `boundary`, `operation`, or `check` |

`purpose` and `question` are prose fields, not declarations that other names can reference.

## File structure, names, and punctuation

```vitrifi
use "./storage/cart.vitrifi" as Store;

feature Checkout.Discounts {
  purpose "Apply coupons while preserving rejected carts.";
  question "Should a future version allow coupon replacement?";

  export type Receipt;
  export type Rejected;
}
```

Imports come first, followed by one feature block. `Checkout.Discounts` is the feature's identity; its dots do not create nested modules or determine a filesystem path. Feature identities must be unique among the modules compiled together.

| Syntax | Meaning |
| --- | --- |
| `{ ... }` | A feature, operation, or check body |
| `;` | Ends a field or a declaration without a body |
| `[a, b]` | A list; `[]` is an empty list |
| `(name: Type, ...)` | An operation's named, typed parameters |
| `-> Type` | The operation's result type |
| `Result<Receipt, Rejected>` | A type with type arguments |
| `Store.Cart` | A declaration named `Cart` exported by import alias `Store` |
| `"text"` | A string, such as prose, a path, or a host symbol selector |
| `// text` or `/* text */` | A comment; block comments do not nest |

Do not add a semicolon after the closing brace of a feature, operation, or check. Whitespace and indentation do not change meaning. Square-bracket lists and parameter lists allow a trailing comma; type argument lists do not.

Identifiers use ASCII letters, digits, and underscores, beginning with a letter or underscore. Names are case-sensitive: `Cart` and `cart` are different. Declaration names are single identifiers; only feature identities and qualified references contain dots. PascalCase for types/resources/requirements/boundaries/checks and camelCase for operations are conventions, not requirements.

Strings must use double quotes. They support JSON-style escapes, including `\"`, `\\`, `\n`, `\t`, and `\u00e9`; a literal unescaped newline is invalid. Prose is stored as text, never evaluated as code.

Declarations share one name scope per feature: a type and a resource cannot both be named `Cart`. An import alias also cannot reuse a declaration or alias name. Built-in type names cannot be redeclared. Declarations may refer to local declarations written later in the file.

## Imports, paths, and exports

```vitrifi
use "./storage/cart.vitrifi" as Store;
```

`use` loads another `.vitrifi` file and gives it a local alias. Inside the importing feature, use `Store.Cart`, `Store.Carts`, or `Store.load`. References have exactly two forms: a local name such as `Receipt`, or `Alias.Name` such as `Store.Cart`. A feature's full identity is not an alternate reference path, and chained access such as `Store.Other.Cart` is unsupported. Import the module you need directly.

Paths in `use`, `bind`, boundary patterns, and check `cwd` are relative to the `.vitrifi` file containing them. `../` is allowed while the result stays inside the selected repository root. Absolute paths and paths escaping that root are rejected. The prototype does not support symlinked source paths.

All declarations are private to their feature by default:

```vitrifi
type InternalDecision;
export type Receipt;
```

`export` allows another feature to reference the declaration after importing its module. It does not expose a host-language symbol, create an HTTP endpoint, or change runtime access control.

An exported operation must use exported user types in its parameters and result, including types nested inside `List`, `Option`, or `Result`. Resources in its effects and requirements in its `requires`/`ensures` must also be exported. Its `calls` may target private operations in the same feature: a public interface can have private implementation steps.

Compilation with an entry feature loads that file and its transitive imports. Compilation without an entry discovers `.vitrifi` files under `--root`, subject to repository discovery rules. There is no global feature registration file.

## `purpose` and `question`: intent and open decisions

```vitrifi
purpose "Apply a valid coupon without changing a rejected cart.";
question "Should customers be allowed to replace an existing coupon?";
```

A feature may have one optional `purpose` and any number of `question` fields. An operation may have one optional `purpose`; it cannot contain `question`.

Write a purpose as the reason this capability exists or the responsibility this operation takes on. Put an independently discussable behavioral promise in a named `requirement` instead, so operations and checks can reference it. Put a decision that has not been made in `question`; it does not become a constraint or cause compilation to fail.

The compiler preserves these strings but does not interpret their truth or enforce their wording. Missing purpose text is allowed.

## `type`: a kind of value

```vitrifi
export type Cart;
export type Receipt;
export type Rejected;
```

A type names something passed into or returned from an operation. `Cart` might represent a Python data class, a Java object, or a serialized document. Vitrifi does not choose that representation.

User-defined types are **nominal**: the declaration gives the type its identity. They are **opaque**: the compiler knows the name but no fields or internal structure. Two distinct type declarations are not automatically interchangeable. The current syntax has no record bodies, field definitions, type aliases, user-defined generic types, inheritance, or inferred types. There is no `bind` inside a type.

| Built-in type | Intended reading | Example |
| --- | --- | --- |
| `String` | Text | `code: String` |
| `Int` | An integer | `discount: Int` |
| `Bool` | A true/false value | `-> Bool` |
| `Unit` | No meaningful result value | `-> Unit` |
| `Any` | An intentionally unspecified kind of value | `payload: Any` |
| `List<T>` | A collection of values of type `T` | `-> List<Receipt>` |
| `Option<T>` | A value of type `T`, or absence | `-> Option<Store.Cart>` |
| `Result<T, E>` | Success of type `T`, or failure of type `E` | `-> Result<Receipt, Rejected>` |

Type arguments can nest, as in `List<Option<Store.Cart>>`. The compiler checks names, declaration kinds, visibility, and argument counts: `Result<Receipt>` is invalid because `Result` requires two arguments.

These are architectural interfaces. The prototype does not compare an operation's parameter types or result type against its host implementation. In the checkout example, a Python return value or exception is described as `Result<Receipt, Rejected>`; this is an authored abstraction, not a generated wrapper or a verified exception conversion.

## `resource`: something an operation can affect

```vitrifi
export resource Carts;
export resource Coupons;
resource FormState;
```

A resource names state or a capability whose use matters to reviewers: persisted carts, coupon data, browser form state, an event stream, or an external service. It has a name only. It does not declare fields, provision infrastructure, or locate a database table.

Use `type Cart` for a cart value passed between operations, and `resource Carts` for the shared cart state they read or change. The names are related by the author's design; the compiler does not infer a relationship from singular/plural spelling.

Choose the granularity at which a change should be visible in review. Separate resources are useful when their permissions or effects differ. A resource for every local variable would usually add noise without describing an architectural responsibility.

### `effects`: actions on resources

Inside an operation:

```vitrifi
effects [read Store.Carts, read Store.Coupons, write Store.Carts];
```

| Action | Intended use |
| --- | --- |
| `read` | Observe resource state |
| `write` | Change resource state |
| `network` | Communicate with a resource representing a remote capability |
| `emit` | Publish to a resource representing an event destination |

Each item is an action followed by a resource reference. Types, operations, and requirements cannot occupy the resource position. Other verbs such as `delete` are unsupported; describe such a mutation with `write` and explain its meaning in the operation or requirement.

An effect describes what an operation may do, including through its declared callees; it does not require that effect on every invocation. For example, `write Store.Carts` can coexist with a requirement that rejected requests perform no writes. Vitrifi has no syntax for conditional effect branches.

The compiler compares exact action/resource pairs along `calls` relationships. If a callee declares `read Carts`, the caller must include that same resolved effect; `write Carts` alone does not cover it. Imported aliases for the same resource resolve to the same identity.

Omitting `effects` means an empty declared effect list. It does not request inference or prove the implementation pure. A caller with an empty list will fail if a declared callee has effects.

## `requirement`, `requires`, and `ensures`: behavioral statements

```vitrifi
export requirement AuthenticatedActor "The host provides an authenticated actor identity.";
export requirement RejectNoWrite "A rejected request performs no cart writes.";
```

A requirement has a name and a double-quoted statement. Prefer a specific behavior that a reviewer can discuss and a check can exercise. State relevant conditions in the text: “a rejected request” is more precise than “safe writes.”

An operation uses requirements in two ways:

```vitrifi
requires [AuthenticatedActor];
ensures [RejectNoWrite];
```

Read `requires` as an assumption that should hold when the operation is used. Read `ensures` as an obligation the operation takes on. These fields contain references to named requirements, not inline strings or Boolean expressions. A requirement declaration alone does not attach that behavior to every operation in the feature.

The compiler checks the references and their visibility. It does not prove the statements, execute assertions, or check that a caller establishes its callee's `requires`. Requirements are not automatically inherited through `calls`. A successful check command supplies evidence for the requirements listed in its `covers`; see [checks](#check-test-evidence).

## `operation`: an interface and its relationships

```vitrifi
export operation applyCoupon(actor: String, cart: String, code: String)
    -> Result<Receipt, Rejected> {
  purpose "Authorize and validate before persisting a discounted cart.";
  bind python "./domain/discounts.py" symbol "apply_coupon";
  effects [read Store.Carts, read Store.Coupons, write Store.Carts];
  requires [AuthenticatedActor];
  ensures [RejectNoWrite];
  calls [Store.load, Store.loadCoupon, Store.save];
}
```

An operation gives an architecturally significant action a name, typed inputs, a result type, and an implementation location. Use operations for feature entry points, important steps, and shared capabilities whose relationships matter in review. You do not need an operation declaration for every private helper in the host code.

The parameter list and result type are required, including for an operation with no inputs or no useful result: `operation refresh() -> Unit { ... }`. Parameter names must be distinct. Every operation requires one host binding; abstract or unbound operations are not supported yet.

| Operation field | Required? | Value and default |
| --- | --- | --- |
| `bind` | Yes | Language, source path, and `symbol` selector |
| `purpose` | No | Double-quoted prose; absent by default |
| `effects` | No | List of action/resource pairs; defaults to `[]` |
| `requires` | No | List of requirement references; defaults to `[]` |
| `ensures` | No | List of requirement references; defaults to `[]` |
| `calls` | No | List of operation references; defaults to `[]` |

Fields may appear in any order, but each may appear only once. Each field ends with `;`.

### `bind`: follow the declaration to implementation

```vitrifi
bind python "./domain/discounts.py" symbol "apply_coupon";
```

The unquoted word selects the host language, the first string selects a file relative to this feature, and the second string selects a definition within that file. Vitrifi operation names may differ from host names.

| Language word | Supported file extensions | Example selector |
| --- | --- | --- |
| `python` | `.py` | `"apply_coupon"` |
| `ruby` | `.rb` | `"DiscountAudit.perform"` |
| `javascript` | `.js`, `.jsx`, `.mjs`, `.cjs` | `"submitCoupon"` |
| `typescript` | `.ts`, `.tsx` | `"handleCoupon"` |
| `java` | `.java` | `"DiscountAudit.record"` |

The language must match the file's supported extension. The compiler parses selected host files with Tree-sitter and checks that the selector identifies exactly one extracted definition. A containing class commonly qualifies a method name. A missing, renamed, or ambiguous selector causes a diagnostic. The same file/symbol cannot be owned by two compiled operations; import and reference the existing operation instead.

Binding does not inject a dependency, wrap a function, enforce a runtime contract, or prove signature equivalence. Dynamic definitions and dispatch, inheritance, overloads, and framework wiring can require richer adapters.

### `calls`: follow the declared dependency graph

```vitrifi
calls [Store.load, Store.loadCoupon, Store.save];
```

Each entry names an operation this operation is intended to use. The list describes architectural dependencies, not a sequence of instructions: it does not say `load` runs before `loadCoupon`, or that `save` always runs. It does not express arguments, branches, repetition, or return-value wiring.

Follow a local name to another operation in the same feature. Follow `Alias.Name` through its `use` to an exported operation in another feature. Then inspect that operation's effects, promises, and calls, or open its binding to read implementation details.

Declared calls can describe injected or cross-language connections even when host syntax cannot resolve the runtime target. They do not create those connections. The compiler checks their names and effect consistency; it does not compare every declared call with an observed host call or type-check call arguments.

## `boundary` and `allows`: permitted implementation imports

```vitrifi
boundary Domain ["./domain/*.py"] allows [Store.Storage];
boundary Workflow ["./workflow/*.py"] allows [Domain];
```

A boundary groups host files using module-relative patterns. `allows` lists the other boundaries those files may directly import. Both lists are required; either can be empty. A boundary has no body and ends with `;`.

Read the example as: workflow files may import domain files; domain files may import files in the exported `Storage` boundary of `Store`. This permission is directional and is not transitive. Workflow does not gain permission to import storage directly. Imports within the same boundary are allowed without listing the boundary itself.

Use boundaries for dependency rules that matter inside a feature or shared capability. They need not be separate feature files. The compiler classifies files by their paths, not by which operation binds them or where the operation is written.

Patterns use the current `globset` matcher. Exact paths and wildcard patterns are accepted; matching is case-sensitive. In this prototype, `*` can match path separators, so `"./domain/*.py"` can also select `domain/internal/helpers.py`. Use exact paths when that distinction matters. Overlapping boundaries on an analyzed file produce an ambiguity error. An empty or unmatched pattern list is allowed and selects no files.

Boundary checks cover resolved local host imports from classified source files. A resolved target must have an unambiguous boundary in the modules being compiled, and that boundary must be the source boundary itself or explicitly allowed. An unclassified source file has no boundary policy to enforce. External or unresolved imports, dynamic access, reflection, and runtime framework wiring remain analysis gaps.

`allows` targets boundaries; `calls` targets operations; `effects` targets resources. They answer different questions. A workflow may declare `write Store.Carts` because it calls a domain operation that writes carts, while its boundary allows imports only from `Domain`. Naming a resource in Vitrifi does not authorize a direct host import of its storage implementation.

## `check`: test evidence

```vitrifi
check Behavior {
  run ["python3", "-m", "unittest", "discover", "-s", "tests", "-v"];
  cwd ".";
  covers [Owner, RejectNoWrite, NonNegative, SingleCoupon];
  timeout 30;
}
```

A check connects an existing test command to the requirements it is intended to exercise. The tests themselves remain ordinary host-language code.

| Check field | Required? | Value and default |
| --- | --- | --- |
| `run` | Yes | Nonempty list of strings; first item is a nonempty executable name/path, remaining items are arguments |
| `covers` | Yes | List of requirement references; `[]` is accepted but associates no requirements |
| `cwd` | No | Existing directory relative to this feature file, inside the repository; defaults to `"."` |
| `timeout` | No | Integer seconds from `1` to `3600`; defaults to `60` |

Fields may appear in any order, but each may appear only once. `run` is an argument vector, not a shell command string: pipes, environment-variable substitution, and wildcard expansion do not happen automatically.

Checks run only when `vitrifi verify` is requested, after compilation succeeds. A verify command executes the checks in the compiled feature set, including imported modules. Parsing, `check`, `compile`, `inspect`, and `review` do not execute them. The executable and its dependencies must be available in the check's environment.

The compiler validates requirement links, command shape, working directory, and timeout. It does not inspect the tests to establish that they truly cover each requirement. A passing command is evidence for one run. It is not proof of all inputs, and the environment can affect the result. Recorded evidence can become stale after repository inputs change.

## Compilation and review

| Command | What it does |
| --- | --- |
| `vitrifi check` | Parses modules, resolves names and types, checks bindings and declared relationships, and checks resolved host import boundaries |
| `vitrifi inspect` | Prints feature contracts and declarations in the terminal |
| `vitrifi source feature.vitrifi operationName` | Prints the selected operation's bound implementation |
| `vitrifi compile` | Writes a resolved model for tools; generated JSON is output, not authored source |
| `vitrifi review --base REF` | Compares architecture declarations against a Git revision |
| `vitrifi verify` | Runs declared checks and records their evidence |
| `vitrifi evidence` | Reads recorded evidence and detects stale inputs |

Keep `.vitrifi` files beside the implementation and review their ordinary PR diffs. A compiler success means that the checked architectural program is consistent within the prototype's analysis. Whether its prose, calls, and effects faithfully describe the application still needs implementation review and appropriate tests.

Continue with [Reading and writing Vitrifi](reading-vitrifi.md), the [checkout editing loop](../examples/checkout/README.md), or the [compiler architecture](architecture.md).
