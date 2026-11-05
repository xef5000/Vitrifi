import unittest

from domain.discounts import CouponRejected, apply_coupon
from storage.repository import Cart, Coupon, load_cart, seed, write_count
from workflow.apply_discount import apply_discount


class DiscountBehaviorTests(unittest.TestCase):
    def setUp(self):
        seed(
            [Cart("cart-1", "alice", 2500)],
            [Coupon("SAVE10", 1000), Coupon("BIGSAVE", 10000), Coupon("OLD", 500, False)],
        )

    def test_owner_receives_discount_and_one_write(self):
        receipt = apply_coupon("alice", "cart-1", " save10 ")
        self.assertEqual(receipt.total_cents, 1500)
        self.assertEqual(receipt.discount_cents, 1000)
        self.assertEqual(load_cart("cart-1").coupon_code, "SAVE10")
        self.assertEqual(write_count(), 1)

    def test_other_customer_cannot_change_cart(self):
        before = load_cart("cart-1")
        with self.assertRaises(CouponRejected):
            apply_coupon("mallory", "cart-1", "SAVE10")
        self.assertEqual(load_cart("cart-1"), before)
        self.assertEqual(write_count(), 0)

    def test_invalid_or_inactive_coupon_preserves_cart_without_writes(self):
        before = load_cart("cart-1")
        for code in ["MISSING", "OLD", ""]:
            with self.subTest(code=code), self.assertRaises(CouponRejected):
                apply_coupon("alice", "cart-1", code)
            self.assertEqual(load_cart("cart-1"), before)
            self.assertEqual(write_count(), 0)

    def test_discount_is_capped_at_subtotal(self):
        receipt = apply_coupon("alice", "cart-1", "BIGSAVE")
        self.assertEqual(receipt.total_cents, 0)
        self.assertEqual(receipt.discount_cents, 2500)

    def test_second_coupon_cannot_stack_or_replace_first(self):
        apply_coupon("alice", "cart-1", "SAVE10")
        before = load_cart("cart-1")
        with self.assertRaises(CouponRejected):
            apply_coupon("alice", "cart-1", "BIGSAVE")
        self.assertEqual(load_cart("cart-1"), before)
        self.assertEqual(write_count(), 1)

    def test_missing_cart_is_rejected_without_writes(self):
        with self.assertRaises(CouponRejected):
            apply_coupon("alice", "missing", "SAVE10")
        self.assertEqual(write_count(), 0)

    def test_workflow_exposes_success_and_rejection(self):
        rejected = apply_discount("mallory", "cart-1", "SAVE10")
        self.assertFalse(rejected["accepted"])
        accepted = apply_discount("alice", "cart-1", "SAVE10")
        self.assertTrue(accepted["accepted"])
        self.assertEqual(accepted["totalCents"], 1500)


if __name__ == "__main__":
    unittest.main()
