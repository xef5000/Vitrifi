"""A deliberately small persistence boundary; no database is required."""

from dataclasses import dataclass, replace


@dataclass(frozen=True)
class Cart:
    id: str
    owner_id: str
    subtotal_cents: int
    discount_cents: int = 0
    coupon_code: str | None = None

    @property
    def total_cents(self) -> int:
        return self.subtotal_cents - self.discount_cents


@dataclass(frozen=True)
class Coupon:
    code: str
    amount_cents: int
    active: bool = True


_carts: dict[str, Cart] = {}
_coupons: dict[str, Coupon] = {}
_writes = 0


def seed(carts: list[Cart], coupons: list[Coupon]) -> None:
    """Reset isolated demonstration data without counting setup as a write."""
    global _carts, _coupons, _writes
    _carts = {cart.id: cart for cart in carts}
    _coupons = {coupon.code: coupon for coupon in coupons}
    _writes = 0


def load_cart(cart_id: str) -> Cart | None:
    cart = _carts.get(cart_id)
    return replace(cart) if cart else None


def load_coupon(code: str) -> Coupon | None:
    return _coupons.get(code)


def save_cart(cart: Cart) -> None:
    global _writes
    _carts[cart.id] = replace(cart)
    _writes += 1


def write_count() -> int:
    return _writes
