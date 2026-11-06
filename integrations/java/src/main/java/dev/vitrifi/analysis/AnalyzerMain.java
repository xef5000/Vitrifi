package dev.vitrifi.analysis;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** A source-only compiler bridge. This process never loads application classes. */
public final class AnalyzerMain {
    private AnalyzerMain() {}

    public static void main(String[] args) {
        try {
            Map<String, String> options = new LinkedHashMap<>();
            for (int i = 0; i < args.length; i += 2) {
                if (i + 1 == args.length || !List.of("--root", "--classpath", "--sources-file").contains(args[i])
                        || options.put(args[i], args[i + 1]) != null) {
                    throw new IllegalArgumentException("Expected --root DIR --classpath PATH --sources-file FILE");
                }
            }
            Path root = Path.of(required(options, "--root")).toRealPath();
            Path sourceList = Path.of(required(options, "--sources-file"));
            List<Path> sources = new ArrayList<>();
            for (String entry : Files.readAllLines(sourceList, StandardCharsets.UTF_8)) {
                if (entry.isBlank()) continue;
                Path relative = Path.of(entry);
                Path path = root.resolve(relative).normalize();
                if (relative.isAbsolute() || !path.startsWith(root) || !path.toRealPath().equals(path)
                        || !entry.endsWith(".java")) {
                    throw new IllegalArgumentException("Expected a nonsymlinked .java source inside root: " + entry);
                }
                sources.add(path);
            }
            if (sources.isEmpty()) throw new IllegalArgumentException("No Java sources were supplied");
            Map<String, Object> result = SourceAnalyzer.analyze(root, sources, options.getOrDefault("--classpath", ""));
            System.out.println(Json.write(result));
            if (!((List<?>) result.get("diagnostics")).isEmpty()) System.exit(1);
        } catch (IOException | RuntimeException exception) {
            System.out.println(Json.write(Map.of("operations", List.of(), "diagnostics", List.of(Map.of(
                    "code", "JAVA_ANALYZER", "message", String.valueOf(exception.getMessage()),
                    "location", Map.of("path", "<java-analyzer>", "line", 1, "column", 1))))));
            System.exit(2);
        }
    }

    private static String required(Map<String, String> options, String name) {
        String value = options.get(name);
        if (value == null || value.isBlank()) throw new IllegalArgumentException("Missing " + name);
        return value;
    }
}
