package dev.vitrifi.example.checkout;

import dev.vitrifi.api.ResourceTable;
import org.springframework.jdbc.core.JdbcTemplate;
import org.springframework.stereotype.Repository;

@Repository
@ResourceTable(value = "Carts", table = "carts")
public class JdbcCartRepository implements CartRepository {
    private final JdbcTemplate jdbc;

    public JdbcCartRepository(JdbcTemplate jdbc) {
        this.jdbc = jdbc;
    }

    @Override
    public Cart load(String cartId) {
        return jdbc.queryForObject(
                "SELECT id, owner_id, total, coupon FROM carts WHERE id = ?",
                (rows, number) -> new Cart(rows.getString("id"), rows.getString("owner_id"),
                        rows.getInt("total"), rows.getString("coupon")),
                cartId);
    }

    @Override
    public void save(String cartId, int total, String coupon) {
        jdbc.update("UPDATE carts SET total = ?, coupon = ? WHERE id = ?", total, coupon, cartId);
    }
}
