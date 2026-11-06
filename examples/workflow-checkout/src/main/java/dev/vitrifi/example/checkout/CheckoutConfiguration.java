package dev.vitrifi.example.checkout;

import dev.vitrifi.spring.VitrifiSpringConfiguration;
import javax.sql.DataSource;
import org.h2.jdbcx.JdbcDataSource;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Import;
import org.springframework.jdbc.core.JdbcTemplate;

@Configuration(proxyBeanMethods = false)
@Import({VitrifiSpringConfiguration.class, DiscountService.class, CheckoutActions.class, JdbcCartRepository.class})
public class CheckoutConfiguration {
    @Bean
    public DataSource dataSource() {
        JdbcDataSource source = new JdbcDataSource();
        source.setURL("jdbc:h2:mem:vitrifi-workflow-checkout;DB_CLOSE_DELAY=-1");
        return source;
    }

    @Bean
    public JdbcTemplate jdbcTemplate(DataSource source) {
        return new JdbcTemplate(source);
    }
}
