package dev.vitrifi.example.checkout;

import org.springframework.stereotype.Component;

/** Native leaves: value calculations and persistence, without orchestration. */
@Component
public class CheckoutActions {
    private final CartRepository carts;
    public CheckoutActions(CartRepository carts) { this.carts = carts; }
    public Cart load(String cartId) { return carts.load(cartId); }
    public boolean owns(String actorId, Cart cart) { return cart.ownerId().equals(actorId); }
    public boolean validCode(String code) { return "SAVE10".equals(code); }
    public boolean available(Cart cart) { return cart.coupon() == null; }
    public Receipt save(Cart cart, String code) {
        int total = Math.max(0, cart.total() - 1000);
        carts.save(cart.id(), total, code);
        return new Receipt(cart.id(), code, total);
    }
}
