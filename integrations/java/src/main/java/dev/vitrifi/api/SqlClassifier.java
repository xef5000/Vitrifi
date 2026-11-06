package dev.vitrifi.api;

import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Deliberately small SQL subset. Unsupported SQL is never classified as harmless. */
public final class SqlClassifier {
    private SqlClassifier() {}
    public record Effect(String action, String operation, String table) {}

    private static final String IDENTIFIER = "[a-zA-Z_][a-zA-Z0-9_]*";
    private static final String TABLE = IDENTIFIER + "(?:\\." + IDENTIFIER + ")?";
    private static final Pattern SELECT = Pattern.compile(
            "^SELECT\\s+(?:\\*|" + IDENTIFIER + "(?:\\s*,\\s*" + IDENTIFIER + ")*)\\s+FROM\\s+(" + TABLE + ")(.*)$",
            Pattern.CASE_INSENSITIVE | Pattern.DOTALL);
    private static final Pattern UPDATE = Pattern.compile(
            "^UPDATE\\s+(" + TABLE + ")\\s+SET\\s+(.+)$", Pattern.CASE_INSENSITIVE | Pattern.DOTALL);
    private static final Pattern INSERT = Pattern.compile(
            "^INSERT\\s+INTO\\s+(" + TABLE + ")\\s*\\((" + IDENTIFIER + "(?:\\s*,\\s*" + IDENTIFIER + ")*)\\)\\s+VALUES\\s*\\(([^()]+)\\)$",
            Pattern.CASE_INSENSITIVE | Pattern.DOTALL);
    private static final Pattern DELETE = Pattern.compile(
            "^DELETE\\s+FROM\\s+(" + TABLE + ")(.*)$", Pattern.CASE_INSENSITIVE | Pattern.DOTALL);
    private static final String SCALAR = "(?:\\?|NULL|TRUE|FALSE|[-+]?\\d+(?:\\.\\d+)?)";
    private static final Pattern VALUE = Pattern.compile(SCALAR, Pattern.CASE_INSENSITIVE);
    private static final Pattern ASSIGNMENT = Pattern.compile(IDENTIFIER + "\\s*=\\s*" + SCALAR, Pattern.CASE_INSENSITIVE);
    private static final Pattern CONDITION = Pattern.compile(IDENTIFIER
            + "\\s*(?:(?:=|<>|!=|<=|>=|<|>)\\s*" + SCALAR + "|IS\\s+(?:NOT\\s+)?NULL)", Pattern.CASE_INSENSITIVE);

    public static Effect classify(String raw) {
        if (raw == null) {
            throw unsupported();
        }
        String sql = raw.strip();
        if (sql.endsWith(";")) {
            sql = sql.substring(0, sql.length() - 1).strip();
        }
        // No comments, string literals, quoted identifiers, multiple statements, or calls.
        // Prepared-statement values are never read or included in evidence.
        if (sql.isEmpty() || sql.contains(";") || sql.contains("--") || sql.contains("/*")
                || sql.contains("'") || sql.contains("\"") || sql.contains("`")) {
            throw unsupported();
        }
        Matcher select = SELECT.matcher(sql);
        if (select.matches()) {
            whereTail(select.group(2));
            return effect("read", "SELECT", select.group(1));
        }
        Matcher update = UPDATE.matcher(sql);
        if (update.matches()) {
            String[] parts = update.group(2).split("(?i)\\s+WHERE\\s+", -1);
            if (parts.length > 2) {
                throw unsupported();
            }
            for (String assignment : parts[0].split(",", -1)) {
                if (!ASSIGNMENT.matcher(assignment.strip()).matches()) {
                    throw unsupported();
                }
            }
            if (parts.length == 2) {
                conditions(parts[1]);
            }
            return effect("write", "UPDATE", update.group(1));
        }
        Matcher insert = INSERT.matcher(sql);
        if (insert.matches()) {
            for (String value : insert.group(3).split(",", -1)) {
                if (!VALUE.matcher(value.strip()).matches()) {
                    throw unsupported();
                }
            }
            return effect("write", "INSERT", insert.group(1));
        }
        Matcher delete = DELETE.matcher(sql);
        if (delete.matches()) {
            whereTail(delete.group(2));
            return effect("write", "DELETE", delete.group(1));
        }
        throw unsupported();
    }

    private static Effect effect(String action, String operation, String table) {
        return new Effect(action, operation, normalizeTable(table));
    }

    public static String normalizeTable(String table) {
        return table.strip().toLowerCase(Locale.ROOT);
    }

    private static void whereTail(String raw) {
        String tail = raw.strip();
        if (!tail.isEmpty() && !tail.toUpperCase(Locale.ROOT).startsWith("WHERE ")) {
            throw unsupported();
        }
        if (!tail.isEmpty()) {
            conditions(tail.substring(6));
        }
    }

    private static void conditions(String conditions) {
        for (String condition : conditions.split("(?i)\\s+(?:AND|OR)\\s+", -1)) {
            if (!CONDITION.matcher(condition.strip()).matches()) {
                throw unsupported();
            }
        }
    }

    private static IllegalArgumentException unsupported() {
        // Do not put SQL literals, parameters, or application payloads in diagnostics.
        return new IllegalArgumentException("SQL is outside the supported single-table SELECT/INSERT/UPDATE/DELETE subset");
    }
}
