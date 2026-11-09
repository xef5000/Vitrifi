/* Explanatory snapshots of the repository's end-to-end mutation checks.
   These examples do not execute Java or run Vitrifi in the browser.
   Terminal output is condensed for the illustration, not raw CLI output. */
window.VITRIFI_EXAMPLES = [
  {
    id: "workflow-rejection",
    tab: "Unsafe sequencing",
    label: "01 / Workflow compilation",
    title: "Catch a write before generating Java.",
    description: "An executable workflow declares that business rejection cannot follow a cart write. Moving save above the guard breaks that rule on a reachable rejection path.",
    file: "Checkout.Discounts.workflow.vitrifi",
    policy: "no_effect_on_rejection [write Carts];",
    before: {
      code: `// Operation excerpt; action declarations omitted.
let cart = call load(cartId);
let owner = call owns(actorId, cart);
if owner {
} else {
  reject "Only the cart owner can apply a coupon";
}
// Other coupon guards precede save in the full file.
let receipt = call save(cart, code);
return receipt;`,
      status: "passed",
      label: "Rejection before persistence",
      summary: "The workflow's closed control flow keeps writes off business-rejection paths. Java analysis also checks the declared native action effects.",
      path: ["load", "owns", "reject or continue", "save"],
      command: "vitrifi workflow build --root examples/workflow-checkout",
      output: "PASSED · workflow compilation\nNo write Carts on rejection\nNative actions and selected targets conform\nGenerated DiscountService.java is current."
    },
    after: {
      code: `// Operation excerpt; action declarations omitted.
let cart = call load(cartId);
let receipt = call save(cart, code);
let owner = call owns(actorId, cart);
if owner {
} else {
  reject "Only the cart owner can apply a coupon";
}
// Other coupon guards now also follow save.
return receipt;`,
      status: "failed",
      label: "Compilation blocked",
      summary: "A continuing path now writes before reaching reject. The compiler rejects the workflow and keeps the previous generated service intact.",
      path: ["load", "write Carts", "reject owner"],
      command: "vitrifi workflow build --root examples/workflow-checkout",
      output: "WORKFLOW_REJECTION_EFFECT\nwrite Carts can precede a business rejection\nNo generated service is overwritten."
    },
    explanation: "The workflow is the source of the orchestration, so ordering is checked directly. Hiding the write inside a Java action declared effects [] also fails: the compiler checks the bound against the action's analyzed effects.",
    limit: "This checks all branches of the restricted workflow under supported Java analysis. Infrastructure failure is distinct from an explicit business rejection; neither mechanism rolls back writes."
  },
  {
    id: "forbidden-write",
    tab: "A hidden write",
    label: "02 / Static policy",
    title: "A preview quietly starts saving.",
    description: "A harmless-looking persistence call changes the feature's behavior. Vitrifi follows the call into the repository and sees the SQL write.",
    policy: '@Forbid(action = "write", resource = "Carts")',
    before: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId) {
    // Load the cart for a preview.
    return carts.load(cartId);
}`,
      status: "passed",
      label: "Read-only preview",
      summary: "Only a recognized read is reachable. The source policy passes.",
      path: ["preview", "load", "SELECT carts"],
      command: "vitrifi generate --root examples/spring-checkout",
      output: "PASSED · static analysis\npreview → read Carts\nForbid write Carts: passed\nGenerated feature view is current."
    },
    after: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId) {
    // This new call reaches an UPDATE.
    carts.save(cartId, 0, "bad");
    return carts.load(cartId);
}`,
      status: "failed",
      label: "Forbidden write found",
      summary: "The new write conflicts with the unchanged policy, even after regeneration.",
      path: ["preview", "save", "UPDATE carts"],
      command: "vitrifi generate --root examples/spring-checkout",
      output: "FAILED · FORBIDDEN_EFFECT\npreview → save → write Carts\nForbid write Carts: violated\nRegenerating the view cannot grant permission."
    },
    explanation: "The annotation supplies the rule. Compiler-resolved calls and recognized SQL supply the facts. A method's name does not decide whether it reads or writes.",
    limit: "Static checking covers the adapter's supported source graph and SQL. Unresolved behavior prevents a complete result."
  },
  {
    id: "write-before-rejection",
    tab: "Runtime evidence",
    label: "03 / Runtime policy",
    title: "The exception is right. The write is wrong.",
    description: "A test can catch the expected rejection while a write has already happened. Scenario-linked JDBC evidence reveals that extra effect.",
    policy: '@NoEffectOnError(action = "write", resource = "Carts")',
    before: {
      code: `@NoEffectOnError(action = "write", resource = "Carts")
public Receipt applyCoupon(
    String actorId, String cartId, String code) {
    Cart cart = carts.load(cartId);
    if (!cart.ownerId().equals(actorId)) {
        throw new CouponRejected("Only the owner can apply a coupon");
    }
    // Additional coupon checks omitted.
    int total = Math.max(0, cart.total() - 1000);
    carts.save(cart.id(), total, code);
    return new Receipt(cart.id(), code, total);
}`,
      status: "passed",
      label: "Rejection without a write",
      summary: "The exercised ownership-rejection path throws before a JDBC write attempt.",
      path: ["load", "reject owner", "no write"],
      command: "vitrifi verify --java --root examples/spring-checkout -- mvn test",
      output: "PASSED · runtime evidence\nScenario: reject another cart owner\napplyCoupon: threw CouponRejected\nNo write Carts on error: passed"
    },
    after: {
      code: `@NoEffectOnError(action = "write", resource = "Carts")
public Receipt applyCoupon(
    String actorId, String cartId, String code) {
    Cart cart = carts.load(cartId);
    carts.save(cart.id(), cart.total(), cart.coupon());
    if (!cart.ownerId().equals(actorId)) {
        throw new CouponRejected("Only the owner can apply a coupon");
    }
    // Additional coupon checks omitted.
    int total = Math.max(0, cart.total() - 1000);
    carts.save(cart.id(), total, code);
    return new Receipt(cart.id(), code, total);
}`,
      status: "failed",
      label: "Write on an error path",
      summary: "Vitrifi retains the violation even when the test catches the expected exception.",
      path: ["load", "write Carts", "reject owner"],
      command: "vitrifi verify --java --root examples/spring-checkout -- mvn test",
      output: "FAILED · runtime policy\nScenario: reject another cart owner\nwrite Carts → threw CouponRejected\nNo write Carts on error: violated"
    },
    explanation: "A call graph shows that a write is possible; the executed scenario shows whether it happened before an exceptional exit. Success-only scenarios leave this policy untested.",
    limit: "The rule concerns JDBC execution attempts, including failed or rolled-back attempts. It does not undo writes or prove all unexecuted paths. Java excerpts omit unrelated validation."
  },
  {
    id: "stale-view",
    tab: "A stale view",
    label: "04 / Implementation agreement",
    title: "The code changed. The review file did not.",
    description: "A Java signature gains a parameter, but the committed feature snapshot still describes the old interface. Fresh source analysis catches the mismatch.",
    policy: "Generated snapshots must agree with current source facts.",
    before: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId) {
    // Matches the committed feature view.
    return carts.load(cartId);
}`,
      status: "passed",
      label: "Snapshot agrees",
      summary: "The generated operation matches the current Java method signature.",
      path: ["Java signature", "generated operation", "match"],
      command: "vitrifi generate --check --root examples/spring-checkout",
      output: "PASSED · snapshot freshness\npreview(String cartId)\nCheckout.Discounts.vitrifi: current\nNo snapshot changes required."
    },
    after: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId, int revision) {
    // The feature file has not been regenerated.
    return carts.load(cartId);
}`,
      status: "stale",
      label: "Snapshot needs regeneration",
      summary: "Freshness checking fails. Source-based review also exposes the signature change.",
      path: ["new parameter", "old snapshot", "mismatch"],
      command: "vitrifi generate --check --root examples/spring-checkout",
      output: "STALE · snapshot freshness\npreview(String cartId, int revision)\nCheckout.Discounts.vitrifi: out of date\nRegenerate and review the feature diff."
    },
    explanation: "The feature view is generated from source. CI can check freshness, and architectural review compares Java revisions even if someone forgot to regenerate.",
    limit: "Historical review uses the current analyzer and dependency classpath. Ordinary build review is still needed for dependency changes."
  },
  {
    id: "unknown-call",
    tab: "An unknown call",
    label: "05 / Explicit uncertainty",
    title: "An indirect call leaves an analysis gap.",
    description: "Wrapping a known repository call in a Supplier introduces dispatch this adapter cannot resolve. Vitrifi reports that missing knowledge.",
    policy: '@Forbid(action = "write", resource = "Carts")',
    before: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId) {
    // Supported constructor-injected repository.
    return carts.load(cartId);
}`,
      status: "passed",
      label: "Known call path",
      summary: "The adapter resolves the repository target and recognizes its read effect.",
      path: ["preview", "load", "read Carts"],
      command: "vitrifi generate --root examples/spring-checkout",
      output: "PASSED · static analysis\npreview → JdbcCartRepository.load\nReachable effect: read Carts\nForbid write Carts: passed"
    },
    after: {
      code: `// DiscountService.java
@Forbid(action = "write",
        resource = "Carts")
public Cart preview(String cartId) {
    java.util.function.Supplier<Cart> fetch =
        () -> carts.load(cartId);
    return fetch.get();
}`,
      status: "incomplete",
      label: "Unknown dispatch",
      summary: "The result is incomplete. The policy cannot receive a clean static result.",
      path: ["preview", "Supplier.get", "unknown target"],
      command: "vitrifi generate --root examples/spring-checkout",
      output: "INCOMPLETE · unresolved behavior\nCall: fetch.get()\nReachable effects: unknown\nForbid write Carts: unknown"
    },
    explanation: "This example is not a proven bug. It is a limit of the current adapter. Keeping the gap visible prevents an unsupported path from looking verified.",
    limit: "The first generated adapter covers bounded Java/Spring wiring and supported JDBC SQL. Additional language and framework adapters can extend that scope."
  }
];
