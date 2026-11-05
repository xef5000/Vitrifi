package dev.vitrifi.example.checkout;

public interface CartRepository {
    Cart load(String cartId);
    void save(String cartId, int total, String coupon);
}
