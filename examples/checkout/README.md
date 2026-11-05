# Checkout: a feature-language experiment

New to the syntax? Start with [Reading and writing Vitrifi](../../docs/reading-vitrifi.md), which explains this example one declaration at a time. Keep the [syntax reference](../../docs/language.md) handy for field and visibility rules.

Read `discounts.vitrifi` first. This one file describes the complete discount feature across the JavaScript form, TypeScript transport, and Python workflow/domain: coupon policy, operations, call relationships, resource effects, internal boundaries, and acceptance checks. Follow its import to `storage/cart.vitrifi` only when you need the shared storage interface.

The optional audit feature is separate: `audit/discount.vitrifi` owns the Ruby delivery worker and Java audit service, with their relationship declared in the same file. The example has three feature files, with no central feature registry.

The Python workflow/domain/storage path is executable. The JavaScript/TypeScript path and the Ruby/Java audit feature are integration seams. Their injected clients and cross-process delivery remain the host application's responsibility; this fixture does not deploy them as a distributed system or wire audit delivery into the discount flow.

## Check and inspect from the repository root

```sh
cargo run -- check --root examples/checkout
cargo run -- inspect examples/checkout/discounts.vitrifi
cargo run -- verify --root examples/checkout
```

Checking a single entry follows its imports:

```sh
cargo run -- check examples/checkout/discounts.vitrifi
```

This loads the complete discount feature and shared storage capability. Repository-wide checking also includes the optional audit feature.

## Try a declared-effect violation

Open `discounts.vitrifi`. `applyCoupon` declares `calls [Store.load, Store.loadCoupon, Store.save]`. The imported `Store.save` operation declares `write Carts`.

1. Remove `write Store.Carts` from `applyCoupon`'s effects.
2. Run the discount entry's `check` command.
3. The compiler reports that the caller omits an effect exposed by a declared callee.
4. Restore the effect and check again.

The caller chain `submitCoupon → handleCoupon → applyDiscount → applyCoupon` is also visible in this file. Each caller exposes the resource effects of the operation it invokes. This checks the consistency of the architecture program; it does not infer or prove every effect of a host body.

## Try a host boundary violation

The same feature file declares `Workflow` and `Domain` boundaries. The workflow may import domain, and domain may import the shared storage boundary. In `workflow/apply_discount.py`, add an import after the existing imports:

```python
from storage.repository import load_cart
```

Run the repository-wide check. The direct host dependency crosses a forbidden boundary, even if the import is unused. Remove it and check again. The workflow can call the feature's `applyCoupon` operation while continuing to expose cart effects in its contract.

## Review a product change

Consider “allow customers to replace an applied coupon.” The first review location is `discounts.vitrifi`: `SingleCoupon` currently promises that an existing coupon cannot be replaced.

Implementing replacement requires reviewing that requirement, the domain implementation, and the acceptance tests together. The same feature file also exposes the path through browser, transport, and workflow, so the reviewer can assess the change without reconstructing the feature from separate layer definitions. Use an existing Git base to compare declared architecture:

```sh
cargo run -- review --base HEAD
```

The normal Git diff remains available. Merely weakening a requirement to silence a checker is a visible policy change, not a successful proof.

## Behavioral checks

`Checkout.Discounts.Behavior` runs seven Python `unittest` cases from this directory:

```sh
cd examples/checkout
python3 -m unittest discover -s tests -v
```

They exercise ownership, rejection without writes, the nonnegative total, the single-coupon rule, and workflow responses. `verify` runs the same declared command explicitly. Passing is evidence for those scenarios; it does not prove the promises for every input.

The host must authenticate the actor id. The in-memory store does not provide transactions or concurrency guarantees. Audit and UI requirements have no attached behavioral check in this fixture, so they remain authored claims. Retry, delivery, and deduplication questions remain visible in the audit feature.
