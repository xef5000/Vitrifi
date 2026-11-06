package dev.vitrifi.spring;

import dev.vitrifi.api.Forbid;
import dev.vitrifi.api.NoEffectOnError;
import dev.vitrifi.api.SqlClassifier;
import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Deque;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicLong;
import java.util.stream.Collectors;

/**
 * Synchronous Spring/JDBC policy monitor. The observed effect is a JDBC statement
 * execution attempt, not an assertion about the database's durable state.
 */
public final class VitrifiRuntime {
    @FunctionalInterface
    interface InvocationBody {
        Object call() throws Throwable;
    }

    record Policy(String feature, String method, String kind, String action, String resource) {
        String key() {
            return feature + "::" + method + "::" + kind + "::" + action + "::" + resource;
        }

        Map<String, Object> describe(String status) {
            return map("feature", feature, "method", method, "kind", kind,
                    "action", action, "resource", resource, "status", status);
        }
    }

    static final class JdbcEvent {
        final String action;
        final String resource;
        final String operation;
        final String table;
        boolean attempted;
        String outcome = "blocked";

        JdbcEvent(SqlClassifier.Effect effect, String resource) {
            this.action = effect.action();
            this.resource = resource;
            this.operation = effect.operation();
            this.table = effect.table();
        }

        Map<String, Object> describe() {
            return map("action", action, "resource", resource, "operation", operation,
                    "table", table, "outcome", outcome, "attempted", attempted);
        }
    }

    static final class Invocation {
        final String id;
        final String feature;
        final String method;
        final long threadId = Thread.currentThread().threadId();
        final String threadName = Thread.currentThread().getName();
        final List<Policy> policies;
        final List<JdbcEvent> events = new ArrayList<>();
        final Set<String> violated = new LinkedHashSet<>();
        final EvidenceSession session;
        final String scenario;
        String outcome;
        String errorType;

        Invocation(String id, String feature, String method, List<Policy> policies, EvidenceSession session) {
            this.id = id;
            this.feature = feature;
            this.method = method;
            this.policies = policies;
            this.session = session;
            this.scenario = session == null ? null : session.currentScenario();
        }

        Map<String, Object> describe() {
            List<Map<String, Object>> results = new ArrayList<>();
            for (Policy policy : policies) {
                String status = violated.contains(policy.key()) ? "violated"
                        : policy.kind().equals("no_effect_on_error") && !outcome.equals("threw") ? "untested" : "passed";
                results.add(policy.describe(status));
            }
            return map("id", id, "feature", feature, "method", method,
                    "thread_id", threadId, "thread_name", threadName, "scenario", scenario,
                    "outcome", outcome, "error_type", errorType,
                    "events", events.stream().map(JdbcEvent::describe).toList(), "policies", results);
        }
    }

    private final Map<String, String> resources = new LinkedHashMap<>();
    private final Map<String, Policy> knownPolicies = new LinkedHashMap<>();
    private final Set<String> instrumentationGaps = new LinkedHashSet<>();
    private final ThreadLocal<Deque<Invocation>> scopes = ThreadLocal.withInitial(ArrayDeque::new);
    private final AtomicLong invocationIds = new AtomicLong();
    private volatile EvidenceSession activeSession;

    synchronized void registerTable(String rawTable, String resource) {
        String table = SqlClassifier.normalizeTable(rawTable);
        String previous = resources.putIfAbsent(table, resource);
        if (table.isBlank() || resource.isBlank()) {
            instrumentationGap("ResourceTable must contain a table and resource name");
        } else if (previous != null && !previous.equals(resource)) {
            instrumentationGap("A database table has conflicting ResourceTable mappings: " + table);
        }
    }

    synchronized void registerMethod(String feature, Method method) {
        for (Policy policy : policies(feature, method)) {
            knownPolicies.putIfAbsent(policy.key(), policy);
            if (!Set.of("read", "write").contains(policy.action())) {
                instrumentationGap("JDBC runtime supports only read/write policies: " + policy.method());
            }
            if (activeSession != null) {
                activeSession.registerPolicy(policy);
            }
        }
    }

    synchronized void instrumentationGap(String message) {
        instrumentationGaps.add(message);
        if (activeSession != null) {
            activeSession.gap(null, message);
        }
    }

    /** Start a checked scenario session, optionally persisted through VITRIFI_TRACE_FILE. */
    public synchronized EvidenceSession startSession() {
        String target = System.getenv("VITRIFI_TRACE_FILE");
        return startSession(target == null || target.isBlank() ? null : Path.of(target));
    }

    /** Explicit path variant for integration tests and embedded callers. */
    public synchronized EvidenceSession startSession(Path traceFile) {
        if (activeSession != null) {
            throw new IllegalStateException("A Vitrifi evidence session is already active");
        }
        if (!scopes.get().isEmpty()) {
            throw new IllegalStateException("Start the evidence session before invoking a feature");
        }
        EvidenceSession session = new EvidenceSession(this, traceFile, new ArrayList<>(knownPolicies.values()));
        activeSession = session;
        for (String gap : instrumentationGaps) {
            session.gap(null, gap);
        }
        for (Policy policy : knownPolicies.values()) {
            if (!resources.containsValue(policy.resource())) {
                session.gap(null, "Policy resource has no ResourceTable mapping: " + policy.resource());
            }
        }
        try {
            session.writeInitialMarker();
        } catch (RuntimeException failure) {
            activeSession = null;
            throw failure;
        }
        return session;
    }

    synchronized void endSession(EvidenceSession session) {
        if (activeSession != session) {
            throw new IllegalStateException("Evidence session is not active");
        }
        if (!scopes.get().isEmpty()) {
            session.gap(null, "Evidence session ended while a feature invocation was active");
        }
        activeSession = null;
    }

    Object invoke(String feature, Method method, InvocationBody body) throws Throwable {
        EvidenceSession session = activeSession;
        if (session != null && !session.onOwnerThread()) {
            session.gap(null, "Feature execution crossed the supported single-thread session boundary");
        }
        Invocation invocation = new Invocation("call-" + invocationIds.incrementAndGet(), feature,
                methodId(method), policies(feature, method), session);
        scopes.get().push(invocation);
        try {
            Object result = body.call();
            invocation.outcome = "returned";
            return result;
        } catch (Throwable failure) {
            invocation.outcome = "threw";
            invocation.errorType = failure.getClass().getName();
            for (Policy policy : invocation.policies) {
                if (policy.kind().equals("no_effect_on_error") && invocation.events.stream().anyMatch(
                        event -> event.attempted && matches(policy, event))) {
                    violation(invocation, policy, "The feature exited exceptionally after a matching JDBC execution attempt");
                }
            }
            // Preserve business exception behavior. The recorded violation remains even
            // when the scenario catches this exception and its ordinary assertions pass.
            throw failure;
        } finally {
            scopes.get().pop();
            if (scopes.get().isEmpty()) {
                scopes.remove();
            }
            if (session != null) {
                session.invocation(invocation);
            }
        }
    }

    List<JdbcEvent> prepareSql(List<String> statements) {
        Deque<Invocation> current = scopes.get();
        if (current.isEmpty()) {
            EvidenceSession session = activeSession;
            if (session != null && !session.onOwnerThread()) {
                session.gap(null, "JDBC execution occurred on a thread outside the synchronous feature scope");
                throw new IncompleteInstrumentationException("Unscoped asynchronous JDBC execution is unsupported");
            }
            return List.of();
        }
        List<JdbcEvent> events = new ArrayList<>();
        for (String sql : statements) {
            SqlClassifier.Effect effect;
            try {
                effect = SqlClassifier.classify(sql);
            } catch (IllegalArgumentException unknown) {
                unsupported(unknown.getMessage());
                throw new IncompleteInstrumentationException(unknown.getMessage());
            }
            String resource;
            synchronized (this) {
                resource = resources.get(effect.table());
            }
            if (resource == null) {
                String message = "SQL target has no ResourceTable mapping: " + effect.table();
                unsupported(message);
                throw new IncompleteInstrumentationException(message);
            }
            JdbcEvent event = new JdbcEvent(effect, resource);
            events.add(event);
            for (Invocation invocation : current) {
                invocation.events.add(event);
            }
            boolean blocked = false;
            for (Invocation invocation : current) {
                for (Policy policy : invocation.policies) {
                    if (policy.kind().equals("forbid") && matches(policy, event)) {
                        blocked = true;
                        violation(invocation, policy, "A forbidden JDBC operation was blocked before execution");
                    }
                }
            }
            if (blocked) {
                throw new PolicyViolationException("Forbidden " + event.action + " on resource " + event.resource);
            }
        }
        return events;
    }

    void unsupported(String message) {
        Set<EvidenceSession> notified = new LinkedHashSet<>();
        for (Invocation invocation : scopes.get()) {
            if (invocation.session != null && notified.add(invocation.session)) {
                invocation.session.gap(invocation, message);
            }
        }
    }

    boolean inFeatureScope() {
        return !scopes.get().isEmpty();
    }

    private void violation(Invocation invocation, Policy policy, String message) {
        if (invocation.violated.add(policy.key()) && invocation.session != null) {
            invocation.session.violation(invocation, policy, message);
        }
    }

    private static boolean matches(Policy policy, JdbcEvent event) {
        return policy.action().equals(event.action) && policy.resource().equals(event.resource);
    }

    private static List<Policy> policies(String feature, Method method) {
        List<Policy> policies = new ArrayList<>();
        for (Forbid annotation : method.getAnnotationsByType(Forbid.class)) {
            policies.add(new Policy(feature, methodId(method), "forbid", annotation.action(), annotation.resource()));
        }
        for (NoEffectOnError annotation : method.getAnnotationsByType(NoEffectOnError.class)) {
            policies.add(new Policy(feature, methodId(method), "no_effect_on_error", annotation.action(), annotation.resource()));
        }
        return policies;
    }

    static String methodId(Method method) {
        return method.getDeclaringClass().getCanonicalName() + "." + method.getName() + "("
                + Arrays.stream(method.getParameterTypes()).map(Class::getCanonicalName).collect(Collectors.joining(",")) + ")";
    }

    static Map<String, Object> map(Object... pairs) {
        Map<String, Object> result = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            result.put((String) pairs[i], pairs[i + 1]);
        }
        return result;
    }
}
