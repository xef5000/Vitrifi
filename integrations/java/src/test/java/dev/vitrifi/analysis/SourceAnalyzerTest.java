package dev.vitrifi.analysis;

import static org.junit.jupiter.api.Assertions.*;

import dev.vitrifi.api.SqlClassifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class SourceAnalyzerTest {
    @TempDir Path root;

    private static final String CHECKOUT = """
            package demo;
            import dev.vitrifi.api.*;
            import org.springframework.stereotype.*;
            import org.springframework.jdbc.core.JdbcTemplate;
            interface Repo { Cart load(String id); void save(String id, int total); }
            record Cart(String id, int total) {}
            @Repository @ResourceTable(value="Carts", table="carts")
            class SqlRepo implements Repo {
                private final JdbcTemplate jdbc;
                SqlRepo(JdbcTemplate jdbc) { this.jdbc = jdbc; }
                public Cart load(String id) {
                    return jdbc.queryForObject("SELECT id, total FROM carts WHERE id = ?",
                        (rs, row) -> new Cart(rs.getString("id"), rs.getInt("total")), id);
                }
                public void save(String id, int total) {
                    jdbc.update("UPDATE carts SET total = ? WHERE id = ?", total, id);
                }
            }
            @Service @Feature(value="Checkout.Discounts", purpose="Apply discounts")
            class Checkout {
                private final Repo repo;
                Checkout(Repo repo) { this.repo = repo; }
                @NoEffectOnError(action="write", resource="Carts")
                public Cart apply(String id) {
                    Cart cart = repo.load(id);
                    if (cart.total() < 0) throw new IllegalArgumentException("bad total");
                    repo.save(id, Math.max(cart.total(), 0));
                    return cart;
                }
                @Forbid(action="write", resource="Carts")
                public Cart preview(String id) { return repo.load(id); }
            }
            """;

    @SuppressWarnings("unchecked")
    private List<Map<String, Object>> list(Map<String, Object> object, String key) {
        return (List<Map<String, Object>>) object.get(key);
    }

    private Map<String, Object> analyze(Map<String, String> files) throws Exception {
        List<Path> sources = new ArrayList<>();
        for (Map.Entry<String, String> entry : files.entrySet()) {
            Path path = root.resolve(entry.getKey());
            Files.createDirectories(path.getParent());
            Files.writeString(path, entry.getValue());
            sources.add(path);
        }
        return SourceAnalyzer.analyze(root, sources, System.getProperty("java.class.path"));
    }

    @Test
    void workflowRuntimePoliciesAndConstructorsAreModeled() throws Exception {
        var model = analyze("""
            package demo;
            @org.springframework.stereotype.Service
            @dev.vitrifi.api.Feature("Checkout")
            class Checkout {
                @dev.vitrifi.api.NoEffectOnRejection(action="write", resource="Carts")
                @dev.vitrifi.api.NoEffectOnRejection(action="read", resource="Carts")
                public void apply(boolean reject) {
                    if (reject) throw new dev.vitrifi.workflow.Rejected("rejected");
                    throw new dev.vitrifi.workflow.Failed("leaf", new IllegalStateException("failed"));
                }
            }
            """);
        clean(model);
        var entry = operation(model, "demo.Checkout.apply(boolean)");
        assertEquals(2, list(entry, "policies").size());
        assertTrue(list(entry, "policies").stream().allMatch(p -> p.get("kind").equals("no_effect_on_rejection")));
        assertTrue(list(entry, "calls").stream().allMatch(c -> c.get("resolution").equals("intrinsic")), Json.write(model));
    }

    @Test
    void sourceCannotSpoofTrustedWorkflowExceptionConstructor() throws Exception {
        var model = analyze(Map.of(
            "demo/Checkout.java", """
                package demo;
                @dev.vitrifi.api.Feature("Checkout") class Checkout {
                    public void apply() { throw new dev.vitrifi.workflow.Rejected("rejected"); }
                }
                """,
            "dev/vitrifi/workflow/Rejected.java", """
                package dev.vitrifi.workflow;
                public class Rejected extends RuntimeException {
                    public Rejected(String message) { super(message); System.nanoTime(); }
                }
                """));
        clean(model);
        var entry = operation(model, "demo.Checkout.apply()");
        assertTrue(list(entry, "calls").stream().noneMatch(c -> c.get("resolution").equals("intrinsic")), Json.write(model));
        var ctor = operation(model, "dev.vitrifi.workflow.Rejected.<init>(java.lang.String)");
        assertTrue(list(ctor, "calls").stream().anyMatch(c -> c.get("resolution").equals("unknown")), Json.write(model));
    }

    private Map<String, Object> analyze(String source) throws Exception {
        return analyze(Map.of("demo/Checkout.java", source));
    }

    private Map<String, Object> operation(Map<String, Object> result, String symbol) {
        return list(result, "operations").stream().filter(o -> o.get("symbol").equals(symbol)).findFirst().orElseThrow(() ->
                new AssertionError("Missing " + symbol + " in " + Json.write(result)));
    }

    private void clean(Map<String, Object> result) {
        assertEquals(List.of(), result.get("diagnostics"), Json.write(result));
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> inventory(Map<String, Object> result) {
        return (Map<String, Object>) result.get("inventory");
    }

    private Map<String, Object> sourceClass(Map<String, Object> result, String name) {
        return list(inventory(result), "classes").stream().filter(c -> c.get("name").equals(name))
                .findFirst().orElseThrow(() -> new AssertionError("Missing inventory class " + name));
    }

    @Test void inventory_keeps_public_entries_after_feature_marker_removal() throws Exception {
        String source = """
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public Checkout() {}
                    public int run(int value) { return value; }
                    public static long extra(long value) { return value; }
                    private void helper() {}
                }
                """;
        Map<String, Object> original = analyze(source);
        Map<String, Object> removed = analyze(source.replace("@Feature(\"Checkout\") ", ""));
        clean(original);
        clean(removed);
        assertEquals(2, inventory(original).get("version"));
        assertEquals(sourceClass(original, "demo.Checkout"), sourceClass(removed, "demo.Checkout"));
        assertEquals(List.of("demo.Checkout.extra(long)", "demo.Checkout.run(int)"),
                sourceClass(removed, "demo.Checkout").get("public_methods"));
        assertEquals(List.of(), sourceClass(removed, "demo.Checkout").get("unsupported_api"));
        assertNull(operation(removed, "demo.Checkout.run(int)").get("feature"));
        assertEquals("long", operation(removed, "demo.Checkout.extra(long)").get("returns"));
    }

    @Test void inventory_exposes_inherited_abstract_and_synthesized_public_api_as_unsupported() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                interface DefaultApi { default int inherited() { return 1; } }
                class Parent { public static void inheritedStatic() {} public int parent() { return 2; } }
                class Checkout extends Parent implements DefaultApi { public int own() { return 3; } }
                abstract class AbstractApi { public abstract int run(); }
                record GeneratedRecord(int value) {}
                """);
        clean(result);
        Map<String, Object> checkout = sourceClass(result, "demo.Checkout");
        assertEquals(List.of("demo.Checkout.own()"), checkout.get("public_methods"));
        String gaps = checkout.get("unsupported_api").toString();
        assertTrue(gaps.contains("demo.DefaultApi.inherited()"));
        assertTrue(gaps.contains("demo.Parent.inheritedStatic()"));
        assertTrue(gaps.contains("demo.Parent.parent()"));
        assertFalse(gaps.contains("java.lang.Object"));
        assertTrue(sourceClass(result, "demo.DefaultApi").get("unsupported_api").toString().contains("not a concrete class"));
        assertTrue(sourceClass(result, "demo.AbstractApi").get("unsupported_api").toString().contains("not a concrete class"));
        assertTrue(sourceClass(result, "demo.GeneratedRecord").get("unsupported_api").toString().contains("no analyzed source declaration"));
    }

    @Test void inventory_preserves_unused_resource_table_identity_when_the_friendly_name_changes() throws Exception {
        String source = """
                package demo;
                import dev.vitrifi.api.*;
                @ResourceTable(value="Carts", table="SHOP.CARTS") class Storage {}
                class Checkout { public int run() { return 1; } }
                """;
        Map<String, Object> original = analyze(source);
        Map<String, Object> renamed = analyze(source.replace("value=\"Carts\"", "value=\"Innocent\""));
        clean(original);
        clean(renamed);
        assertEquals(List.of(Map.of("owner", "demo.Storage", "path", "demo/Checkout.java",
                "table", "shop.carts", "resource", "Carts")), inventory(original).get("resources"));
        assertEquals(List.of(Map.of("owner", "demo.Storage", "path", "demo/Checkout.java",
                "table", "shop.carts", "resource", "Innocent")), inventory(renamed).get("resources"));
        Map<String, Object> conflicting = analyze(source + "\n@ResourceTable(value=\"Other\", table=\"shop.carts\") class OtherStorage {}\n");
        assertEquals(2, list(inventory(conflicting), "resources").size());
        assertTrue(list(conflicting, "diagnostics").stream().anyMatch(d -> d.get("message").toString().contains("conflicting resources")));
    }

    @Test void native_types_keep_primitive_width_and_generic_arguments() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import java.util.List;
                class Checkout {
                    public int width(int value) { return value; }
                    public long width(long value) { return value; }
                    public List<String> values(List<String> input) { return input; }
                }
                """);
        clean(result);
        assertEquals(List.of("demo.Checkout.values(java.util.List)", "demo.Checkout.width(int)", "demo.Checkout.width(long)"),
                sourceClass(result, "demo.Checkout").get("public_methods"));
        assertEquals("int", operation(result, "demo.Checkout.width(int)").get("returns"));
        assertEquals("long", operation(result, "demo.Checkout.width(long)").get("returns"));
        Map<String, Object> generic = operation(result, "demo.Checkout.values(java.util.List)");
        assertEquals("java.util.List<java.lang.String>", generic.get("returns"));
        assertEquals("java.util.List<java.lang.String>", list(generic, "parameters").getFirst().get("annotation"));
    }

    @Test void generic_type_variable_bounds_are_explicit_inventory_gaps() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                class GenericOwner<T extends Number> { public T value(T input) { return input; } }
                class Checkout {
                    public <T extends Number> T value() { return null; }
                    public java.util.List<String> concrete(java.util.List<String> input) { return input; }
                }
                """);
        clean(result);
        assertTrue(sourceClass(result, "demo.GenericOwner").get("unsupported_api").toString()
                .contains("class has unmodeled generic type parameter bounds"));
        assertEquals(List.of("Public method has unmodeled generic type parameter bounds: demo.Checkout.value()"),
                sourceClass(result, "demo.Checkout").get("unsupported_api"));
        Map<String, Object> changed = analyze("""
                package demo;
                class Checkout { public <T extends java.io.Serializable> T value() { return null; } }
                """);
        clean(changed);
        assertEquals(operation(result, "demo.Checkout.value()").get("returns"),
                operation(changed, "demo.Checkout.value()").get("returns"));
        assertFalse(((List<?>) sourceClass(changed, "demo.Checkout").get("unsupported_api")).isEmpty());
    }

    @Test void native_methods_cannot_look_like_complete_empty_effect_implementations() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public native int external();
                    public int run() { return hidden(); }
                    private native int hidden();
                }
                """);
        clean(result);
        assertEquals(List.of("Public native method has no analyzable Java body: demo.Checkout.external()"),
                sourceClass(result, "demo.Checkout").get("unsupported_api"));
        for (String name : List.of("external", "hidden")) {
            assertTrue(list(operation(result, "demo.Checkout." + name + "()"), "calls").stream()
                    .anyMatch(c -> c.get("resolution").equals("unknown") && c.get("expression").equals("unmodeled method body")));
        }
        assertEquals("demo/Checkout.java::demo.Checkout.hidden()", list(operation(result, "demo.Checkout.run()"), "calls").getFirst().get("target"));
    }

    @Test void nested_classes_have_separate_complete_public_method_inventories() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                class Checkout {
                    public int run() { return 1; }
                    public static class Nested { public static long extra() { return 2; } }
                }
                """);
        clean(result);
        assertEquals(List.of("demo.Checkout.run()"), sourceClass(result, "demo.Checkout").get("public_methods"));
        assertEquals(List.of("demo.Checkout.Nested.extra()"), sourceClass(result, "demo.Checkout.Nested").get("public_methods"));
        assertEquals("long", operation(result, "demo.Checkout.Nested.extra()").get("returns"));
    }

    @Test void public_static_entries_expose_initialization_gaps_without_an_analyzed_caller() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    static { System.getenv("UNMODELED"); }
                    public static int run() { return 1; }
                }
                class FieldEntry {
                    static Object value = System.getenv("UNMODELED");
                    public static int run() { return 1; }
                }
                class ConstantEntry {
                    static final int value = 1;
                    public static int run() { return value; }
                }
                """);
        clean(result);
        for (String name : List.of("Checkout", "FieldEntry")) {
            assertEquals(List.of("Public static entry can trigger unmodeled class initialization: demo." + name + ".run()"),
                    sourceClass(result, "demo." + name).get("unsupported_api"));
            assertEquals(List.of(), operation(result, "demo." + name + ".run()").get("calls"));
        }
        assertEquals(List.of(), sourceClass(result, "demo.ConstantEntry").get("unsupported_api"));
    }

    @Test void static_entry_initialization_includes_superclasses_and_default_method_interfaces() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                class Parent { static { System.getenv("UNMODELED"); } }
                class ParentEntry extends Parent { public static int run() { return 1; } }
                interface DefaultInit {
                    Object value = System.getenv("UNMODELED");
                    default int helper() { return 1; }
                }
                interface Indirect extends DefaultInit {}
                class DefaultEntry implements Indirect {
                    public static int run() { return 1; }
                    public int helper() { return 1; }
                }
                interface NoDefaultInit {
                    Object value = System.getenv("NOT_INITIALIZED_BY_CLASS");
                    void consume();
                }
                class NoDefaultEntry implements NoDefaultInit {
                    public static int run() { return 1; }
                    public void consume() {}
                }
                """);
        clean(result);
        for (String name : List.of("ParentEntry", "DefaultEntry")) {
            assertEquals(List.of("Public static entry can trigger unmodeled class initialization: demo." + name + ".run()"),
                    sourceClass(result, "demo." + name).get("unsupported_api"));
        }
        assertEquals(List.of(), sourceClass(result, "demo.NoDefaultEntry").get("unsupported_api"));
    }

    @Test void public_generic_and_covariant_overrides_expose_unmodeled_bridges() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                interface Convert<T> { T convert(T value); }
                interface Strings extends Convert<String> {}
                class GenericEntry implements Strings {
                    public String convert(String value) { return value; }
                }
                class Parent { public Number value() { return null; } }
                class CovariantEntry extends Parent { public Integer value() { return null; } }
                class ExternalEntry implements java.util.function.Function<String, String> {
                    public String apply(String value) { return value; }
                }
                """);
        clean(result);
        assertEquals(List.of("demo.GenericEntry.convert(java.lang.String)"),
                sourceClass(result, "demo.GenericEntry").get("public_methods"));
        assertEquals(List.of("Public override requires an unmodeled compiler bridge: demo.GenericEntry.convert(java.lang.String) overrides demo.Convert.convert(java.lang.Object)"),
                sourceClass(result, "demo.GenericEntry").get("unsupported_api"));
        assertEquals(List.of("Public override requires an unmodeled compiler bridge: demo.CovariantEntry.value() overrides demo.Parent.value()"),
                sourceClass(result, "demo.CovariantEntry").get("unsupported_api"));
        assertTrue(sourceClass(result, "demo.ExternalEntry").get("unsupported_api").toString()
                .contains("demo.ExternalEntry.apply(java.lang.String) overrides java.util.function.Function.apply(java.lang.Object)"));
    }

    @Test void ordinary_overrides_do_not_fabricate_bridge_gaps() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                interface Api { Number value(); }
                class PlainEntry implements Api { public Number value() { return null; } }
                class OverrideEntry extends PlainEntry { public Number value() { return null; } }
                """);
        clean(result);
        assertEquals(List.of(), sourceClass(result, "demo.PlainEntry").get("unsupported_api"));
        assertEquals(List.of(), sourceClass(result, "demo.OverrideEntry").get("unsupported_api"));
    }

    @Test void resolves_real_spring_injection_jdbc_effects_and_row_mapper_body() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT);
        clean(result);
        Map<String, Object> apply = operation(result, "demo.Checkout.apply(java.lang.String)");
        assertEquals("demo.Cart", apply.get("returns"));
        assertEquals("java.lang.String", list(apply, "parameters").getFirst().get("annotation"));
        assertEquals("demo/Checkout.java::demo.Checkout.apply(java.lang.String)", apply.get("id"));
        assertEquals("no_effect_on_error", list(apply, "policies").getFirst().get("kind"));
        assertTrue(list(apply, "calls").stream().anyMatch(c -> "demo/Checkout.java::demo.SqlRepo.save(java.lang.String,int)".equals(c.get("target"))));
        assertTrue(list(apply, "calls").stream().noneMatch(c -> c.get("resolution").equals("unknown")), Json.write(apply));
        Map<String, Object> save = operation(result, "demo.SqlRepo.save(java.lang.String,int)");
        assertEquals(List.of(Map.of("action", "write", "resource", "Carts")), save.get("direct_effects"));
        Map<String, Object> load = operation(result, "demo.SqlRepo.load(java.lang.String)");
        assertEquals(List.of(Map.of("action", "read", "resource", "Carts")), load.get("direct_effects"));
        Map<String, Object> callback = list(result, "operations").stream().filter(o -> o.get("id").toString().contains("$rowMapper")).findFirst().orElseThrow();
        assertEquals("demo.Cart", callback.get("returns"));
        assertEquals("java.sql.ResultSet", list(callback, "parameters").getFirst().get("annotation"));
        assertEquals(3, list(callback, "calls").size());
        assertTrue(list(callback, "calls").stream().allMatch(c -> c.get("resolution").equals("intrinsic")), Json.write(callback));
    }

    @Test void method_renames_and_overload_types_come_from_javac() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public int run() { return calculate(1) + calculate("a"); }
                    private int calculate(int value) { return value; }
                    private int calculate(String value) { return value.length(); }
                }
                """);
        clean(result);
        List<Map<String, Object>> calls = list(operation(result, "demo.Checkout.run()"), "calls");
        assertEquals(List.of("demo/Checkout.java::demo.Checkout.calculate(int)",
                "demo/Checkout.java::demo.Checkout.calculate(java.lang.String)"), calls.stream().map(c -> c.get("target")).toList());
        Map<String, Object> renamed = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public long run(int value) { return changed(value); }
                    private long changed(int value) { return value; }
                }
                """);
        clean(renamed);
        assertEquals("long", operation(renamed, "demo.Checkout.run(int)").get("returns"));
        assertTrue(list(renamed, "operations").stream().noneMatch(o -> o.get("symbol").toString().contains("calculate")));
    }

    @Test void an_ambiguous_injected_interface_does_not_choose_the_first_bean() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT + """
                @Service class OtherRepo implements Repo {
                    public Cart load(String id) { return new Cart(id, 0); }
                    public void save(String id, int total) {}
                }
                """);
        clean(result);
        List<Map<String, Object>> calls = list(operation(result, "demo.Checkout.apply(java.lang.String)"), "calls");
        assertTrue(calls.stream().filter(c -> c.get("expression").toString().startsWith("repo.")).allMatch(c -> c.get("resolution").equals("unknown")));
        assertTrue(calls.stream().anyMatch(c -> c.get("reason").toString().contains("2 candidate")));
    }

    @Test void shadowed_fields_and_mutable_constructor_wiring_remain_unknown() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT.replace("public Cart preview(String id) { return repo.load(id); }",
                "public Cart preview(String id) { Repo repo = null; return repo.load(id); }"));
        clean(result);
        Map<String, Object> preview = operation(result, "demo.Checkout.preview(java.lang.String)");
        assertEquals("unknown", list(preview, "calls").getFirst().get("resolution"));
        Map<String, Object> mutable = analyze(CHECKOUT.replace("private final Repo repo;", "private Repo repo;"));
        clean(mutable);
        assertTrue(list(operation(mutable, "demo.Checkout.apply(java.lang.String)"), "calls").stream()
                .filter(c -> c.get("expression").toString().startsWith("repo.")).allMatch(c -> c.get("resolution").equals("unknown")));
    }

    @Test void save_spelling_does_not_invent_an_effect_and_unknown_externals_stay_visible() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public String run(Object injected) { save(); return injected.toString(); }
                    private void save() {}
                }
                """);
        clean(result);
        assertEquals(List.of(), operation(result, "demo.Checkout.save()").get("direct_effects"));
        List<Map<String, Object>> calls = list(operation(result, "demo.Checkout.run(java.lang.Object)"), "calls");
        assertEquals("resolved", calls.getFirst().get("resolution"));
        assertEquals("unknown", calls.getLast().get("resolution"));
    }

    @Test void dynamic_joined_and_unmapped_sql_remain_unknown() throws Exception {
        for (String sql : List.of("\"UPDATE carts SET total = ? WHERE id = ?\" + id",
                "\"UPDATE missing SET total = ? WHERE id = ?\"",
                "\"UPDATE carts SET total = (SELECT total FROM other) WHERE id = ?\"")) {
            String source = CHECKOUT.replace("\"UPDATE carts SET total = ? WHERE id = ?\"", sql);
            Map<String, Object> result = analyze(source);
            clean(result);
            Map<String, Object> save = operation(result, "demo.SqlRepo.save(java.lang.String,int)");
            assertEquals(List.of(), save.get("direct_effects"));
            assertEquals("unknown", list(save, "calls").getFirst().get("resolution"));
        }
    }

    @Test void arbitrary_lambda_bodies_are_not_misattributed_to_the_parent() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public Runnable run() { return () -> save(); }
                    private void save() {}
                }
                """);
        clean(result);
        List<Map<String, Object>> calls = list(operation(result, "demo.Checkout.run()"), "calls");
        assertEquals(1, calls.size());
        assertEquals("unknown", calls.getFirst().get("resolution"));
        assertNull(calls.getFirst().get("target"));
    }

    @Test void unsupported_calls_inside_a_jdbc_callback_are_preserved() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT.replace("new Cart(rs.getString(\"id\"), rs.getInt(\"total\"))",
                "new Cart(System.getenv(\"ID\"), rs.getInt(\"total\"))"));
        clean(result);
        Map<String, Object> callback = list(result, "operations").stream().filter(o -> o.get("id").toString().contains("$rowMapper")).findFirst().orElseThrow();
        assertTrue(list(callback, "calls").stream().anyMatch(c -> c.get("expression").equals("System.getenv") && c.get("resolution").equals("unknown")));
    }

    @Test void indirect_jdbc_callback_dispatch_remains_unknown() throws Exception {
        String source = CHECKOUT.replace("private final JdbcTemplate jdbc;", """
                private final org.springframework.jdbc.core.RowMapper<Cart> mapper =
                    (rs, row) -> new Cart(rs.getString("id"), rs.getInt("total"));
                private final JdbcTemplate jdbc;
                """).replace("(rs, row) -> new Cart(rs.getString(\"id\"), rs.getInt(\"total\")), id", "mapper, id");
        Map<String, Object> result = analyze(source);
        clean(result);
        assertTrue(list(operation(result, "demo.SqlRepo.load(java.lang.String)"), "calls").stream()
                .anyMatch(c -> c.get("resolution").equals("unknown") && c.get("expression").equals("JDBC callback")));
    }

    @Test void source_visible_jdbc_subclasses_disable_the_standard_dispatch_summary() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT + """
                class CustomJdbc extends JdbcTemplate {
                    @Override public int update(String sql, Object... args) {
                        System.getenv("SIDE_EFFECT"); return 0;
                    }
                }
                """);
        clean(result);
        Map<String, Object> save = operation(result, "demo.SqlRepo.save(java.lang.String,int)");
        assertEquals(List.of(), save.get("direct_effects"));
        assertTrue(list(save, "calls").stream().anyMatch(c -> c.get("resolution").equals("unknown")));
    }

    @Test void invalid_metadata_and_noninterceptable_policies_are_errors() throws Exception {
        Map<String, Object> malformed = analyze(CHECKOUT.replace("action=\"write\"", "action=\"delete\""));
        assertTrue(list(malformed, "diagnostics").stream().anyMatch(d -> d.get("code").equals("JAVA_METADATA")));
        Map<String, Object> hidden = analyze(CHECKOUT.replace("public Cart preview", "private Cart preview"));
        assertTrue(list(hidden, "diagnostics").stream().anyMatch(d -> d.get("code").equals("JAVA_RUNTIME_POLICY")));
        Map<String, Object> internal = analyze(CHECKOUT.replace("Cart cart = repo.load(id);", "Cart cart = preview(id);"));
        assertTrue(list(internal, "diagnostics").stream().anyMatch(d -> d.get("message").toString().contains("Same-instance")));
    }

    @Test void repeatable_policy_metadata_is_not_lost() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT.replace("@Forbid(action=\"write\", resource=\"Carts\")",
                "@Forbid(action=\"write\", resource=\"Carts\") @Forbid(action=\"network\", resource=\"Payments\")"));
        clean(result);
        assertEquals(2, list(operation(result, "demo.Checkout.preview(java.lang.String)"), "policies").size());
    }

    @Test void conditional_or_custom_component_wiring_does_not_resolve_a_target() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT.replace("@Repository @ResourceTable", "@org.springframework.context.annotation.Profile(\"production\") @Repository @ResourceTable"));
        clean(result);
        assertTrue(list(operation(result, "demo.Checkout.preview(java.lang.String)"), "calls").stream().allMatch(c -> c.get("resolution").equals("unknown")));
    }

    @Test void straightforward_bean_factories_resolve_but_dynamic_factories_do_not() throws Exception {
        String source = CHECKOUT.replace("@Repository @ResourceTable", "@ResourceTable") + """
                @org.springframework.context.annotation.Configuration
                class Configuration {
                    @org.springframework.context.annotation.Bean
                    Repo repo(JdbcTemplate jdbc) { return new SqlRepo(jdbc); }
                }
                """;
        Map<String, Object> result = analyze(source);
        clean(result);
        assertEquals("resolved", list(operation(result, "demo.Checkout.preview(java.lang.String)"), "calls").getFirst().get("resolution"));
        Map<String, Object> dynamic = analyze(source.replace("return new SqlRepo(jdbc);", "if (jdbc == null) return null; return new SqlRepo(jdbc);"));
        clean(dynamic);
        assertEquals("unknown", list(operation(dynamic, "demo.Checkout.preview(java.lang.String)"), "calls").getFirst().get("resolution"));
    }

    @Test void unresolved_types_fail_compilation_instead_of_fabricating_symbols() throws Exception {
        Map<String, Object> result = analyze("package demo; class Checkout { Missing go() { return null; } }");
        assertEquals(List.of(), result.get("operations"));
        assertTrue(list(result, "diagnostics").stream().anyMatch(d -> d.get("code").equals("JAVA_COMPILER")));
    }

    @Test void source_analysis_does_not_execute_static_initializers() throws Exception {
        Path marker = root.resolve("executed");
        String source = """
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    static { try { java.nio.file.Files.writeString(java.nio.file.Path.of("%s"), "bad"); }
                        catch (java.io.IOException e) { throw new RuntimeException(e); } }
                    public int run() { return 1; }
                }
                """.formatted(marker.toString().replace("\\", "\\\\"));
        Map<String, Object> result = analyze(source);
        clean(result);
        assertFalse(Files.exists(marker));
    }

    @Test void generic_symbols_use_erasure_while_signatures_keep_declared_types() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                @Feature("Checkout") class Checkout {
                    public java.util.List<String> run(java.util.List<String> values, int[] indexes) { return values; }
                }
                """);
        clean(result);
        Map<String, Object> operation = operation(result, "demo.Checkout.run(java.util.List,int[])");
        assertEquals("java.util.List<java.lang.String>", operation.get("returns"));
        assertTrue(operation.get("signature").toString().contains("java.util.List<java.lang.String>"));
    }

    @Test void standard_exception_super_is_summarized_but_custom_constructor_body_is_traversed() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                class Rejected extends RuntimeException {
                    Rejected(String message) { super(message); System.getenv("DEBUG"); }
                }
                @Feature("Checkout") class Checkout {
                    public void run() { throw new Rejected("rejected"); }
                }
                """);
        clean(result);
        Map<String, Object> constructor = operation(result, "demo.Rejected.<init>(java.lang.String)");
        assertEquals("intrinsic", list(constructor, "calls").getFirst().get("resolution"));
        assertEquals("unknown", list(constructor, "calls").getLast().get("resolution"));
        assertEquals(constructor.get("id"), list(operation(result, "demo.Checkout.run()"), "calls").getFirst().get("target"));
    }

    @Test void source_defined_record_accessors_are_analyzed_as_real_methods() throws Exception {
        String source = CHECKOUT.replace("record Cart(String id, int total) {}", """
                record Cart(String id, int total) {
                    public int total() { Effects.write(); return total; }
                }
                class Effects {
                    static JdbcTemplate jdbc;
                    static void write() { jdbc.update("UPDATE carts SET total = ? WHERE id = ?", 1, "cart"); }
                }
                """);
        Map<String, Object> result = analyze(source);
        clean(result);
        assertTrue(list(operation(result, "demo.Checkout.apply(java.lang.String)"), "calls").stream()
                .anyMatch(c -> "demo/Checkout.java::demo.Cart.total()".equals(c.get("target"))));
        assertEquals("demo/Checkout.java::demo.Effects.write()", list(operation(result, "demo.Cart.total()"), "calls").getFirst().get("target"));
        assertEquals(List.of(Map.of("action", "write", "resource", "Carts")), operation(result, "demo.Effects.write()").get("direct_effects"));
    }

    @Test void summarized_record_calls_retain_source_owners_without_changing_effect_summaries() throws Exception {
        Map<String, Object> result = analyze(CHECKOUT);
        clean(result);
        Map<String, Object> apply = operation(result, "demo.Checkout.apply(java.lang.String)");
        List<Map<String, Object>> accessors = list(apply, "calls").stream()
                .filter(c -> c.get("expression").equals("cart.total")).toList();
        assertEquals(2, accessors.size());
        for (Map<String, Object> call : accessors) {
            assertEquals("intrinsic", call.get("resolution"));
            assertEquals("demo.Cart", call.get("source_owner"));
            assertNull(call.get("target"));
        }
        Map<String, Object> callback = operation(result, "demo.SqlRepo.load(java.lang.String)$rowMapper1");
        Map<String, Object> construction = list(callback, "calls").stream()
                .filter(c -> c.get("expression").equals("new Cart")).findFirst().orElseThrow();
        assertEquals("intrinsic", construction.get("resolution"));
        assertEquals("demo.Cart", construction.get("source_owner"));
        assertNull(construction.get("target"));
        assertEquals(List.of(), callback.get("direct_effects"));
        assertTrue(list(apply, "calls").stream().filter(c -> c.get("expression").equals("Math.max")
                || c.get("expression").equals("new IllegalArgumentException")).allMatch(c -> !c.containsKey("source_owner")));
        assertTrue(list(callback, "calls").stream().filter(c -> c.get("expression").toString().startsWith("rs."))
                .allMatch(c -> !c.containsKey("source_owner")));
    }

    @Test void summarized_nested_record_calls_use_the_canonical_source_owner() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                class Values { record Cart(int total) {} }
                class Checkout {
                    public int run(Values.Cart cart) { return new Values.Cart(cart.total()).total(); }
                }
                """);
        clean(result);
        List<Map<String, Object>> calls = list(operation(result, "demo.Checkout.run(demo.Values.Cart)"), "calls");
        assertEquals(3, calls.size());
        assertTrue(calls.stream().allMatch(c -> c.get("resolution").equals("intrinsic")
                && c.get("source_owner").equals("demo.Values.Cart") && c.get("target") == null), Json.write(calls));
    }

    @Test void shared_sql_classifier_rejects_hidden_tables_sequences_and_functions() {
        for (String sql : List.of("UPDATE carts SET total = ? FROM secrets WHERE id = ?",
                "UPDATE carts SET total = NEXT VALUE FOR secret_sequence WHERE id = ?",
                "SELECT nextval() FROM carts", "SELECT * FROM carts JOIN secrets ON carts.id = secrets.id",
                "UPDATE carts SET total = ?; DELETE FROM secrets", "UPDATE carts SET total = custom_function(?)",
                "INSERT INTO carts (id) VALUES (NEXT VALUE FOR secret_sequence)")) {
            assertThrows(IllegalArgumentException.class, () -> SqlClassifier.classify(sql), sql);
        }
        assertEquals(new SqlClassifier.Effect("read", "SELECT", "carts"), SqlClassifier.classify("SELECT id, total FROM carts WHERE id = ?"));
        assertEquals(new SqlClassifier.Effect("write", "INSERT", "carts"), SqlClassifier.classify("INSERT INTO carts (id, total) VALUES (?, ?)"));
        assertEquals(new SqlClassifier.Effect("write", "DELETE", "carts"), SqlClassifier.classify("DELETE FROM carts WHERE id = ?"));
        assertEquals(new SqlClassifier.Effect("write", "UPDATE", "carts"), SqlClassifier.classify("UPDATE carts SET total = ?, coupon = ? WHERE id = ?"));
    }

    @Test void implicit_java_dispatch_is_not_silently_reported_pure() throws Exception {
        Map<String, Object> result = analyze("""
                package demo;
                import dev.vitrifi.api.*;
                class Hidden { static Object VALUE = System.getenv("VALUE"); }
                record Value(Object value) {}
                @Feature("Checkout") class Checkout {
                    public void close(AutoCloseable resource) throws Exception { try (resource) {} }
                    public void iterate(Iterable<Integer> values) { for (int value : values) {} }
                    public String concatenate(Object value) { return "value=" + value; }
                    public String append(Object value) { String result = ""; result += value; return result; }
                    public Object field() { return Hidden.VALUE; }
                    public Object deconstruct(Object value) { return value instanceof Value(var inner) ? inner : null; }
                    public int array(int[] values) { int total = 0; for (int value : values) total += value; return total; }
                }
                """);
        clean(result);
        for (String symbol : List.of("close(java.lang.AutoCloseable)", "iterate(java.lang.Iterable)",
                "concatenate(java.lang.Object)", "append(java.lang.Object)", "field()", "deconstruct(java.lang.Object)")) {
            assertTrue(list(operation(result, "demo.Checkout." + symbol), "calls").stream().anyMatch(c -> c.get("resolution").equals("unknown")), symbol);
        }
        assertEquals(List.of(), operation(result, "demo.Checkout.array(int[])").get("calls"));
    }

    @Test void source_visible_async_advice_keeps_a_real_call_graph_incomplete() throws Exception {
        Map<String, Object> methodAdvice = analyze(CHECKOUT.replace("public void save(String id, int total) {",
                "@org.springframework.scheduling.annotation.Async public void save(String id, int total) {"));
        clean(methodAdvice);
        assertTrue(list(operation(methodAdvice, "demo.SqlRepo.save(java.lang.String,int)"), "calls").stream()
                .anyMatch(c -> c.get("resolution").equals("unknown") && c.get("expression").equals("Spring @Async advice")));
        Map<String, Object> typeAdvice = analyze(CHECKOUT.replace("@Repository @ResourceTable",
                "@org.springframework.scheduling.annotation.Async @Repository @ResourceTable"));
        clean(typeAdvice);
        assertTrue(list(operation(typeAdvice, "demo.SqlRepo.load(java.lang.String)"), "calls").stream()
                .anyMatch(c -> c.get("resolution").equals("unknown") && c.get("expression").equals("Spring @Async advice")));
    }

    @Test void callback_identity_survives_whitespace_only_source_changes() throws Exception {
        Map<String, Object> original = analyze(CHECKOUT);
        Map<String, Object> shifted = analyze(CHECKOUT.replace("return jdbc.queryForObject", "\n\n// extra documentation\n                    return jdbc.queryForObject"));
        clean(original);
        clean(shifted);
        assertEquals(list(original, "operations").stream().map(o -> o.get("id")).toList(),
                list(shifted, "operations").stream().map(o -> o.get("id")).toList());
        Map<String, Object> before = operation(original, "demo.SqlRepo.load(java.lang.String)$rowMapper1");
        Map<String, Object> after = operation(shifted, "demo.SqlRepo.load(java.lang.String)$rowMapper1");
        assertNotEquals(before.get("line"), after.get("line"));
        assertEquals(before.get("signature"), after.get("signature"));
        assertEquals(list(operation(original, "demo.SqlRepo.load(java.lang.String)"), "calls").stream().map(c -> c.get("target")).toList(),
                list(operation(shifted, "demo.SqlRepo.load(java.lang.String)"), "calls").stream().map(c -> c.get("target")).toList());
    }
}
