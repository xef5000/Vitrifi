package dev.vitrifi.analysis;

import java.util.Iterator;
import java.util.Map;

/** Small output-only JSON encoder; the bridge has no application dependencies. */
final class Json {
    private Json() {}

    static String write(Object value) {
        if (value == null) return "null";
        if (value instanceof Boolean || value instanceof Number) return value.toString();
        if (value instanceof Map<?, ?> map) {
            StringBuilder out = new StringBuilder("{");
            for (Iterator<? extends Map.Entry<?, ?>> it = map.entrySet().iterator(); it.hasNext();) {
                Map.Entry<?, ?> entry = it.next();
                out.append(write(entry.getKey().toString())).append(':').append(write(entry.getValue()));
                if (it.hasNext()) out.append(',');
            }
            return out.append('}').toString();
        }
        if (value instanceof Iterable<?> list) {
            StringBuilder out = new StringBuilder("[");
            for (Iterator<?> it = list.iterator(); it.hasNext();) {
                out.append(write(it.next()));
                if (it.hasNext()) out.append(',');
            }
            return out.append(']').toString();
        }
        StringBuilder out = new StringBuilder("\"");
        for (char c : value.toString().toCharArray()) {
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 32) out.append(String.format("\\u%04x", (int) c));
                    else out.append(c);
                }
            }
        }
        return out.append('"').toString();
    }
}
