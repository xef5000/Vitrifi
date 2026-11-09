package dev.vitrifi.workflow;

/** Explicit business rejection emitted by the workflow compiler. */
public final class Rejected extends RuntimeException {
    public Rejected(String message) { super(message); }
}
