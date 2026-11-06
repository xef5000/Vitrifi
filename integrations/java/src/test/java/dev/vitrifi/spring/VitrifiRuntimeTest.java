package dev.vitrifi.spring;

import com.fasterxml.jackson.databind.ObjectMapper;
import dev.vitrifi.api.Feature;
import dev.vitrifi.api.Forbid;
import dev.vitrifi.api.NoEffectOnError;
import dev.vitrifi.api.ResourceTable;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import javax.sql.DataSource;
import org.h2.jdbcx.JdbcDataSource;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.dao.DataIntegrityViolationException;
import org.springframework.jdbc.core.JdbcTemplate;
import static org.junit.jupiter.api.Assertions.*;

class VitrifiRuntimeTest {
    @TempDir
    Path temporary;

    @Configuration(proxyBeanMethods = false)
    static class Infrastructure {
        @Bean
        DataSource dataSource() {
            JdbcDataSource source = new JdbcDataSource();
            source.setURL("jdbc:h2:mem:" + UUID.randomUUID() + ";DB_CLOSE_DELAY=-1");
            return source;
        }

        @Bean
        JdbcTemplate jdbcTemplate(DataSource source) {
            return new JdbcTemplate(source);
        }

        @Bean
        CartMapping cartMapping() {
            return new CartMapping();
        }
    }

    @ResourceTable(value = "Carts", table = "carts")
    static class CartMapping {}

    @Feature("Test.Safe")
    public static class SafeService {
        private final JdbcTemplate jdbc;

        public SafeService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @NoEffectOnError(action = "write", resource = "Carts")
        public void apply(boolean reject) {
            if (reject) {
                throw new IllegalArgumentException("rejected");
            }
            jdbc.update("UPDATE carts SET total = ? WHERE id = ?", 12, "cart-1");
        }

        @Forbid(action = "write", resource = "Carts")
        public int preview() {
            return jdbc.queryForObject("SELECT total FROM carts WHERE id = ?", Integer.class, "cart-1");
        }
    }

    @Feature("Test.Mutant")
    public static class MutantService {
        private final JdbcTemplate jdbc;

        public MutantService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @NoEffectOnError(action = "write", resource = "Carts")
        public void apply(String privatePayload) {
            jdbc.update("UPDATE carts SET label = ? WHERE id = ?", privatePayload, "cart-1");
            throw new IllegalArgumentException(privatePayload);
        }
    }

    @Feature("Test.Forbidden")
    public static class ForbiddenService {
        private final JdbcTemplate jdbc;

        public ForbiddenService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @Forbid(action = "write", resource = "Carts")
        public void update() {
            jdbc.update("UPDATE carts SET total = ? WHERE id = ?", 99, "cart-1");
        }
    }

    @Feature("Test.Batch")
    public static class BatchService {
        private final JdbcTemplate jdbc;

        public BatchService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @Forbid(action = "write", resource = "Carts")
        public void update() {
            jdbc.batchUpdate("UPDATE carts SET total = 99", "UPDATE carts SET total = 100");
        }
    }

    @Feature("Test.FailedWrite")
    public static class FailedWriteService {
        private final JdbcTemplate jdbc;

        public FailedWriteService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @NoEffectOnError(action = "write", resource = "Carts")
        public void update() {
            jdbc.update("UPDATE carts SET total = ? WHERE id = ?", -1, "cart-1");
        }
    }

    @Feature("Test.Unknown")
    public static class UnknownService {
        private final JdbcTemplate jdbc;

        public UnknownService(JdbcTemplate jdbc) {
            this.jdbc = jdbc;
        }

        @Forbid(action = "write", resource = "Carts")
        public int query() {
            return jdbc.queryForObject("SELECT COUNT(*) FROM carts", Integer.class);
        }
    }

    @Feature("Test.Final")
    public static class FinalMethodService {
        @Forbid(action = "write", resource = "Carts")
        public final void unobservable() {}
    }

    @Test
    void realSpringCallsProduceCompleteEvidenceOnlyAfterSuccessfulScenarios() throws Exception {
        try (var context = context(SafeService.class)) {
            Path file = temporary.resolve("safe.json");
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(file);
            assertFalse(new ObjectMapper().readTree(file.toFile()).get("complete").asBoolean());
            SafeService service = context.getBean(SafeService.class);
            session.scenario("rejection", () -> assertThrows(IllegalArgumentException.class, () -> service.apply(true)));
            session.scenario("acceptance", () -> service.apply(false));
            session.scenario("preview", () -> assertEquals(12, service.preview()));
            session.verify();
            Map<String, Object> report = session.finish();
            assertEquals("passed", report.get("status"));
            assertEquals(true, report.get("complete"));
            assertEquals(3, list(report, "invocations").size());
            assertTrue(list(report, "policy_coverage").stream().allMatch(policy -> "passed".equals(policy.get("status"))));
            assertTrue(new ObjectMapper().readTree(file.toFile()).get("complete").asBoolean());
        }
    }

    @Test
    void aCaughtBusinessExceptionCannotHideAWriteBeforeValidation() throws Exception {
        try (var context = context(MutantService.class)) {
            Path file = temporary.resolve("mutant.json");
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(file);
            MutantService service = context.getBean(MutantService.class);
            session.scenario("ordinary rejection assertion", () -> assertThrows(IllegalArgumentException.class,
                    () -> service.apply("PRIVATE-CUSTOMER-PAYLOAD")));
            Map<String, Object> report = session.finish();
            assertEquals("passed", list(report, "scenarios").getFirst().get("outcome"));
            assertEquals("failed", report.get("status"));
            assertEquals("no_effect_on_error", list(report, "violations").getFirst().get("kind"));
            assertThrows(AssertionError.class, session::verify);
            String json = Files.readString(file);
            assertFalse(json.contains("PRIVATE-CUSTOMER-PAYLOAD"));
            assertFalse(json.contains("UPDATE carts"));
        }
    }

    @Test
    void forbidBlocksTheDriverBeforeChangingTheDatabase() throws Exception {
        try (var context = context(ForbiddenService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("forbid.json"));
            session.scenario("write is blocked", () -> assertThrows(PolicyViolationException.class,
                    () -> context.getBean(ForbiddenService.class).update()));
            assertEquals(10, context.getBean(JdbcTemplate.class).queryForObject("SELECT total FROM carts WHERE id = ?", Integer.class, "cart-1"));
            Map<String, Object> report = session.finish();
            assertEquals("failed", report.get("status"));
            Map<String, Object> event = list(list(report, "invocations").getFirst(), "events").getFirst();
            assertEquals(false, event.get("attempted"));
            assertEquals("blocked", event.get("outcome"));
        }
    }

    @Test
    void batchExecutionCannotBypassForbid() throws Exception {
        try (var context = context(BatchService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("batch.json"));
            session.scenario("batch is blocked", () -> assertThrows(PolicyViolationException.class,
                    () -> context.getBean(BatchService.class).update()));
            assertEquals(10, context.getBean(JdbcTemplate.class).queryForObject("SELECT total FROM carts WHERE id = ?", Integer.class, "cart-1"));
            assertEquals("failed", session.finish().get("status"));
        }
    }

    @Test
    void failedDriverWritesStillCountAsWriteAttempts() throws Exception {
        try (var context = context(FailedWriteService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("failed-write.json"));
            session.scenario("constraint rejects the update", () -> assertThrows(DataIntegrityViolationException.class,
                    () -> context.getBean(FailedWriteService.class).update()));
            Map<String, Object> report = session.finish();
            assertEquals("failed", report.get("status"));
            Map<String, Object> event = list(list(report, "invocations").getFirst(), "events").getFirst();
            assertEquals(true, event.get("attempted"));
            assertEquals("failed", event.get("outcome"));
        }
    }

    @Test
    void unsupportedSqlProducesIncompleteEvidenceEvenWhenTheTestCatchesIt() throws Exception {
        try (var context = context(UnknownService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("unknown.json"));
            session.scenario("unsupported query", () -> assertThrows(IncompleteInstrumentationException.class,
                    () -> context.getBean(UnknownService.class).query()));
            Map<String, Object> report = session.finish();
            assertEquals("incomplete", report.get("status"));
            assertFalse(list(report, "gaps").isEmpty());
            assertThrows(AssertionError.class, session::verify);
        }
    }

    @Test
    void aSuccessOnlyRunDoesNotClaimToCoverNoEffectOnError() throws Exception {
        try (var context = context(SafeService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("untested.json"));
            SafeService service = context.getBean(SafeService.class);
            session.scenario("only success", () -> {
                service.apply(false);
                service.preview();
            });
            Map<String, Object> report = session.finish();
            assertEquals("incomplete", report.get("status"));
            assertTrue(list(report, "policy_coverage").stream().anyMatch(policy ->
                    "no_effect_on_error".equals(policy.get("kind")) && "untested".equals(policy.get("status"))));
        }
    }

    @Test
    void unproxyablePoliciesAndEmptySessionsCannotPass() {
        try (var context = context(FinalMethodService.class)) {
            EvidenceSession session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("final.json"));
            Map<String, Object> report = session.finish();
            assertEquals("incomplete", report.get("status"));
            assertTrue(list(report, "gaps").stream().anyMatch(gap -> gap.get("message").toString().contains("cannot be intercepted")));
        }
    }

    @Test
    void aSecondSessionCannotOverwriteEarlierFailureWithPassingEvidence() throws Exception {
        Path file = temporary.resolve("single-session.json");
        try (var context = context(MutantService.class)) {
            EvidenceSession first = context.getBean(VitrifiRuntime.class).startSession(file);
            first.scenario("first session violates policy", () -> assertThrows(IllegalArgumentException.class,
                    () -> context.getBean(MutantService.class).apply("private")));
            assertEquals("failed", first.finish().get("status"));
        }
        try (var context = context(SafeService.class)) {
            assertThrows(IllegalStateException.class, () -> context.getBean(VitrifiRuntime.class).startSession(file));
        }
        var report = new ObjectMapper().readTree(file.toFile());
        assertEquals("incomplete", report.get("status").asText());
        assertFalse(report.get("complete").asBoolean());
    }

    @Test
    void anActiveSessionCannotEraseAConflictFromAnotherContext() throws Exception {
        Path file = temporary.resolve("conflicting-session.json");
        try (var firstContext = context(SafeService.class); var secondContext = context(SafeService.class)) {
            EvidenceSession first = firstContext.getBean(VitrifiRuntime.class).startSession(file);
            SafeService service = firstContext.getBean(SafeService.class);
            first.scenario("exercise both policies", () -> {
                assertThrows(IllegalArgumentException.class, () -> service.apply(true));
                service.preview();
            });
            assertThrows(IllegalStateException.class, () -> secondContext.getBean(VitrifiRuntime.class).startSession(file));
            assertEquals("incomplete", first.finish().get("status"));
            assertThrows(AssertionError.class, first::verify);
            assertFalse(new ObjectMapper().readTree(file.toFile()).get("complete").asBoolean());
        }
    }

    @Feature("Test.Workflow")
    public static class WorkflowPolicyService {
        private final JdbcTemplate jdbc;
        public WorkflowPolicyService(JdbcTemplate jdbc) { this.jdbc = jdbc; }
        @dev.vitrifi.api.NoEffectOnRejection(action = "write", resource = "Carts")
        public void apply(boolean write, boolean reject) {
            if (write) jdbc.update("UPDATE carts SET total = ? WHERE id = ?", 12, "cart-1");
            if (reject) throw new dev.vitrifi.workflow.Rejected("business rejection");
            throw new dev.vitrifi.workflow.Failed("save", new IllegalStateException("database failed"));
        }
    }

    @Test
    void infrastructureFailureDoesNotExerciseBusinessRejectionPolicy() throws Exception {
        try (var context = context(WorkflowPolicyService.class)) {
            var session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("workflow-failure.json"));
            var service = context.getBean(WorkflowPolicyService.class);
            session.scenario("failure after write", () -> assertThrows(dev.vitrifi.workflow.Failed.class, () -> service.apply(true, false)));
            var report = session.finish();
            assertEquals("incomplete", report.get("status"));
            assertEquals("untested", list(report, "policy_coverage").getFirst().get("status"));
            assertTrue(list(report, "violations").isEmpty());
        }
    }

    @Test
    void businessRejectionAfterWriteIsStillAViolation() throws Exception {
        try (var context = context(WorkflowPolicyService.class)) {
            var session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("workflow-rejected.json"));
            var service = context.getBean(WorkflowPolicyService.class);
            session.scenario("bad rejection", () -> assertThrows(dev.vitrifi.workflow.Rejected.class, () -> service.apply(true, true)));
            var report = session.finish();
            assertEquals("failed", report.get("status"));
            assertEquals("no_effect_on_rejection", list(report, "violations").getFirst().get("kind"));
        }
    }

    @Test
    void rejectionBeforeWriteCoversPolicyAlongsideInfrastructureFailure() throws Exception {
        try (var context = context(WorkflowPolicyService.class)) {
            var session = context.getBean(VitrifiRuntime.class).startSession(temporary.resolve("workflow-safe.json"));
            var service = context.getBean(WorkflowPolicyService.class);
            session.scenario("safe rejection", () -> assertThrows(dev.vitrifi.workflow.Rejected.class, () -> service.apply(false, true)));
            session.scenario("infrastructure failure", () -> assertThrows(dev.vitrifi.workflow.Failed.class, () -> service.apply(true, false)));
            session.verify();
        }
    }

    private static AnnotationConfigApplicationContext context(Class<?> feature) {
        AnnotationConfigApplicationContext context = new AnnotationConfigApplicationContext();
        context.register(VitrifiSpringConfiguration.class, Infrastructure.class, feature);
        context.refresh();
        JdbcTemplate jdbc = context.getBean(JdbcTemplate.class);
        jdbc.execute("CREATE TABLE carts (id VARCHAR(30) PRIMARY KEY, total INT CHECK (total >= 0), label VARCHAR(100))");
        jdbc.update("INSERT INTO carts (id, total, label) VALUES (?, ?, ?)", "cart-1", 10, "initial");
        return context;
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> list(Map<String, Object> report, String key) {
        return (List<Map<String, Object>>) report.get(key);
    }
}
