# Java/Spring checkout: derived architecture and checked execution

This example makes the Java implementation the source of structural facts. Vitrifi derives operation signatures, calls, and supported JDBC effects from Java source. A few annotations supply feature identity, a database resource name, and policies that the implementation must satisfy.

The generated `.vitrifi` file shows the implementation's structure. The neighboring authored `.target.vitrifi` file independently constrains the public API, resource mapping, allowed effects, dependency boundaries, and runtime policies. Regenerate the view when Java changes; change the target only when the intended architecture changes. Private helpers and call lists do not need to be mirrored in the target.

## What to read

- [Checkout.Discounts.target.vitrifi](src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.target.vitrifi) is the architecture contract. The [target guide](../../docs/targets.md) explains each field and its enforcement.
- [Checkout.Discounts.vitrifi](src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.vitrifi) is the generated implementation view.
- [DiscountService.java](src/main/java/dev/vitrifi/example/checkout/DiscountService.java) owns the feature and its policies. It receives a `CartRepository` through its constructor.
- [CartRepository.java](src/main/java/dev/vitrifi/example/checkout/CartRepository.java) is the injected interface.
- [JdbcCartRepository.java](src/main/java/dev/vitrifi/example/checkout/JdbcCartRepository.java) is its Spring repository implementation. It contains the SQL that establishes the resource effects.
- [CheckoutConfiguration.java](src/main/java/dev/vitrifi/example/checkout/CheckoutConfiguration.java) connects a real Spring context, H2, `JdbcTemplate`, and the Vitrifi runtime integration.
- [DiscountServiceTest.java](src/test/java/dev/vitrifi/example/checkout/DiscountServiceTest.java) exercises acceptance, ownership rejection, invalid coupons, replacement rejection, and previewing a cart.

The Java code uses one in-memory database and synchronous Spring-managed calls. The feature is deliberately small enough to demonstrate agreement between generated facts and the real implementation.

## What the annotations contribute

```java
@Service
@Feature(value = "Checkout.Discounts", purpose = "Apply one valid coupon to the owner's cart without writes on rejection")
public class DiscountService {
    private final CartRepository carts;

    public DiscountService(CartRepository carts) {
        this.carts = carts;
    }

    @NoEffectOnError(action = "write", resource = "Carts")
    public Receipt applyCoupon(String actorId, String cartId, String code) {
        // Ordinary Java validation and persistence code.
    }

    @Forbid(action = "write", resource = "Carts")
    public Cart preview(String cartId) {
        return carts.load(cartId);
    }
}
```

The snippet abbreviates the method body; the linked source is the runnable implementation.

`@Feature` names the feature and explains its purpose. `@ResourceTable(value = "Carts", table = "carts")` on the repository maps a SQL table to the architectural resource `Carts`. These annotations provide names the compiler cannot infer from business intent. The repository's SQL supplies the read/write facts.

`@Forbid(action = "write", resource = "Carts")` says the preview operation must not write carts. Source analysis can report a reachable, supported write. The runtime guard also blocks a matching JDBC operation before the driver's execute method is called.

`@NoEffectOnError(action = "write", resource = "Carts")` says that an invocation of `applyCoupon` which exits by throwing must not have attempted a matching JDBC write. The monitor keeps this violation even if the calling test catches the business exception. A returned error object is a normal return and is not covered by this annotation's exception rule.

Here, “no writes” means **no JDBC execution attempts**, including attempts that fail at the database or are later rolled back. It does not mean only “no durable state change.” That stronger distinction matters when writing policies.

## Build, generate, and verify

Use Java 21, Maven, and a Rust toolchain. From the repository root:

```sh
mvn install
mvn -f examples/spring-checkout/pom.xml generate-resources
cargo build

./target/debug/vitrifi lint --root examples/spring-checkout
./target/debug/vitrifi conform --root examples/spring-checkout
./target/debug/vitrifi generate --root examples/spring-checkout
./target/debug/vitrifi verify --java --root examples/spring-checkout -- mvn test
```

The Maven build installs the Java analyzer/runtime locally and writes the example's analysis classpath. The `generate-resources` command prepares the standalone test classpath before Vitrifi fingerprints it; dependency or classpath changes during verification invalidate the evidence. Generation uses that classpath to attribute Java source without running application code. The generated feature view is written beside its Java feature source.

Verification is explicit. It runs the test command in the example directory, supplies a fresh trace destination and session identity, and checks the resulting report. Java tests also run normally with `mvn test` in this directory after the reactor has been installed.

The test fixture starts one `EvidenceSession` for the verification command, wraps each scenario with `evidence.scenario(...)`, and calls `evidence.verify()` after all scenarios. A new session writes an unfinished marker before execution. Only finishing the session can write a completed report. Reusing a trace destination for another session makes the report incomplete, so later tests cannot overwrite earlier failures. Policy violations, unsupported operations, and untested required paths prevent a passing result. A successful process that produced no session evidence cannot establish verification.

Runtime events record feature and method identity, thread scope, SQL operation, resource, and outcome. The report does not contain SQL parameter values, raw SQL, returned objects, or business exception messages. `VITRIFI_TRACE_FILE` selects the report destination when using the integration directly; the CLI handles it during verification.

## Try a real implementation violation

In `DiscountService.applyCoupon`, insert a persistence call immediately after loading the cart, before the ownership and coupon checks:

```java
Cart cart = carts.load(cartId);
carts.save(cart.id(), cart.total(), code); // Deliberate mutation: writes before validation.
```

Run the verification command again. Rejection tests still catch `CouponRejected` as expected; they deliberately contain no assertions about database write counts. Nevertheless, the Vitrifi session records the write followed by the exceptional exit, and verification fails `no_effect_on_error`.

Remove that added line and rerun verification. The normal implementation validates before its persistence call.

For a second experiment, add a `carts.save(...)` call to `preview`. The write becomes visible to source analysis. If executed through the Spring proxy, the JDBC guard throws `PolicyViolationException` before the database executes the forbidden statement. Updating the generated file cannot grant permission. Even deleting `@Forbid` leaves the independent target prohibition in place and causes conformance to fail.

`scripts/test-target-workflow.py` automates these experiments in a temporary copy, along with signature drift, undeclared public methods, feature-marker deletion, resource remapping, harmless helper extraction, unsupported dispatch, and missing runtime evidence. In CI, pass the target's explicit path to `conform` or `check` so its deletion also fails.

## What the evidence covers

The pilot combines attributed source analysis with events at actual JDBC execution boundaries. Its evidence is limited to the supported adapter and the scenarios that ran:

- Feature methods must be public, nonfinal, synchronous calls through Spring-managed proxies. Calling a policy method through `this`, constructing the service directly, or bypassing the instrumented `DataSource` does not provide the same interception. Unsupported source paths must remain visible as gaps.
- The SQL classifier accepts a bounded single-table subset of `SELECT`, `INSERT`, `UPDATE`, and `DELETE`, with simple scalar assignments and predicates. Prepared-statement values stay in the driver. Joins, subqueries, functions, stored procedures, sequence expressions, multiple statements, and other unsupported SQL cannot be treated as known harmless effects.
- Within a feature scope, unsupported SQL or an unmapped table produces incomplete evidence and blocks the operation. The integration prevents unwrapping its JDBC proxies and blocks unsupported updatable-result-set effects within that scope.
- SQL events describe client statement attempts. Database triggers, server-side side effects, independent clients, external services, asynchronous work, and durable transaction outcomes are outside this pilot's claim.
- `NoEffectOnError` needs an actual exceptional invocation to be tested. A success-only run leaves it `untested`. A passing `Forbid` scenario is evidence about observed executions, not a proof over every possible input.

The feature and resource annotations carry intent. Generated source facts explain the implementation, while runtime policies test behavior that a plain call graph cannot establish.
