package dev.vitrifi.example.checkout;

import dev.vitrifi.api.Feature;
import dev.vitrifi.api.Forbid;
import dev.vitrifi.api.NoEffectOnError;
import org.springframework.stereotype.Service;

@Service
@Feature(value = "Checkout.Discounts", purpose = "Apply one valid coupon to the owner's cart without writes on rejection")
public class DiscountService {
    private final CartRepository carts;

    public DiscountService(CartRepository carts) {
        this.carts = carts;
    }

    @NoEffectOnError(action = "write", resource = "Carts")
    public Receipt applyCoupon(String actorId, String cartId, String code) {
        Cart cart = carts.load(cartId);
        if (!cart.ownerId().equals(actorId)) {
            throw new CouponRejected("Only the cart owner can apply a coupon");
        }
        if (!"SAVE10".equals(code)) {
            throw new CouponRejected("Coupon is invalid");
        }
        if (cart.coupon() != null) {
            throw new CouponRejected("A coupon is already applied");
        }
        int total = Math.max(0, cart.total() - 1000);
        carts.save(cart.id(), total, code);
        return new Receipt(cart.id(), code, total);
    }

    @Forbid(action = "write", resource = "Carts")
    public Cart preview(String cartId) {
        return carts.load(cartId);
    }
}
