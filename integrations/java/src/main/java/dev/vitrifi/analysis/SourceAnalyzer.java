package dev.vitrifi.analysis;

import com.sun.source.tree.*;
import com.sun.source.util.JavacTask;
import com.sun.source.util.TreePath;
import com.sun.source.util.TreePathScanner;
import com.sun.source.util.Trees;
import dev.vitrifi.api.SqlClassifier;
import java.io.File;
import java.io.IOException;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.*;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import javax.lang.model.element.*;
import javax.lang.model.type.*;
import javax.lang.model.util.Elements;
import javax.lang.model.util.Types;
import javax.tools.*;

/**
 * Bounded Java/Spring source analysis. javac supplies symbol and type identity;
 * framework and library summaries are explicit and retain their assumptions.
 */
public final class SourceAnalyzer {
    private static final String API = "dev.vitrifi.api.";
    private static final Set<String> COMPONENTS = Set.of("org.springframework.stereotype.Component",
            "org.springframework.stereotype.Service", "org.springframework.stereotype.Repository");
    private static final String AUTOWIRED = "org.springframework.beans.factory.annotation.Autowired";
    private static final String ASYNC = "org.springframework.scheduling.annotation.Async";
    private static final String BEAN = "org.springframework.context.annotation.Bean";
    private static final Set<String> WIRING_MODIFIERS = Set.of("org.springframework.context.annotation.Profile",
            "org.springframework.context.annotation.Conditional", "org.springframework.context.annotation.Lazy",
            "org.springframework.context.annotation.Scope", "org.springframework.beans.factory.annotation.Qualifier",
            "org.springframework.context.annotation.Primary", "org.springframework.context.annotation.Fallback");
    private static final Set<String> ACTIONS = Set.of("read", "write", "network", "emit");
    private static final Pattern NAME = Pattern.compile("[A-Za-z_][A-Za-z0-9_]*(?:\\.[A-Za-z_][A-Za-z0-9_]*)*");
    private static final Pattern TABLE = Pattern.compile("[A-Za-z_][A-Za-z0-9_]*(?:\\.[A-Za-z_][A-Za-z0-9_]*)?");
    private final Path root;
    private final Trees trees;
    private final Types types;
    private final Elements elements;
    private final List<Map<String, Object>> diagnostics = new ArrayList<>();
    private final LinkedHashMap<ExecutableElement, Operation> methods = new LinkedHashMap<>();
    private final LinkedHashMap<TypeElement, ClassInfo> classes = new LinkedHashMap<>();
    private final Set<TypeElement> sourceTypes = new HashSet<>();
    private final List<Operation> callbacks = new ArrayList<>();
    private final List<BeanCandidate> beans = new ArrayList<>();
    private final Map<VariableElement, Injection> injections = new HashMap<>();
    private final Map<String, String> resourceTables = new TreeMap<>();
    private final List<Map<String, Object>> resourceInventory = new ArrayList<>();
    private final Set<String> conflictedTables = new HashSet<>();
    private final Map<String, String> featurePurposes = new HashMap<>();

    private SourceAnalyzer(Path root, JavacTask task) {
        this.root = root;
        this.trees = Trees.instance(task);
        this.types = task.getTypes();
        this.elements = task.getElements();
    }

    public static Map<String, Object> analyze(Path root, List<Path> sources, String classpath) throws IOException {
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        if (compiler == null) throw new IllegalStateException("A full JDK with jdk.compiler is required");
        DiagnosticCollector<JavaFileObject> compilerDiagnostics = new DiagnosticCollector<>();
        StringWriter messages = new StringWriter();
        try (StandardJavaFileManager manager = compiler.getStandardFileManager(compilerDiagnostics, Locale.ROOT,
                StandardCharsets.UTF_8)) {
            String toolClasspath = System.getProperty("java.class.path", "");
            String cp = classpath.isEmpty() ? toolClasspath : classpath + File.pathSeparator + toolClasspath;
            List<String> options = List.of("-proc:none", "-implicit:none", "-encoding", "UTF-8",
                    "--class-path", cp, "--source-path", "");
            List<Path> orderedSources = sources.stream().distinct().sorted().toList();
            JavacTask task = (JavacTask) compiler.getTask(messages, manager, compilerDiagnostics, options, null,
                    manager.getJavaFileObjectsFromPaths(orderedSources));
            List<CompilationUnitTree> units = new ArrayList<>();
            task.parse().forEach(units::add);
            task.analyze(); // Never call generate(), call(), application code, or processors.
            SourceAnalyzer analyzer = new SourceAnalyzer(root.toAbsolutePath().normalize(), task);
            for (javax.tools.Diagnostic<? extends JavaFileObject> d : compilerDiagnostics.getDiagnostics()) {
                if (d.getKind() != javax.tools.Diagnostic.Kind.ERROR) continue;
                String path = d.getSource() == null ? "<javac>" : analyzer.relative(Path.of(d.getSource().toUri()));
                analyzer.diagnostics.add(map("code", "JAVA_COMPILER", "message", d.getMessage(Locale.ROOT),
                        "location", map("path", path, "line", Math.max(1, d.getLineNumber()),
                                "column", Math.max(1, d.getColumnNumber()))));
            }
            if (!analyzer.diagnostics.isEmpty()) return analyzer.result();
            for (CompilationUnitTree unit : units) analyzer.register(unit);
            analyzer.findBeans();
            for (ClassInfo info : analyzer.classes.values()) {
                if (info.feature != null && analyzer.isRegistered(info.type) && info.type.getModifiers().contains(Modifier.FINAL)) {
                    analyzer.diagnostic("JAVA_RUNTIME_POLICY", "A final feature bean cannot be intercepted by the Spring class proxy", info.path);
                }
            }
            analyzer.findInjections();
            for (Operation operation : List.copyOf(analyzer.methods.values())) analyzer.scan(operation);
            return analyzer.result();
        }
    }

    private Map<String, Object> result() {
        List<Operation> all = new ArrayList<>(methods.values());
        all.addAll(callbacks);
        all.sort(Comparator.comparing(o -> o.id));
        List<Map<String, Object>> serialized = new ArrayList<>();
        for (Operation operation : all) {
            operation.data.put("direct_effects", operation.effects.values().stream().toList());
            serialized.add(operation.data);
        }
        return map("operations", serialized, "diagnostics", diagnostics,
                "inventory", map("version", 2, "classes", classInventory(), "resources", resourceInventory.stream()
                        .sorted(Comparator.comparing(r -> r.get("owner").toString())).toList()));
    }

    private List<Map<String, Object>> classInventory() {
        List<Map<String, Object>> result = new ArrayList<>();
        for (ClassInfo info : classes.values()) {
            Set<String> publicMethods = new TreeSet<>();
            Set<String> unsupported = new TreeSet<>();
            if (!info.type.getKind().isClass() || info.type.getModifiers().contains(Modifier.ABSTRACT)) {
                unsupported.add("Covered type is not a concrete class: " + info.type.getQualifiedName());
            }
            if (!info.type.getTypeParameters().isEmpty()) {
                unsupported.add("Covered class has unmodeled generic type parameter bounds: " + info.type.getQualifiedName());
            }
            List<ExecutableElement> inherited = inheritedMethods(info.type);
            for (Element member : info.type.getEnclosedElements()) {
                if (!(member instanceof ExecutableElement method) || method.getKind() != ElementKind.METHOD
                        || !method.getModifiers().contains(Modifier.PUBLIC)) continue;
                publicMethods.add(qualifiedMethod(method));
                if (!method.getTypeParameters().isEmpty()) {
                    unsupported.add("Public method has unmodeled generic type parameter bounds: " + qualifiedMethod(method));
                }
                if (method.getModifiers().contains(Modifier.NATIVE)) {
                    unsupported.add("Public native method has no analyzable Java body: " + qualifiedMethod(method));
                }
                if (method.getModifiers().contains(Modifier.STATIC) && !entryInitializersSafe(info.type)) {
                    unsupported.add("Public static entry can trigger unmodeled class initialization: " + qualifiedMethod(method));
                }
                for (ExecutableElement parent : inherited) {
                    if (elements.overrides(method, parent, info.type) && differentErasedDescriptor(method, parent)) {
                        unsupported.add("Public override requires an unmodeled compiler bridge: "
                                + qualifiedMethod(method) + " overrides " + qualifiedMethod(parent));
                    }
                }
                if (!methods.containsKey(method)) {
                    unsupported.add("Public method has no analyzed source declaration: " + qualifiedMethod(method));
                }
            }
            for (Element member : elements.getAllMembers(info.type)) {
                if (!(member instanceof ExecutableElement method) || method.getKind() != ElementKind.METHOD
                        || !method.getModifiers().contains(Modifier.PUBLIC)) continue;
                TypeElement owner = (TypeElement) method.getEnclosingElement();
                if (!owner.equals(info.type) && !owner.getQualifiedName().contentEquals("java.lang.Object")) {
                    unsupported.add("Inherited public method is outside covered declared API: " + qualifiedMethod(method));
                }
            }
            result.add(map("name", info.type.getQualifiedName().toString(), "path", location(info.path).get("path"),
                    "public_methods", List.copyOf(publicMethods), "unsupported_api", List.copyOf(unsupported)));
        }
        result.sort(Comparator.comparing(c -> c.get("name").toString()));
        return result;
    }

    private List<ExecutableElement> inheritedMethods(TypeElement type) {
        List<ExecutableElement> result = new ArrayList<>();
        Set<TypeElement> seen = new HashSet<>();
        Deque<TypeMirror> pending = new ArrayDeque<>(types.directSupertypes(type.asType()));
        while (!pending.isEmpty()) {
            TypeMirror next = pending.removeFirst();
            if (!(next instanceof DeclaredType declared) || !(declared.asElement() instanceof TypeElement parent)
                    || !seen.add(parent)) continue;
            for (Element member : parent.getEnclosedElements()) {
                if (member instanceof ExecutableElement method && method.getKind() == ElementKind.METHOD) result.add(method);
            }
            pending.addAll(types.directSupertypes(next));
        }
        return result;
    }

    private boolean differentErasedDescriptor(ExecutableElement method, ExecutableElement parent) {
        // Compare the original declarations, not asMemberOf: substitution would
        // hide exactly the erased signatures for which javac emits a bridge.
        if (!types.isSameType(types.erasure(method.getReturnType()), types.erasure(parent.getReturnType()))) return true;
        if (method.getParameters().size() != parent.getParameters().size()) return true;
        for (int index = 0; index < method.getParameters().size(); index++) {
            if (!types.isSameType(types.erasure(method.getParameters().get(index).asType()),
                    types.erasure(parent.getParameters().get(index).asType()))) return true;
        }
        return false;
    }

    private void register(CompilationUnitTree unit) {
        new TreePathScanner<Void, Void>() {
            @Override public Void visitClass(ClassTree node, Void unused) {
                Element element = trees.getElement(getCurrentPath());
                if (element instanceof TypeElement type) {
                    sourceTypes.add(type);
                    if (type.getQualifiedName().isEmpty()) {
                        if (annotation(type, API + "Feature") != null || annotation(type, API + "ResourceTable") != null) {
                            diagnostic("JAVA_METADATA", "Vitrifi metadata on local or anonymous classes is unsupported", getCurrentPath());
                        }
                        return null;
                    }
                    ClassInfo info = new ClassInfo(type, node, getCurrentPath());
                    classes.put(type, info);
                    info.feature = feature(type, getCurrentPath());
                    AnnotationMirror table = annotation(type, API + "ResourceTable");
                    if (table != null) {
                        String resource = stringValue(table, "value");
                        String name = stringValue(table, "table");
                        resourceInventory.add(map("owner", type.getQualifiedName().toString(),
                                "path", location(getCurrentPath()).get("path"),
                                "table", SqlClassifier.normalizeTable(name), "resource", resource));
                        if (!NAME.matcher(resource).matches() || !TABLE.matcher(name).matches()) {
                            diagnostic("JAVA_METADATA", "ResourceTable requires a resource name and a simple SQL table name", getCurrentPath());
                        } else {
                            name = SqlClassifier.normalizeTable(name);
                            String previous = resourceTables.putIfAbsent(name, resource);
                            if (previous != null && !previous.equals(resource)) {
                                conflictedTables.add(name);
                                diagnostic("JAVA_METADATA", "SQL table " + name + " maps to conflicting resources", getCurrentPath());
                            }
                        }
                    }
                }
                return super.visitClass(node, unused);
            }

            @Override public Void visitMethod(MethodTree node, Void unused) {
                Element element = trees.getElement(getCurrentPath());
                // javac inserts synthetic constructors/accessors; they are not source methods.
                if (element instanceof ExecutableElement method && sourceEnd(getCurrentPath()) >= 0) {
                    TypeElement owner = (TypeElement) method.getEnclosingElement();
                    ClassInfo info = classes.get(owner);
                    if (info == null) return null;
                    Map<String, Object> marker = method.getKind() == ElementKind.METHOD
                            && method.getModifiers().contains(Modifier.PUBLIC) && info != null ? info.feature : null;
                    Operation operation = new Operation(methodId(method), getCurrentPath(), method, node,
                            methodSignature(method), parameters(method), method.getReturnType().toString(), marker, false);
                    methods.put(method, operation);
                    List<Map<String, Object>> policies = policies(method, getCurrentPath());
                    operation.data.put("policies", policies);
                    if (!policies.isEmpty() && (marker == null || method.getModifiers().contains(Modifier.STATIC)
                            || method.getModifiers().contains(Modifier.FINAL) || owner.getModifiers().contains(Modifier.FINAL))) {
                        diagnostic("JAVA_RUNTIME_POLICY", "Policies require a public, nonstatic, nonfinal method of a nonfinal @Feature class/package", getCurrentPath());
                    }
                    if (hasAsyncAdvice(method) || hasAsyncAdvice(owner)) {
                        call(operation, getCurrentPath(), "Spring @Async advice", null, "unknown",
                                "asynchronous Spring advice can execute outside the synchronous policy context; cross-thread effects are not modeled");
                    }
                }
                return super.visitMethod(node, unused);
            }
        }.scan(unit, null);
    }

    private Map<String, Object> feature(TypeElement type, TreePath path) {
        AnnotationMirror marker = annotation(type, API + "Feature");
        if (marker == null) marker = annotation(elements.getPackageOf(type), API + "Feature");
        if (marker == null) return null;
        String name = stringValue(marker, "value");
        String purpose = stringValue(marker, "purpose");
        if (!NAME.matcher(name).matches()) {
            diagnostic("JAVA_METADATA", "Feature value must be a nonempty dotted identifier", path);
            return null;
        }
        String previous = featurePurposes.putIfAbsent(name, purpose);
        if (previous != null && !previous.equals(purpose)) {
            diagnostic("JAVA_METADATA", "Feature " + name + " has conflicting purpose text", path);
        }
        return map("name", name, "purpose", purpose);
    }

    private List<Map<String, Object>> policies(ExecutableElement method, TreePath path) {
        List<Map<String, Object>> result = new ArrayList<>();
        for (AnnotationMirror annotation : expandedAnnotations(method)) {
            String type = annotation.getAnnotationType().toString();
            String kind = switch (type) {
                case API + "Forbid" -> "forbid";
                case API + "NoEffectOnError" -> "no_effect_on_error";
                case API + "NoEffectOnRejection" -> "no_effect_on_rejection";
                default -> null;
            };
            if (kind == null) continue;
            String action = stringValue(annotation, "action");
            String resource = stringValue(annotation, "resource");
            if (!ACTIONS.contains(action) || !NAME.matcher(resource).matches()) {
                diagnostic("JAVA_METADATA", "Policy requires read/write/network/emit and a resource name", path);
                continue;
            }
            Map<String, Object> policy = map("kind", kind, "effect", effect(action, resource));
            if (!result.contains(policy)) result.add(policy);
        }
        return result;
    }

    private void findBeans() {
        for (ClassInfo info : classes.values()) {
            boolean component = info.type.getAnnotationMirrors().stream()
                    .anyMatch(a -> annotationMatches(a, COMPONENTS, new HashSet<>()));
            boolean directComponent = info.type.getAnnotationMirrors().stream()
                    .anyMatch(a -> COMPONENTS.contains(a.getAnnotationType().toString()));
            if (component && !info.type.getModifiers().contains(Modifier.ABSTRACT)
                    && info.type.getKind().isClass()) {
                beans.add(new BeanCandidate(info.type.asType(), info.type, directComponent && !unsupportedWiring(info.type),
                        "source Spring component " + info.type.getQualifiedName()));
            }
        }
        for (Operation operation : methods.values()) {
            ExecutableElement method = operation.method;
            if (annotation(method, BEAN) == null) continue;
            TypeElement concrete = null;
            BlockTree body = operation.tree.getBody();
            if (body != null && body.getStatements().size() == 1
                    && body.getStatements().getFirst() instanceof ReturnTree ret
                    && ret.getExpression() instanceof NewClassTree creation && creation.getClassBody() == null) {
                TypeMirror type = trees.getTypeMirror(TreePath.getPath(operation.path.getCompilationUnit(), creation));
                if (type instanceof DeclaredType declared && declared.asElement() instanceof TypeElement e) concrete = e;
            }
            boolean supported = concrete != null && !unsupportedWiring(method)
                    && !unsupportedWiring(method.getEnclosingElement());
            beans.add(new BeanCandidate(method.getReturnType(), concrete, supported,
                    "source @Bean factory " + methodId(method)));
        }
    }

    private void findInjections() {
        for (ClassInfo info : classes.values()) {
            List<Operation> constructors = methods.values().stream().filter(o -> o.method.getKind() == ElementKind.CONSTRUCTOR
                    && o.method.getEnclosingElement().equals(info.type)).toList();
            List<Operation> annotated = constructors.stream().filter(o -> annotation(o.method, AUTOWIRED) != null).toList();
            Operation selected = constructors.size() == 1 ? constructors.getFirst()
                    : annotated.size() == 1 ? annotated.getFirst() : null;
            Map<VariableElement, Integer> writes = fieldWrites(info);
            if (selected != null && selected.tree.getBody() != null) {
                Set<? extends VariableElement> parameters = new HashSet<>(selected.method.getParameters());
                for (StatementTree statement : selected.tree.getBody().getStatements()) {
                    if (!(statement instanceof ExpressionStatementTree expression)
                            || !(expression.getExpression() instanceof AssignmentTree assignment)) continue;
                    Element left = element(info.path, assignment.getVariable());
                    Element right = element(info.path, assignment.getExpression());
                    if (left instanceof VariableElement field && field.getKind() == ElementKind.FIELD
                            && field.getEnclosingElement().equals(info.type) && right instanceof VariableElement parameter
                            && parameters.contains(parameter)) {
                        boolean stable = field.getModifiers().contains(Modifier.FINAL) && writes.getOrDefault(field, 0) == 1;
                        boolean supported = stable && !unsupportedWiring(parameter) && !unsupportedWiring(selected.method)
                                && !unsupportedWiring(info.type) && isRequired(selected.method);
                        injections.put(field, new Injection(parameter.asType(), supported,
                                supported ? "final field assigned from selected Spring constructor parameter"
                                        : "constructor wiring or field mutation is outside the supported subset"));
                    }
                }
            }
            for (Element member : info.type.getEnclosedElements()) {
                if (member instanceof VariableElement field && field.getKind() == ElementKind.FIELD
                        && annotation(field, AUTOWIRED) != null) {
                    boolean supported = field.getModifiers().contains(Modifier.PRIVATE) && !field.getModifiers().contains(Modifier.STATIC)
                            && writes.getOrDefault(field, 0) == 0 && !unsupportedWiring(field)
                            && !unsupportedWiring(info.type) && isRequired(field);
                    injections.put(field, new Injection(field.asType(), supported,
                            supported ? "@Autowired field with no source reassignment"
                                    : "field injection has unsupported configuration or source reassignment"));
                }
            }
        }
    }

    private Map<VariableElement, Integer> fieldWrites(ClassInfo info) {
        Map<VariableElement, Integer> writes = new HashMap<>();
        new TreePathScanner<Void, Void>() {
            private void record(ExpressionTree target) {
                Element value = element(info.path, target);
                if (value instanceof VariableElement field && field.getKind() == ElementKind.FIELD) writes.merge(field, 1, Integer::sum);
            }
            @Override public Void visitAssignment(AssignmentTree node, Void unused) { record(node.getVariable()); return super.visitAssignment(node, unused); }
            @Override public Void visitCompoundAssignment(CompoundAssignmentTree node, Void unused) { record(node.getVariable()); return super.visitCompoundAssignment(node, unused); }
            @Override public Void visitUnary(UnaryTree node, Void unused) {
                if (Set.of(Tree.Kind.PREFIX_INCREMENT, Tree.Kind.PREFIX_DECREMENT,
                        Tree.Kind.POSTFIX_INCREMENT, Tree.Kind.POSTFIX_DECREMENT).contains(node.getKind())) record(node.getExpression());
                return super.visitUnary(node, unused);
            }
            @Override public Void visitVariable(VariableTree node, Void unused) {
                Element value = trees.getElement(getCurrentPath());
                if (node.getInitializer() != null && value instanceof VariableElement field
                        && field.getKind() == ElementKind.FIELD) writes.merge(field, 1, Integer::sum);
                return super.visitVariable(node, unused);
            }
        // A private field can also be written by a sibling/nesting class in this unit.
        }.scan(info.path.getCompilationUnit(), null);
        return writes;
    }

    private void scan(Operation operation) {
        if (operation.tree != null && operation.tree.getBody() != null) {
            new Calls(operation).scan(new TreePath(operation.path, operation.tree.getBody()), null);
        } else if (operation.method != null && operation.method.getKind() == ElementKind.METHOD) {
            call(operation, operation.path, "unmodeled method body", null, "unknown",
                    "native or abstract method declaration has no Java implementation body to analyze");
        }
    }

    private final class Calls extends TreePathScanner<Void, Void> {
        private final Operation operation;
        private final Set<LambdaExpressionTree> handledCallbacks = Collections.newSetFromMap(new IdentityHashMap<>());
        Calls(Operation operation) { this.operation = operation; }

        @Override public Void visitMethodInvocation(MethodInvocationTree node, Void unused) {
            Element target = trees.getElement(getCurrentPath());
            if (!(target instanceof ExecutableElement method)) {
                call(operation, getCurrentPath(), node.getMethodSelect().toString(), null, "unknown", "javac did not resolve a method symbol");
            } else if (jdbc(operation, getCurrentPath(), node, method)) {
                for (ExpressionTree argument : node.getArguments()) {
                    if (argument instanceof LambdaExpressionTree lambda && rowMapper(lambda, getCurrentPath())) {
                        handledCallbacks.add(lambda);
                        callback(operation, getCurrentPath(), lambda);
                    } else if (jdbcCallback(argument, getCurrentPath())) {
                        call(operation, TreePath.getPath(getCurrentPath().getCompilationUnit(), argument), "JDBC callback", null,
                                "unknown", "JDBC callback is not an inline source RowMapper; its body and dispatch remain unknown");
                    }
                }
            } else if (intrinsic(method, operation.jdbcMapper)) {
                intrinsicCall(operation, getCurrentPath(), node.getMethodSelect().toString(), method,
                        "bounded intrinsic summary for " + qualifiedMethod(method));
            } else {
                Operation declaredTarget = methods.get(method);
                ExpressionTree receiver = node.getMethodSelect() instanceof MemberSelectTree select ? select.getExpression() : null;
                if (declaredTarget != null && (receiver == null || receiver.toString().equals("this") || receiver.toString().equals("super"))
                        && !((List<?>) declaredTarget.data.get("policies")).isEmpty()) {
                    diagnostic("JAVA_RUNTIME_POLICY", "Same-instance call to policy method bypasses Spring proxy interception: " + qualifiedMethod(method), getCurrentPath());
                }
                Resolution resolution = resolve(method, node, getCurrentPath());
                call(operation, getCurrentPath(), node.getMethodSelect().toString(), resolution.target,
                        resolution.target == null ? "unknown" : "resolved", resolution.reason);
            }
            return super.visitMethodInvocation(node, unused);
        }

        @Override public Void visitNewClass(NewClassTree node, Void unused) {
            Element target = trees.getElement(getCurrentPath());
            if (node.getClassBody() != null) {
                call(operation, getCurrentPath(), "new " + node.getIdentifier(), null, "unknown", "anonymous class construction and dispatch are not modeled");
            } else if (target instanceof ExecutableElement constructor) {
                TypeElement owner = (TypeElement) constructor.getEnclosingElement();
                Operation known = methods.get(constructor);
                if (known != null && classInitializersSafe(owner)) {
                    call(operation, getCurrentPath(), "new " + node.getIdentifier(), known.id, "resolved", "javac-resolved source constructor; no unmodeled class initializers");
                } else if (plainRecord(owner) || (standardExceptionConstructor(constructor) || workflowExceptionConstructor(constructor))) {
                    intrinsicCall(operation, getCurrentPath(), "new " + node.getIdentifier(), constructor,
                            "bounded value-record or standard exception construction summary");
                } else {
                    call(operation, getCurrentPath(), "new " + node.getIdentifier(), null, "unknown", "constructor body or initialization effects are not modeled: " + owner.getQualifiedName());
                }
            }
            // Arguments execute now; anonymous class/method bodies do not.
            scan(node.getEnclosingExpression(), unused);
            for (ExpressionTree argument : node.getArguments()) scan(argument, unused);
            return null;
        }

        @Override public Void visitLambdaExpression(LambdaExpressionTree node, Void unused) {
            if (!handledCallbacks.contains(node)) call(operation, getCurrentPath(), "lambda", null, "unknown",
                    "deferred lambda invocation is not modeled by this call adapter");
            return null;
        }
        @Override public Void visitMemberReference(MemberReferenceTree node, Void unused) {
            call(operation, getCurrentPath(), node.toString(), null, "unknown", "method-reference invocation depends on its consumer");
            return null;
        }
        @Override public Void visitTry(TryTree node, Void unused) {
            for (Tree resource : node.getResources()) {
                call(operation, TreePath.getPath(getCurrentPath().getCompilationUnit(), resource), "implicit resource.close()", null,
                        "unknown", "try-with-resources invokes close() implicitly; close dispatch is not modeled");
            }
            return super.visitTry(node, unused);
        }
        @Override public Void visitEnhancedForLoop(EnhancedForLoopTree node, Void unused) {
            TreePath expression = TreePath.getPath(getCurrentPath().getCompilationUnit(), node.getExpression());
            if (trees.getTypeMirror(expression).getKind() != TypeKind.ARRAY) {
                call(operation, expression, "implicit iteration", null, "unknown",
                        "enhanced-for invokes Iterable.iterator/Iterator.hasNext/next; their dispatch is not modeled");
            }
            return super.visitEnhancedForLoop(node, unused);
        }
        @Override public Void visitBinary(BinaryTree node, Void unused) {
            if (node.getKind() == Tree.Kind.PLUS && trees.getTypeMirror(getCurrentPath()).toString().equals("java.lang.String")
                    && (stringConversionMayCall(node.getLeftOperand()) || stringConversionMayCall(node.getRightOperand()))) {
                call(operation, getCurrentPath(), "implicit string conversion", null, "unknown",
                        "String concatenation may invoke a reference value's toString()");
            }
            return super.visitBinary(node, unused);
        }
        @Override public Void visitCompoundAssignment(CompoundAssignmentTree node, Void unused) {
            if (node.getKind() == Tree.Kind.PLUS_ASSIGNMENT && trees.getTypeMirror(getCurrentPath()).toString().equals("java.lang.String")
                    && stringConversionMayCall(node.getExpression())) {
                call(operation, getCurrentPath(), "implicit string conversion", null, "unknown",
                        "String concatenation assignment may invoke a reference value's toString()");
            }
            return super.visitCompoundAssignment(node, unused);
        }
        @Override public Void visitAssert(AssertTree node, Void unused) {
            if (node.getDetail() != null && stringConversionMayCall(node.getDetail())) {
                call(operation, getCurrentPath(), "implicit assertion detail conversion", null, "unknown",
                        "an enabled failed assertion may invoke the detail object's toString()");
            }
            return super.visitAssert(node, unused);
        }
        @Override public Void visitDeconstructionPattern(DeconstructionPatternTree node, Void unused) {
            call(operation, getCurrentPath(), "implicit record-pattern accessors", null, "unknown",
                    "record deconstruction invokes accessors implicitly; pattern dispatch is not modeled");
            return super.visitDeconstructionPattern(node, unused);
        }
        @Override public Void visitIdentifier(IdentifierTree node, Void unused) {
            staticFieldInitialization();
            return super.visitIdentifier(node, unused);
        }
        @Override public Void visitMemberSelect(MemberSelectTree node, Void unused) {
            staticFieldInitialization();
            return super.visitMemberSelect(node, unused);
        }
        private boolean stringConversionMayCall(ExpressionTree expression) {
            TypeMirror type = trees.getTypeMirror(TreePath.getPath(getCurrentPath().getCompilationUnit(), expression));
            return !type.getKind().isPrimitive() && type.getKind() != TypeKind.NULL && !type.toString().equals("java.lang.String");
        }
        private void staticFieldInitialization() {
            Element element = trees.getElement(getCurrentPath());
            if (element instanceof VariableElement field && field.getModifiers().contains(Modifier.STATIC)
                    && field.getConstantValue() == null && field.getEnclosingElement() instanceof TypeElement owner
                    && !classInitializersSafe(owner)) {
                call(operation, getCurrentPath(), "implicit initialization of " + owner.getQualifiedName(), null, "unknown",
                        "nonconstant static field access can trigger unmodeled class initialization");
            }
        }
        @Override public Void visitClass(ClassTree node, Void unused) { return null; }
        @Override public Void visitMethod(MethodTree node, Void unused) { return null; }
    }

    private Resolution resolve(ExecutableElement method, MethodInvocationTree call, TreePath path) {
        String owner = ((TypeElement) method.getEnclosingElement()).getQualifiedName().toString();
        Operation target = methods.get(method);
        boolean exact = method.getModifiers().contains(Modifier.STATIC) || method.getModifiers().contains(Modifier.PRIVATE)
                || method.getModifiers().contains(Modifier.FINAL)
                || method.getEnclosingElement().getModifiers().contains(Modifier.FINAL);
        ExpressionTree receiver = call.getMethodSelect() instanceof MemberSelectTree select ? select.getExpression() : null;
        if (receiver != null && receiver.toString().equals("super")) exact = true;
        if (exact && target != null && method.getKind() != ElementKind.CONSTRUCTOR) {
            if (method.getModifiers().contains(Modifier.STATIC) && !classInitializersSafe((TypeElement) method.getEnclosingElement())) {
                return new Resolution(null, "static source call can trigger unmodeled class initialization: " + owner);
            }
            return new Resolution(target.id, "javac-resolved nonvirtual source method");
        }
        if (method.getKind() == ElementKind.CONSTRUCTOR && owner.equals("java.lang.Object")) {
            // Constructor calls represented as super() are handled as an explicit intrinsic below.
            return new Resolution(null, "Object constructor has no modeled resource effect");
        }
        if (receiver != null) {
            Element value = element(path, receiver);
            if (value instanceof VariableElement field && injections.containsKey(field)) {
                Injection injection = injections.get(field);
                if (!injection.supported) return new Resolution(null, injection.reason);
                TypeElement containingClass = (TypeElement) field.getEnclosingElement();
                if (!isRegistered(containingClass)) return new Resolution(null, "injection belongs to a class not registered in the analyzed Spring component set");
                return beanDispatch(method, injection.type, injection.reason);
            }
            if (receiver instanceof NewClassTree creation && creation.getClassBody() == null) {
                TypeMirror mirror = trees.getTypeMirror(TreePath.getPath(path.getCompilationUnit(), receiver));
                if (mirror instanceof DeclaredType declared && declared.asElement() instanceof TypeElement concrete) {
                    return concreteDispatch(method, concrete, "explicit source new receiver");
                }
            }
        }
        if (receiver == null || receiver.toString().equals("this")) {
            TypeElement containing = enclosingClass(path);
            if (containing != null && isRegistered(containing)) return beanDispatch(method, containing.asType(), "current Spring bean receiver");
        }
        return new Resolution(null, target == null ? "external method body is not modeled: " + qualifiedMethod(method)
                : "javac resolves the declaration, but virtual receiver identity is unknown: " + qualifiedMethod(method));
    }

    private Resolution beanDispatch(ExecutableElement method, TypeMirror receiverType, String origin) {
        List<BeanCandidate> candidates = beans.stream().filter(b -> types.isAssignable(b.exposed, receiverType)
                || b.concrete != null && types.isAssignable(b.concrete.asType(), receiverType)).toList();
        if (candidates.size() != 1) return new Resolution(null, "Spring injection has " + candidates.size()
                + " candidate beans; no target chosen (profiles, qualifiers, external registrations are not inferred)");
        BeanCandidate bean = candidates.getFirst();
        if (!bean.supported || bean.concrete == null) return new Resolution(null, "Spring candidate uses unsupported conditional/factory wiring: " + bean.reason);
        return concreteDispatch(method, bean.concrete, origin + "; " + bean.reason
                + "; assumes analyzed bean registrations are complete and no external proxy/advice changes dispatch");
    }

    private Resolution concreteDispatch(ExecutableElement method, TypeElement concrete, String reason) {
        List<ExecutableElement> candidates = elements.getAllMembers(concrete).stream()
                .filter(e -> e instanceof ExecutableElement).map(e -> (ExecutableElement) e)
                .filter(e -> e.equals(method) || e.getKind() == ElementKind.METHOD && elements.overrides(e, method, concrete))
                .toList();
        if (candidates.size() != 1) return new Resolution(null, "concrete receiver implementation is ambiguous or unavailable: " + concrete);
        Operation target = methods.get(candidates.getFirst());
        if (target == null || target.tree.getBody() == null) return new Resolution(null, "resolved receiver has no analyzed source implementation: " + concrete);
        return new Resolution(target.id, reason);
    }

    private boolean jdbc(Operation operation, TreePath path, MethodInvocationTree node, ExecutableElement method) {
        String owner = ((TypeElement) method.getEnclosingElement()).getQualifiedName().toString();
        if (!Set.of("org.springframework.jdbc.core.JdbcTemplate", "org.springframework.jdbc.core.JdbcOperations").contains(owner)) return false;
        TypeElement jdbcOperations = elements.getTypeElement("org.springframework.jdbc.core.JdbcOperations");
        if (jdbcOperations != null && sourceTypes.stream().anyMatch(t -> types.isAssignable(types.erasure(t.asType()), types.erasure(jdbcOperations.asType())))) {
            call(operation, path, node.getMethodSelect().toString(), null, "unknown",
                    "an analyzed source type customizes JdbcOperations/JdbcTemplate; the standard JDBC summary cannot establish its dispatch");
            return true;
        }
        String name = method.getSimpleName().toString();
        String sql = node.getArguments().isEmpty() ? null : literalString(node.getArguments().getFirst());
        SqlEffect effect = sql == null ? null : sqlEffect(sql, name);
        if (effect == null || conflictedTables.contains(effect.table) || !resourceTables.containsKey(effect.table)) {
            call(operation, path, node.getMethodSelect().toString(), null, "unknown",
                    "JDBC call requires supported single-table literal SELECT/UPDATE/INSERT/DELETE SQL and an unambiguous @ResourceTable mapping");
            return true;
        }
        String resource = resourceTables.get(effect.table);
        operation.effects.put(effect.action + ":" + resource, effect(effect.action, resource));
        call(operation, path, node.getMethodSelect().toString(), null, "intrinsic", "JDBC SQL adapter: " + effect.action
                + " table " + effect.table + " mapped to " + resource
                + "; describes attempted SQL, not triggers, stored procedures, custom JdbcTemplate overrides, or transaction outcome");
        return true;
    }

    private SqlEffect sqlEffect(String raw, String method) {
        try {
            SqlClassifier.Effect effect = SqlClassifier.classify(raw);
            boolean query = Set.of("queryForObject", "query", "queryForList").contains(method);
            if (query && effect.action().equals("read") || method.equals("update") && effect.action().equals("write")) {
                return new SqlEffect(effect.action(), effect.table());
            }
        } catch (IllegalArgumentException unsupported) {
            // Dynamic/unsupported SQL remains an explicit unknown call.
        }
        return null;
    }

    private boolean rowMapper(LambdaExpressionTree lambda, TreePath parent) {
        TypeMirror mirror = trees.getTypeMirror(TreePath.getPath(parent.getCompilationUnit(), lambda));
        return mirror instanceof DeclaredType type && type.asElement() instanceof TypeElement element
                && element.getQualifiedName().contentEquals("org.springframework.jdbc.core.RowMapper");
    }

    private boolean jdbcCallback(ExpressionTree expression, TreePath parent) {
        TypeMirror actual = trees.getTypeMirror(TreePath.getPath(parent.getCompilationUnit(), expression));
        if (actual == null || actual.getKind() == TypeKind.NULL) return false;
        for (String callback : List.of("RowMapper", "RowCallbackHandler", "ResultSetExtractor", "PreparedStatementCreator",
                "PreparedStatementSetter", "ParameterizedPreparedStatementSetter", "BatchPreparedStatementSetter",
                "CallableStatementCreator", "CallableStatementCallback", "ConnectionCallback", "PreparedStatementCallback", "StatementCallback")) {
            TypeElement type = elements.getTypeElement("org.springframework.jdbc.core." + callback);
            if (type != null && types.isAssignable(types.erasure(actual), types.erasure(type.asType()))) return true;
        }
        return false;
    }

    private void callback(Operation caller, TreePath parent, LambdaExpressionTree lambda) {
        TreePath path = TreePath.getPath(parent.getCompilationUnit(), lambda);
        List<Map<String, Object>> params = new ArrayList<>();
        for (VariableTree parameter : lambda.getParameters()) {
            Element element = element(path, parameter);
            params.add(map("name", parameter.getName().toString(), "annotation", element == null ? "unknown" : element.asType().toString()));
        }
        String returns = "unknown";
        TypeMirror lambdaType = trees.getTypeMirror(path);
        if (lambdaType instanceof DeclaredType declared) {
            for (Element member : declared.asElement().getEnclosedElements()) {
                if (member instanceof ExecutableElement method && method.getSimpleName().contentEquals("mapRow")) {
                    returns = ((ExecutableType) types.asMemberOf(declared, method)).getReturnType().toString();
                }
            }
        }
        // Per-method lexical order is unaffected by whitespace and line-number shifts.
        String id = caller.id + "$rowMapper" + ++caller.rowMapperCount;
        Operation operation = new Operation(id, path, null, null, "RowMapper callback -> " + returns, params, returns, null, true);
        callbacks.add(operation);
        call(caller, path, "JdbcTemplate RowMapper callback", id, "resolved", "standard JdbcTemplate query may invoke this synchronous source RowMapper; callback body analyzed separately");
        new Calls(operation).scan(new TreePath(path, lambda.getBody()), null);
    }

    private boolean intrinsic(ExecutableElement method, boolean jdbcMapper) {
        String owner = ((TypeElement) method.getEnclosingElement()).getQualifiedName().toString();
        String name = method.getSimpleName().toString();
        if (owner.equals("java.lang.Object") && method.getKind() == ElementKind.CONSTRUCTOR) return true;
        if (standardExceptionConstructor(method) || workflowExceptionConstructor(method)) return true;
        if (owner.equals("java.lang.String") && Set.of("length", "isEmpty", "isBlank", "strip", "trim", "toLowerCase",
                "toUpperCase", "equals", "equalsIgnoreCase", "startsWith", "endsWith", "substring", "charAt", "compareTo", "concat").contains(name)) return true;
        if (owner.equals("java.lang.Math") && Set.of("min", "max", "abs", "round", "floor", "ceil", "addExact", "subtractExact", "multiplyExact").contains(name)) return true;
        if (Set.of("java.lang.Integer", "java.lang.Long", "java.lang.Double", "java.lang.Boolean").contains(owner)
                && Set.of("valueOf", "parseInt", "parseLong", "parseDouble", "parseBoolean", "intValue", "longValue", "doubleValue", "booleanValue").contains(name)) return true;
        if (owner.equals("java.util.Objects") && name.equals("requireNonNull") && method.getParameters().size() <= 2
                && method.getParameters().stream().noneMatch(p -> p.asType().toString().contains("Supplier"))) return true;
        if (jdbcMapper && owner.equals("java.sql.ResultSet") && Set.of("getString", "getInt", "getLong", "getDouble", "getBoolean", "getBigDecimal", "wasNull").contains(name)) return true;
        TypeElement type = (TypeElement) method.getEnclosingElement();
        return !methods.containsKey(method) && plainRecord(type)
                && type.getRecordComponents().stream().anyMatch(c -> c.getAccessor().equals(method));
    }

    private boolean workflowExceptionConstructor(ExecutableElement method) {
        TypeElement owner = (TypeElement) method.getEnclosingElement();
        if (method.getKind() != ElementKind.CONSTRUCTOR || sourceTypes.contains(owner)) return false;
        String signature = method.getParameters().stream().map(p -> p.asType().toString()).collect(Collectors.joining(","));
        return owner.getQualifiedName().contentEquals("dev.vitrifi.workflow.Rejected") && signature.equals("java.lang.String")
                || owner.getQualifiedName().contentEquals("dev.vitrifi.workflow.Failed") && signature.equals("java.lang.String,java.lang.Throwable");
    }

    private boolean standardExceptionConstructor(ExecutableElement method) {
        String owner = ((TypeElement) method.getEnclosingElement()).getQualifiedName().toString();
        if (method.getKind() != ElementKind.CONSTRUCTOR || !Set.of("java.lang.IllegalArgumentException",
                "java.lang.IllegalStateException", "java.lang.UnsupportedOperationException", "java.lang.RuntimeException",
                "java.lang.Exception", "java.lang.Throwable").contains(owner)) return false;
        // Throwable-taking overloads can call a user-defined cause.toString().
        return method.getParameters().isEmpty() || method.getParameters().size() == 1
                && method.getParameters().getFirst().asType().toString().equals("java.lang.String");
    }

    private boolean plainRecord(TypeElement type) {
        ClassInfo info = classes.get(type);
        return info != null && type.getKind() == ElementKind.RECORD && classInitializersSafe(type)
                && methods.keySet().stream().noneMatch(m -> m.getEnclosingElement().equals(type) && m.getKind() == ElementKind.CONSTRUCTOR);
    }

    private boolean classInitializersSafe(TypeElement type) {
        ClassInfo info = classes.get(type);
        if (info == null) return false;
        for (Tree member : info.tree.getMembers()) {
            if (member instanceof BlockTree) return false;
            if (member instanceof VariableTree variable && variable.getInitializer() != null) {
                Element element = element(info.path, variable);
                if (!(element instanceof VariableElement value) || value.getConstantValue() == null) return false;
            }
        }
        return true;
    }

    private boolean entryInitializersSafe(TypeElement type) {
        if (!classInitializersSafe(type)) return false;
        if (type.getSuperclass() instanceof DeclaredType parent
                && parent.asElement() instanceof TypeElement superclass
                && !superclass.getQualifiedName().contentEquals("java.lang.Object")
                && !entryInitializersSafe(superclass)) return false;
        return defaultInterfaceInitializersSafe(type, new HashSet<>());
    }

    private boolean defaultInterfaceInitializersSafe(TypeElement type, Set<TypeElement> seen) {
        if (!seen.add(type)) return true;
        // Initializing a class also initializes superinterfaces declaring default
        // methods, including those reached through an intermediate interface.
        if (type.getKind() == ElementKind.INTERFACE && type.getEnclosedElements().stream()
                .anyMatch(member -> member.getModifiers().contains(Modifier.DEFAULT))
                && !classInitializersSafe(type)) return false;
        for (TypeMirror mirror : type.getInterfaces()) {
            if (mirror instanceof DeclaredType declared && declared.asElement() instanceof TypeElement parent
                    && !defaultInterfaceInitializersSafe(parent, seen)) return false;
        }
        return true;
    }

    private boolean isRegistered(TypeElement type) {
        return beans.stream().anyMatch(b -> b.supported && type.equals(b.concrete));
    }

    private boolean hasAsyncAdvice(Element element) {
        return element.getAnnotationMirrors().stream().anyMatch(a -> annotationMatches(a, Set.of(ASYNC), new HashSet<>()));
    }

    private boolean unsupportedWiring(Element element) {
        for (AnnotationMirror annotation : element.getAnnotationMirrors()) {
            String name = annotation.getAnnotationType().toString();
            if (WIRING_MODIFIERS.contains(name) || name.startsWith("org.springframework.boot.autoconfigure.condition.")) return true;
            if (annotationMatches(annotation, WIRING_MODIFIERS, new HashSet<>())) return true;
        }
        return false;
    }

    private boolean annotationMatches(AnnotationMirror annotation, Set<String> targetNames, Set<String> visited) {
        String name = annotation.getAnnotationType().toString();
        if (targetNames.contains(name)) return true;
        if (!visited.add(name) || name.startsWith("java.lang.annotation.")) return false;
        return annotation.getAnnotationType().asElement().getAnnotationMirrors().stream()
                .anyMatch(a -> annotationMatches(a, targetNames, visited));
    }

    private boolean isRequired(Element element) {
        AnnotationMirror annotation = annotation(element, AUTOWIRED);
        return annotation == null || !Boolean.FALSE.equals(values(annotation).get("required"));
    }

    private List<AnnotationMirror> expandedAnnotations(Element element) {
        List<AnnotationMirror> result = new ArrayList<>();
        for (AnnotationMirror annotation : element.getAnnotationMirrors()) {
            String name = annotation.getAnnotationType().toString();
            if (Set.of(API + "Forbids", API + "NoEffectsOnError", API + "NoEffectsOnRejection").contains(name)) {
                Object nested = values(annotation).get("value");
                if (nested instanceof List<?> list) for (Object item : list) {
                    if (item instanceof AnnotationValue value && value.getValue() instanceof AnnotationMirror child) result.add(child);
                }
            } else result.add(annotation);
        }
        return result;
    }

    private AnnotationMirror annotation(Element element, String type) {
        if (element == null) return null;
        return element.getAnnotationMirrors().stream().filter(a -> a.getAnnotationType().toString().equals(type)).findFirst().orElse(null);
    }

    private Map<String, Object> values(AnnotationMirror annotation) {
        Map<String, Object> result = new HashMap<>();
        elements.getElementValuesWithDefaults(annotation).forEach((key, value) -> result.put(key.getSimpleName().toString(), value.getValue()));
        return result;
    }

    private String stringValue(AnnotationMirror annotation, String name) {
        Object value = values(annotation).get(name);
        return value instanceof String string ? string : "";
    }

    private static String literalString(ExpressionTree expression) {
        return expression instanceof LiteralTree literal && literal.getValue() instanceof String string ? string : null;
    }

    private TypeElement enclosingClass(TreePath path) {
        for (TreePath current = path; current != null; current = current.getParentPath()) {
            if (current.getLeaf() instanceof ClassTree && trees.getElement(current) instanceof TypeElement type) return type;
        }
        return null;
    }

    private Element element(TreePath within, Tree tree) {
        TreePath path = TreePath.getPath(within.getCompilationUnit(), tree);
        return path == null ? null : trees.getElement(path);
    }

    private String methodId(ExecutableElement method) {
        TreePath path = trees.getPath(method);
        String source = path == null ? "<external>" : relative(Path.of(path.getCompilationUnit().getSourceFile().toUri()));
        return source + "::" + qualifiedMethod(method);
    }

    private String qualifiedMethod(ExecutableElement method) {
        TypeElement owner = (TypeElement) method.getEnclosingElement();
        return owner.getQualifiedName() + "." + method.getSimpleName() + "("
                + method.getParameters().stream().map(p -> types.erasure(p.asType()).toString()).collect(Collectors.joining(",")) + ")";
    }

    private String methodSignature(ExecutableElement method) {
        String name = ((TypeElement) method.getEnclosingElement()).getQualifiedName() + "." + method.getSimpleName();
        String generics = method.getTypeParameters().isEmpty() ? "" : "<" + method.getTypeParameters().stream().map(Object::toString).collect(Collectors.joining(", ")) + "> ";
        String thrown = method.getThrownTypes().isEmpty() ? "" : " throws " + method.getThrownTypes().stream().map(Object::toString).collect(Collectors.joining(", "));
        return generics + method.getReturnType() + " " + name + "("
                + method.getParameters().stream().map(p -> p.asType() + " " + p.getSimpleName()).collect(Collectors.joining(", ")) + ")" + thrown;
    }

    private List<Map<String, Object>> parameters(ExecutableElement method) {
        return method.getParameters().stream().map(p -> map("name", p.getSimpleName().toString(), "annotation", p.asType().toString())).toList();
    }

    private long sourceEnd(TreePath path) { return trees.getSourcePositions().getEndPosition(path.getCompilationUnit(), path.getLeaf()); }

    private Map<String, Object> location(TreePath path) {
        CompilationUnitTree unit = path.getCompilationUnit();
        long position = trees.getSourcePositions().getStartPosition(unit, path.getLeaf());
        return map("path", relative(Path.of(unit.getSourceFile().toUri())),
                "line", position < 0 ? 1 : unit.getLineMap().getLineNumber(position),
                "column", position < 0 ? 1 : unit.getLineMap().getColumnNumber(position));
    }

    private String relative(Path path) {
        Path absolute = path.toAbsolutePath().normalize();
        return absolute.startsWith(root) ? root.relativize(absolute).toString().replace(File.separatorChar, '/') : absolute.toString();
    }

    private void diagnostic(String code, String message, TreePath path) {
        diagnostics.add(map("code", code, "message", message, "location", location(path)));
    }

    private void call(Operation operation, TreePath path, String expression, String target, String resolution, String reason) {
        Map<String, Object> at = location(path);
        operation.calls.add(map("expression", expression, "line", at.get("line"), "column", at.get("column"),
                "target", target, "resolution", resolution, "reason", reason));
    }

    private void intrinsicCall(Operation operation, TreePath path, String expression, ExecutableElement method, String reason) {
        call(operation, path, expression, null, "intrinsic", reason);
        TypeElement owner = (TypeElement) method.getEnclosingElement();
        if (sourceTypes.contains(owner)) {
            operation.calls.getLast().put("source_owner", owner.getQualifiedName().toString());
        }
    }

    private static Map<String, Object> effect(String action, String resource) { return map("action", action, "resource", resource); }

    private static Map<String, Object> map(Object... pairs) {
        Map<String, Object> result = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) result.put((String) pairs[i], pairs[i + 1]);
        return result;
    }

    private final class Operation {
        final String id;
        final TreePath path;
        final ExecutableElement method;
        final MethodTree tree;
        final boolean jdbcMapper;
        final Map<String, Object> data;
        final List<Map<String, Object>> calls = new ArrayList<>();
        final Map<String, Map<String, Object>> effects = new TreeMap<>();
        int rowMapperCount;
        Operation(String id, TreePath path, ExecutableElement method, MethodTree tree, String signature,
                List<Map<String, Object>> parameters, String returns, Map<String, Object> feature, boolean jdbcMapper) {
            this.id = id; this.path = path; this.method = method; this.tree = tree; this.jdbcMapper = jdbcMapper;
            Map<String, Object> at = location(path);
            this.data = map("id", id, "path", at.get("path"), "symbol", id.substring(id.indexOf("::") + 2),
                    "language", "java", "line", at.get("line"), "signature", signature, "parameters", parameters,
                    "returns", returns, "feature", feature, "calls", calls, "direct_effects", List.of(), "policies", List.of());
        }
    }

    private static final class ClassInfo {
        final TypeElement type;
        final ClassTree tree;
        final TreePath path;
        Map<String, Object> feature;
        ClassInfo(TypeElement type, ClassTree tree, TreePath path) { this.type = type; this.tree = tree; this.path = path; }
    }
    private record BeanCandidate(TypeMirror exposed, TypeElement concrete, boolean supported, String reason) {}
    private record Injection(TypeMirror type, boolean supported, String reason) {}
    private record Resolution(String target, String reason) {}
    private record SqlEffect(String action, String table) {}
}
