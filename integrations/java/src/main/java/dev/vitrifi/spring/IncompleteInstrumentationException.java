package dev.vitrifi.spring;

/** The integration cannot safely classify an operation in the supported runtime scope. */
public final class IncompleteInstrumentationException extends RuntimeException {
    public IncompleteInstrumentationException(String message) {
        super(message);
    }
}
