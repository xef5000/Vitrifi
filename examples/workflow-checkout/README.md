# Checkout with compiled Vitrifi orchestration

Read [Checkout.Discounts.workflow.vitrifi](src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.workflow.vitrifi) first. It owns the owner/coupon/replacement checks and the position of the save. [CheckoutActions.java](src/main/java/dev/vitrifi/example/checkout/CheckoutActions.java) implements the native leaves. `DiscountService.java` is generated and committed, so Maven builds it normally.

From the repository root:

```sh
cargo build
mvn install
mvn -f examples/workflow-checkout/pom.xml generate-resources
./target/debug/vitrifi workflow build --root examples/workflow-checkout
./target/debug/vitrifi workflow verify --root examples/workflow-checkout -- mvn test
./target/debug/vitrifi generate --root examples/workflow-checkout
```

The H2 scenarios cover acceptance, another owner, invalid coupons, replacement, read-only preview, a failed database write, and a native leaf throwing the rejection exception. Only the explicit workflow rejection exercises the business-rejection policy. See the [language guide](../../docs/workflows.md) for syntax, semantics and limitations.

Compare [spring-checkout](../spring-checkout) for the earlier service with handwritten Java orchestration. Move `save` ahead of a rejecting check to see the workflow compiler fail before updating the generated Java; add a hidden write inside a supposedly read-only action to see native effect validation fail.
