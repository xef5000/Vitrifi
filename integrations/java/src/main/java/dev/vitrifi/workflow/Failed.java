package dev.vitrifi.workflow;

/** An action failed. This is never a business rejection, even if its cause was one. */
public final class Failed extends RuntimeException {
    private final String action;
    public Failed(String action, Throwable cause) {
        super("Action failed: " + action, cause);
        this.action = action;
    }
    public String action() { return action; }
}
