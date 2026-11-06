package dev.vitrifi.spring;

import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Role;
import org.springframework.beans.factory.config.BeanDefinition;

/** Import into a Spring application or integration-test context to enable the pilot. */
@Configuration(proxyBeanMethods = false)
public class VitrifiSpringConfiguration {
    @Bean
    @Role(BeanDefinition.ROLE_INFRASTRUCTURE)
    public static VitrifiRuntime vitrifiRuntime() {
        return new VitrifiRuntime();
    }

    @Bean
    @Role(BeanDefinition.ROLE_INFRASTRUCTURE)
    public static VitrifiBeanPostProcessor vitrifiBeanPostProcessor(VitrifiRuntime runtime) {
        return new VitrifiBeanPostProcessor(runtime);
    }
}
