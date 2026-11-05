package dev.vitrifi.example.checkout;

public class CouponRejected extends RuntimeException {
    public CouponRejected(String message) {
        super(message);
    }
}
