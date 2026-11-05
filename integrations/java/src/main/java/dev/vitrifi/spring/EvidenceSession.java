package dev.vitrifi.spring;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import java.io.IOException;
import java.nio.channels.FileChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/** Explicit scenario runner; catching a business exception never removes a violation. */
public final class EvidenceSession implements AutoCloseable {
    private static final Object FILE_MUTEX = new Object();
    @FunctionalInterface
    public interface Scenario {
        void run() throws Exception;
    }

    private final VitrifiRuntime runtime;
    private final Path traceFile;
    private final String sessionId;
    private final long ownerThread = Thread.currentThread().threadId();
    private final Map<String, VitrifiRuntime.Policy> policies = new LinkedHashMap<>();
    private final List<Map<String, Object>> scenarios = new ArrayList<>();
    private final List<VitrifiRuntime.Invocation> invocations = new ArrayList<>();
    private final List<Map<String, Object>> violations = new ArrayList<>();
    private final List<Map<String, Object>> gaps = new ArrayList<>();
    private String currentScenario;
    private Map<String, Object> finished;

    EvidenceSession(VitrifiRuntime runtime, Path traceFile, List<VitrifiRuntime.Policy> knownPolicies) {
        this.runtime = runtime;
        this.traceFile = traceFile;
        String supplied = System.getenv("VITRIFI_SESSION_ID");
        this.sessionId = supplied == null || supplied.isBlank() ? UUID.randomUUID().toString() : supplied;
        knownPolicies.forEach(this::registerPolicy);
    }

    synchronized void registerPolicy(VitrifiRuntime.Policy policy) {
        policies.putIfAbsent(policy.key(), policy);
    }

    boolean onOwnerThread() {
        return ownerThread == Thread.currentThread().threadId();
    }

    synchronized String currentScenario() {
        return currentScenario;
    }

    /** Execute ordinary test assertions as one named scenario. */
    public void scenario(String name, Scenario body) throws Exception {
        synchronized (this) {
            if (finished != null || currentScenario != null || !onOwnerThread()) {
                throw new IllegalStateException("Scenarios must run sequentially on the session thread");
            }
            if (name == null || name.isBlank()) {
                throw new IllegalArgumentException("A scenario needs a nonempty name");
            }
            currentScenario = name;
        }
        String outcome = "passed";
        String errorType = null;
        try {
            body.run();
        } catch (Exception | Error failure) {
            outcome = "failed";
            errorType = failure.getClass().getName();
            throw failure;
        } finally {
            synchronized (this) {
                scenarios.add(VitrifiRuntime.map("name", name, "outcome", outcome, "error_type", errorType));
                currentScenario = null;
            }
        }
    }

    synchronized void invocation(VitrifiRuntime.Invocation invocation) {
        invocations.add(invocation);
        if (invocation.scenario == null) {
            gap(invocation, "Feature invocation was not attached to a named verification scenario");
        }
    }

    synchronized void violation(VitrifiRuntime.Invocation invocation, VitrifiRuntime.Policy policy, String message) {
        violations.add(VitrifiRuntime.map("invocation_id", invocation.id, "feature", invocation.feature,
                "method", invocation.method, "kind", policy.kind(), "action", policy.action(),
                "resource", policy.resource(), "message", message));
    }

    synchronized void gap(VitrifiRuntime.Invocation invocation, String message) {
        gaps.add(VitrifiRuntime.map("invocation_id", invocation == null ? null : invocation.id, "message", message));
    }

    void writeInitialMarker() {
        if (traceFile != null) {
            // A trace belongs to exactly one session. Another test class/JVM must not
            // erase a failed session by later writing a clean result to the same file.
            synchronized (FILE_MUTEX) {
                try {
                    Files.createDirectories(traceFile.toAbsolutePath().getParent());
                    try {
                        Files.createFile(claimFile());
                    } catch (FileAlreadyExistsException duplicate) {
                        poisonDuplicateSession();
                        throw new IllegalStateException("Only one Vitrifi EvidenceSession may use a trace destination");
                    }
                    if (Files.exists(traceFile)) {
                        poisonDuplicateSession();
                        throw new IllegalStateException("Vitrifi requires a fresh trace destination");
                    }
                } catch (IOException failure) {
                    throw new IllegalStateException("Cannot claim Vitrifi verification trace destination", failure);
                }
            }
        }
        write(VitrifiRuntime.map("schema_version", 1, "session_id", sessionId,
                "complete", false, "status", "incomplete", "reason", "Verification session has not completed"));
    }

    /** Complete and persist the report without throwing on a policy failure. */
    public synchronized Map<String, Object> finish() {
        if (finished != null) {
            return finished;
        }
        if (!onOwnerThread()) {
            throw new IllegalStateException("Finish the evidence session on its owner thread");
        }
        if (currentScenario != null) {
            gap(null, "Evidence session ended before the active scenario completed");
        }
        runtime.endSession(this);
        List<Map<String, Object>> coverage = new ArrayList<>();
        boolean untested = false;
        for (VitrifiRuntime.Policy policy : policies.values()) {
            boolean violated = invocations.stream().anyMatch(invocation -> invocation.violated.contains(policy.key()));
            boolean tested = invocations.stream().anyMatch(invocation -> invocation.policies.contains(policy)
                    && (!policy.kind().equals("no_effect_on_error") || "threw".equals(invocation.outcome)));
            String status = violated ? "violated" : tested ? "passed" : "untested";
            untested |= !tested;
            coverage.add(policy.describe(status));
        }
        if (scenarios.isEmpty()) {
            gap(null, "No verification scenarios ran");
        }
        if (invocations.isEmpty()) {
            gap(null, "No feature invocation was observed");
        }
        boolean failed = !violations.isEmpty() || scenarios.stream().anyMatch(scenario -> "failed".equals(scenario.get("outcome")));
        String status = failed ? "failed" : untested || !gaps.isEmpty() ? "incomplete" : "passed";
        finished = VitrifiRuntime.map("schema_version", 1, "session_id", sessionId,
                "complete", true, "status", status, "scenarios", List.copyOf(scenarios),
                "invocations", invocations.stream().map(VitrifiRuntime.Invocation::describe).toList(),
                "policy_coverage", coverage, "violations", List.copyOf(violations), "gaps", List.copyOf(gaps),
                "scope", "Synchronous Spring-proxied feature calls and instrumented JDBC statement execution attempts; not a proof over all inputs or durable database state");
        write(finished);
        return finished;
    }

    /** Complete the report, then make incomplete evidence or policy failures fail the test run. */
    public void verify() {
        Map<String, Object> report = finish();
        if (!"passed".equals(report.get("status"))) {
            throw new AssertionError("Vitrifi runtime verification " + report.get("status")
                    + ": " + violations.size() + " violation(s), " + gaps.size() + " gap(s); inspect policy_coverage for untested paths");
        }
    }

    @Override
    public void close() {
        verify();
    }

    private void write(Map<String, Object> report) {
        if (traceFile == null) {
            return;
        }
        synchronized (FILE_MUTEX) {
            try (FileChannel claim = FileChannel.open(claimFile(), StandardOpenOption.READ, StandardOpenOption.WRITE);
                    var ignored = claim.lock()) {
                if (claim.size() != 0) {
                    report.put("complete", false);
                    report.put("status", "incomplete");
                    report.put("reason", "Multiple sessions attempted to use this trace destination");
                }
                writeAtomic(report);
            } catch (IOException failure) {
                throw new IllegalStateException("Cannot persist Vitrifi verification evidence", failure);
            }
        }
    }

    private Path claimFile() {
        Path target = traceFile.toAbsolutePath();
        return target.resolveSibling("." + target.getFileName() + ".vitrifi-session");
    }

    private void poisonDuplicateSession() throws IOException {
        try (FileChannel claim = FileChannel.open(claimFile(), StandardOpenOption.READ, StandardOpenOption.WRITE);
                var ignored = claim.lock()) {
            claim.truncate(0);
            claim.write(StandardCharsets.UTF_8.encode("conflict"));
            claim.force(true);
            writeAtomic(VitrifiRuntime.map("schema_version", 1, "session_id", sessionId,
                    "complete", false, "status", "incomplete", "reason",
                    "Multiple sessions attempted to use this trace destination"));
        }
    }

    private void writeAtomic(Map<String, Object> report) {
        Path target = traceFile.toAbsolutePath();
        Path temporary = null;
        try {
            Files.createDirectories(target.getParent());
            temporary = Files.createTempFile(target.getParent(), ".vitrifi-session-", ".json");
            new ObjectMapper().enable(SerializationFeature.INDENT_OUTPUT).writeValue(temporary.toFile(), report);
            try {
                Files.move(temporary, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException unavailable) {
                Files.move(temporary, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException failure) {
            throw new IllegalStateException("Cannot persist Vitrifi verification evidence", failure);
        } finally {
            if (temporary != null) {
                try {
                    Files.deleteIfExists(temporary);
                } catch (IOException ignored) {
                    // A write failure was already surfaced; no success marker is fabricated.
                }
            }
        }
    }
}
