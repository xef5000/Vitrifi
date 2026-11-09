package dev.vitrifi.example.checkout;

public record Receipt(String cartId, String coupon, int total) {}
