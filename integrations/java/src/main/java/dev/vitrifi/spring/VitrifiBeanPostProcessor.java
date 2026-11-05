package dev.vitrifi.spring;

import dev.vitrifi.api.Feature;
import dev.vitrifi.api.Forbid;
import dev.vitrifi.api.NoEffectOnError;
import dev.vitrifi.api.ResourceTable;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import javax.sql.DataSource;
import org.aopalliance.intercept.MethodInterceptor;
import org.springframework.aop.framework.ProxyFactory;
import org.springframework.aop.support.AopUtils;
import org.springframework.beans.factory.config.BeanPostProcessor;
import org.springframework.core.Ordered;
import org.springframework.core.annotation.AnnotatedElementUtils;

/** Instruments actual Spring instances, including interface-injected repositories. */
public final class VitrifiBeanPostProcessor implements BeanPostProcessor, Ordered {
    private final VitrifiRuntime runtime;

    VitrifiBeanPostProcessor(VitrifiRuntime runtime) {
        this.runtime = runtime;
    }

    @Override
    public int getOrder() {
        return Ordered.LOWEST_PRECEDENCE;
    }

    @Override
    public Object postProcessAfterInitialization(Object bean, String beanName) {
        if (bean instanceof DataSource source) {
            return JdbcInstrumentation.wrap(source, runtime);
        }
        Class<?> targetClass = AopUtils.getTargetClass(bean);
        ResourceTable table = AnnotatedElementUtils.findMergedAnnotation(targetClass, ResourceTable.class);
        if (table != null) {
            runtime.registerTable(table.table(), table.value());
        }
        Feature feature = AnnotatedElementUtils.findMergedAnnotation(targetClass, Feature.class);
        if (feature == null) {
            feature = targetClass.getPackage().getAnnotation(Feature.class);
        }
        if (feature == null) {
            return bean;
        }
        String featureName = feature.value();
        for (Class<?> owner = targetClass; owner != null && owner != Object.class; owner = owner.getSuperclass()) {
            for (Method method : owner.getDeclaredMethods()) {
                if (!Modifier.isPublic(method.getModifiers()) && (method.getAnnotationsByType(Forbid.class).length > 0
                        || method.getAnnotationsByType(NoEffectOnError.class).length > 0)) {
                    runtime.registerMethod(featureName, method);
                    runtime.instrumentationGap("Nonpublic policy method cannot be intercepted by Spring proxy: " + VitrifiRuntime.methodId(method));
                }
            }
        }
        for (Method method : targetClass.getMethods()) {
            if (method.getDeclaringClass() != Object.class && !method.isSynthetic()) {
                runtime.registerMethod(featureName, method);
                if (Modifier.isFinal(method.getModifiers()) || Modifier.isStatic(method.getModifiers())) {
                    runtime.instrumentationGap("Feature method cannot be intercepted by Spring proxy: " + VitrifiRuntime.methodId(method));
                }
                if (java.util.concurrent.Future.class.isAssignableFrom(method.getReturnType())
                        || java.util.concurrent.CompletionStage.class.isAssignableFrom(method.getReturnType())
                        || method.isAnnotationPresent(org.springframework.scheduling.annotation.Async.class)
                        || targetClass.isAnnotationPresent(org.springframework.scheduling.annotation.Async.class)) {
                    runtime.instrumentationGap("Asynchronous feature methods are outside the synchronous runtime scope: " + VitrifiRuntime.methodId(method));
                }
            }
        }
        if (Modifier.isFinal(targetClass.getModifiers())) {
            runtime.instrumentationGap("Final feature class cannot be intercepted by Spring proxy: " + targetClass.getName());
            return bean;
        }
        ProxyFactory proxy = new ProxyFactory(bean);
        proxy.setProxyTargetClass(true);
        proxy.addAdvice((MethodInterceptor) invocation -> {
            Method method = AopUtils.getMostSpecificMethod(invocation.getMethod(), targetClass);
            if (method.getDeclaringClass() == Object.class) {
                return invocation.proceed();
            }
            return runtime.invoke(featureName, method, invocation::proceed);
        });
        return proxy.getProxy(targetClass.getClassLoader());
    }
}
