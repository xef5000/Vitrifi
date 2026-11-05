"""The workflow does not know the cart's persistence representation."""

from domain.discounts import CouponRejected, apply_coupon


def apply_discount(actor_id: str, cart_id: str, code: str) -> dict:
    try:
        receipt = apply_coupon(actor_id, cart_id, code)
    except CouponRejected as error:
        return {"accepted": False, "error": str(error)}
    return {
        "accepted": True,
        "cartId": receipt.cart_id,
        "couponCode": receipt.coupon_code,
        "discountCents": receipt.discount_cents,
        "totalCents": receipt.total_cents,
    }
