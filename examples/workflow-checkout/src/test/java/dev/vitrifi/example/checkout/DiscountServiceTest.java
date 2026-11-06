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
import static org.junit.jupiter.api.Assertions.assertTrue;

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
                assertThrows(dev.vitrifi.workflow.Rejected.class, () -> discounts.applyCoupon("bob", "cart-1", "SAVE10")));
    }

    @Test
    void rejectsInvalidCoupon() throws Exception {
        evidence.scenario("reject an invalid coupon", () ->
                assertThrows(dev.vitrifi.workflow.Rejected.class, () -> discounts.applyCoupon("alice", "cart-1", "INVALID")));
    }

    @Test
    void rejectsReplacement() throws Exception {
        evidence.scenario("reject coupon replacement", () -> {
            discounts.applyCoupon("alice", "cart-1", "SAVE10");
            assertThrows(dev.vitrifi.workflow.Rejected.class, () -> discounts.applyCoupon("alice", "cart-1", "SAVE10"));
        });
    }

    @Test
    void previewReadsWithoutWriting() throws Exception {
        evidence.scenario("preview the cart", () -> assertEquals(2500, discounts.preview("cart-1").total()));
    }
    @Test
    void failedDatabaseAttemptIsNotBusinessRejection() throws Exception {
        jdbc.execute("ALTER TABLE carts ADD CONSTRAINT fail_discount CHECK (total > 2000)");
        try {
            evidence.scenario("database failure after accepted validation", () -> {
                var failure = assertThrows(dev.vitrifi.workflow.Failed.class,
                        () -> discounts.applyCoupon("alice", "cart-1", "SAVE10"));
                assertEquals("save", failure.action());
                assertTrue(failure.getCause() instanceof org.springframework.dao.DataIntegrityViolationException);
            });
        } finally {
            jdbc.execute("ALTER TABLE carts DROP CONSTRAINT fail_discount");
        }
    }

    @Test
    void actionCannotMasqueradeAsWorkflowRejection() {
        var malicious = new CheckoutActions(null) {
            @Override public Cart load(String id) { throw new dev.vitrifi.workflow.Rejected("from a leaf"); }
        };
        var service = new DiscountService(malicious);
        var failure = assertThrows(dev.vitrifi.workflow.Failed.class, () -> service.preview("cart-1"));
        assertEquals("load", failure.action());
        assertTrue(failure.getCause() instanceof dev.vitrifi.workflow.Rejected);
    }
}
