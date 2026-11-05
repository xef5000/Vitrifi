package dev.vitrifi.example.checkout;

import dev.vitrifi.spring.EvidenceSession;
import dev.vitrifi.spring.VitrifiRuntime;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestInstance;
import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.jdbc.core.JdbcTemplate;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/** The rejection scenarios intentionally do not assert a write count. Vitrifi does. */
@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class DiscountServiceTest {
    private AnnotationConfigApplicationContext context;
    private JdbcTemplate jdbc;
    private DiscountService discounts;
    private EvidenceSession evidence;

    @BeforeAll
    void start() {
        context = new AnnotationConfigApplicationContext(CheckoutConfiguration.class);
        jdbc = context.getBean(JdbcTemplate.class);
        discounts = context.getBean(DiscountService.class);
        jdbc.execute("CREATE TABLE IF NOT EXISTS carts (id VARCHAR(30) PRIMARY KEY, owner_id VARCHAR(30), total INT, coupon VARCHAR(30))");
        evidence = context.getBean(VitrifiRuntime.class).startSession();
    }

    @BeforeEach
    void resetCart() {
        jdbc.update("DELETE FROM carts");
        jdbc.update("INSERT INTO carts (id, owner_id, total, coupon) VALUES (?, ?, ?, ?)", "cart-1", "alice", 2500, null);
    }

    @AfterAll
    void verify() {
        try {
            if (evidence != null) {
                evidence.verify();
            }
        } finally {
            if (context != null) {
                context.close();
            }
        }
    }

    @Test
    void ownerCanApplyCoupon() throws Exception {
        evidence.scenario("owner applies a valid coupon", () -> {
            Receipt receipt = discounts.applyCoupon("alice", "cart-1", "SAVE10");
            assertEquals(1500, receipt.total());
            assertEquals("SAVE10", discounts.preview("cart-1").coupon());
        });
    }

    @Test
    void rejectsAnotherOwner() throws Exception {
        evidence.scenario("reject another cart owner", () ->
                assertThrows(CouponRejected.class, () -> discounts.applyCoupon("bob", "cart-1", "SAVE10")));
    }

    @Test
    void rejectsInvalidCoupon() throws Exception {
        evidence.scenario("reject an invalid coupon", () ->
                assertThrows(CouponRejected.class, () -> discounts.applyCoupon("alice", "cart-1", "INVALID")));
    }

    @Test
    void rejectsReplacement() throws Exception {
        evidence.scenario("reject coupon replacement", () -> {
            discounts.applyCoupon("alice", "cart-1", "SAVE10");
            assertThrows(CouponRejected.class, () -> discounts.applyCoupon("alice", "cart-1", "SAVE10"));
        });
    }

    @Test
    void previewReadsWithoutWriting() throws Exception {
        evidence.scenario("preview the cart", () -> assertEquals(2500, discounts.preview("cart-1").total()));
    }
}
