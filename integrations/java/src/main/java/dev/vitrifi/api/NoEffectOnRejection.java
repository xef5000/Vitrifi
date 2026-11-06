package dev.vitrifi.api;

import java.lang.annotation.ElementType;
import java.lang.annotation.Repeatable;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/** Runtime policy: an invocation that throws dev.vitrifi.workflow.Rejected must have attempted no such effect. */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.METHOD)
@Repeatable(NoEffectsOnRejection.class)
public @interface NoEffectOnRejection {
    String action();
    String resource();
}
