package audit;

import java.util.ArrayList;
import java.util.List;

/** Independent audit-service seam. Network delivery is outside this fixture. */
public final class DiscountAudit {
    private final List<String> events = new ArrayList<>();

    public void record(String cartId, String couponCode, int discountCents) {
        if (discountCents < 0) {
            throw new IllegalArgumentException("Discount cannot be negative");
        }
        events.add(cartId + ":" + couponCode + ":" + discountCents);
    }

    public List<String> entries() {
        return List.copyOf(events);
    }
}
