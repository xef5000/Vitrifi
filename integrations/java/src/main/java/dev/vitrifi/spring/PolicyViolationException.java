package dev.vitrifi.spring;

/** Raised before a JDBC operation forbidden by an active feature policy executes. */
public final class PolicyViolationException extends RuntimeException {
    public PolicyViolationException(String message) {
        super(message);
    }
}
