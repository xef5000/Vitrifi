/* Prewritten snapshots from scripts/test-workflow-proofs.py.
   This static page does not execute Vitrifi or check certificates. */
(() => {
  const flat = `let cart = call load(cartId);
let owner = call owns(actorId, cart);
if owner {
} else {
  reject "Only the cart owner can apply a coupon";
}
let valid = call validCode(code);
if valid {
} else {
  reject "Coupon is invalid";
}
let unused = call available(cart);
if unused {
} else {
  reject "A coupon is already applied";
}
let receipt = call save(cart, code);
return receipt;`;

  const nested = `let cart = call load(cartId);
let owner = call owns(actorId, cart);
if owner {
  let valid = call validCode(code);
  if valid {
    let unused = call available(cart);
    if unused {
      let result = call save(cart, code);
      return result;
    } else {
      reject "A coupon is already applied";
    }
  } else {
    reject "Coupon is invalid";
  }
} else {
  reject "Only the cart owner can apply a coupon";
}`;

  const native = `public Receipt save(Cart cart, String code) {
    int total = Math.max(0, cart.total() - 1000);
    carts.save(cart.id(), total, code);
    return new Receipt(cart.id(), code, total);
}`;

  window.VITRIFI_PROOFS = [
    {
      id: "refactor", tab: "Restructure the flow", status: "passed",
      title: "Different structure. The same execution.",
      description: "Move each continuation inside its successful branch and rename receipt to result. The Java actions, contracts and policies stay unchanged.",
      file: "Checkout.Discounts.workflow.vitrifi", before: flat, after: nested,
      label: "Equivalence certified",
      summary: "Three branch_continuation steps bring the workflows to the same term, allowing lexical local renaming. The checker replays every step independently of proof search.",
      command: "vitrifi workflow proof-check refactor.proof.json --base APPROVED_COMMIT --root examples/workflow-checkout",
      output: "PASSED · workflow_equivalent\n3 checked steps · branch_continuation\nSame values, exits and ordered action invocations\nNative inputs and required baseline match.",
      consequence: "A failed action stops both versions at the same point. An unselected branch performs no calls. The same rejection path reaches the same message without saving."
    },
    {
      id: "extra-call", tab: "Add an extra call", status: "unproven",
      title: "The same effect set is not the same behavior.",
      description: "The new version calls validCode twice. Its effects [] declaration covers modeled resource effects; it does not establish that the Java action is pure.",
      file: "Checkout.Discounts.workflow.vitrifi", before: flat,
      after: nested.replace("  let valid = call validCode(code);", "  let extra = call validCode(code);\n  let valid = call validCode(code);"),
      label: "No certificate found",
      summary: "The ruleset has no law for duplicating or deleting native calls. The extra invocation could fail, diverge, or affect native state even without a modeled JDBC effect.",
      command: "vitrifi workflow prove … --base APPROVED_COMMIT --out refactor.proof.json",
      output: "PROOF_NOT_FOUND\nThe derivations do not meet under the supported laws.\nNo certificate is produced.",
      consequence: "PROOF_NOT_FOUND is a limit of the supported proof search, not a general proof of inequivalence. Equal effect sets alone cannot justify this change."
    },
    {
      id: "rejection", tab: "Change a rejection", status: "unproven",
      title: "A message is part of the result.",
      description: "Keep the calls and effects but change the invalid-coupon rejection. The restricted workflow semantics observe the rejection message too.",
      file: "Checkout.Discounts.workflow.vitrifi", before: flat,
      after: nested.replace('reject "Coupon is invalid";', 'reject "Try another coupon";'),
      label: "No certificate found",
      summary: "Branch restructuring preserves the selected rejection. It cannot turn one rejection message into another, even when both versions still avoid a write.",
      command: "vitrifi workflow prove … --base APPROVED_COMMIT --out refactor.proof.json",
      output: "PROOF_NOT_FOUND\nRejection values do not reduce to a common term.\nNo certificate is produced.",
      consequence: "An intentional business behavior change can still be reviewed and tested. It does not receive a certificate claiming the old workflow's behavior is preserved."
    },
    {
      id: "native", tab: "Change the calculation", status: "failed",
      title: "The workflow stayed the same. The discount did not.",
      description: "Change 1000 to 900 inside a Java action. Ordinary effect checks still see a permitted write, but the native context no longer matches the baseline.",
      file: "CheckoutActions.java", before: native, after: native.replace("- 1000", "- 900"),
      label: "Native context changed",
      summary: "The proof pipeline compares native file contents against the Git baseline. A changed calculation blocks certification before algebraic replay can claim unchanged behavior.",
      command: "vitrifi workflow prove … --base APPROVED_COMMIT --out refactor.proof.json",
      output: "PROOF_CONTEXT_CHANGED\nNative source differs from the baseline.\nReview the calculation and run business tests.",
      consequence: "The certificate does not decide which discount is correct. It requires the native actions to stay unchanged while it certifies an orchestration refactor."
    }
  ];
})();
