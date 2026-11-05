"""Validate every rejection condition before the single persistence write."""

from dataclasses import dataclass, replace

from storage.repository import load_cart, load_coupon, save_cart


class CouponRejected(ValueError):
    """A rejected request leaves the cart unchanged."""


@dataclass(frozen=True)
class DiscountReceipt:
    cart_id: str
    coupon_code: str
    discount_cents: int
    total_cents: int


def apply_coupon(actor_id: str, cart_id: str, code: str) -> DiscountReceipt:
    cart = load_cart(cart_id)
    if cart is None or cart.owner_id != actor_id:
        raise CouponRejected("Cart is unavailable to this customer")
    if cart.subtotal_cents < 0:
        raise CouponRejected("Cart subtotal cannot be negative")
    if cart.coupon_code is not None:
        raise CouponRejected("Only one coupon may be applied")

    coupon = load_coupon(code.strip().upper())
    if coupon is None or not coupon.active or coupon.amount_cents <= 0:
        raise CouponRejected("Coupon is invalid or inactive")

    discount = min(cart.subtotal_cents, coupon.amount_cents)
    updated = replace(cart, discount_cents=discount, coupon_code=coupon.code)
    save_cart(updated)
    return DiscountReceipt(
        cart_id=updated.id,
        coupon_code=coupon.code,
        discount_cents=discount,
        total_cents=updated.total_cents,
    )
