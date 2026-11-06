package dev.vitrifi.spring;

import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.sql.CallableStatement;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.List;
import javax.sql.DataSource;

/** Intercepts the driver boundary instead of trusting repository annotations as effect facts. */
final class JdbcInstrumentation {
    private JdbcInstrumentation() {}

    static DataSource wrap(DataSource source, VitrifiRuntime runtime) {
        return (DataSource) Proxy.newProxyInstance(DataSource.class.getClassLoader(), new Class<?>[]{DataSource.class},
                (proxy, method, args) -> {
                    Object wrapper = wrapperMethod(proxy, method, args, runtime);
                    if (wrapper != UNHANDLED) {
                        return wrapper;
                    }
                    Object result = call(method, source, args);
                    if (result instanceof Connection connection && method.getName().equals("getConnection")) {
                        return connection(connection, runtime);
                    }
                    return result;
                });
    }

    private static Connection connection(Connection target, VitrifiRuntime runtime) {
        return (Connection) Proxy.newProxyInstance(Connection.class.getClassLoader(), new Class<?>[]{Connection.class},
                (proxy, method, args) -> {
                    Object wrapper = wrapperMethod(proxy, method, args, runtime);
                    if (wrapper != UNHANDLED) {
                        return wrapper;
                    }
                    if (runtime.inFeatureScope() && (method.getName().equals("setSchema") || method.getName().equals("setCatalog"))) {
                        unsupported(runtime, "Changing the database namespace inside a feature is unsupported");
                    }
                    Object result = call(method, target, args);
                    if (result instanceof Statement statement) {
                        String sql = args != null && args.length > 0 && args[0] instanceof String ? (String) args[0] : null;
                        return statement(statement, sql, (Connection) proxy, runtime);
                    }
                    if (result instanceof DatabaseMetaData metadata) {
                        return metadata(metadata, (Connection) proxy, runtime);
                    }
                    return result;
                });
    }

    private static Statement statement(Statement target, String preparedSql, Connection connection, VitrifiRuntime runtime) {
        Class<?> iface = target instanceof CallableStatement ? CallableStatement.class
                : target instanceof PreparedStatement ? PreparedStatement.class : Statement.class;
        List<String> batch = new ArrayList<>();
        return (Statement) Proxy.newProxyInstance(iface.getClassLoader(), new Class<?>[]{iface},
                (proxy, method, args) -> {
                    Object wrapper = wrapperMethod(proxy, method, args, runtime);
                    if (wrapper != UNHANDLED) {
                        return wrapper;
                    }
                    String name = method.getName();
                    if (name.equals("getConnection")) {
                        return connection;
                    }
                    if (name.equals("addBatch")) {
                        Object result = call(method, target, args);
                        batch.add(sql(preparedSql, args));
                        return result;
                    }
                    if (name.equals("clearBatch")) {
                        Object result = call(method, target, args);
                        batch.clear();
                        return result;
                    }
                    if (name.equals("executeBatch") || name.equals("executeLargeBatch")) {
                        List<VitrifiRuntime.JdbcEvent> events = runtime.prepareSql(new ArrayList<>(batch));
                        events.forEach(event -> event.attempted = true);
                        try {
                            Object result = call(method, target, args);
                            events.forEach(event -> event.outcome = "succeeded");
                            return result;
                        } catch (Throwable failure) {
                            events.forEach(event -> event.outcome = "failed");
                            throw failure;
                        } finally {
                            batch.clear();
                        }
                    }
                    if (name.equals("execute") || name.equals("executeQuery") || name.equals("executeUpdate") || name.equals("executeLargeUpdate")) {
                        // A statement may fail or roll back later; it remains an execution attempt.
                        List<VitrifiRuntime.JdbcEvent> events = runtime.prepareSql(java.util.Collections.singletonList(sql(preparedSql, args)));
                        events.forEach(event -> event.attempted = true);
                        try {
                            Object result = call(method, target, args);
                            events.forEach(event -> event.outcome = "succeeded");
                            return result instanceof ResultSet rows ? resultSet(rows, (Statement) proxy, runtime) : result;
                        } catch (Throwable failure) {
                            events.forEach(event -> event.outcome = "failed");
                            throw failure;
                        }
                    }
                    Object result = call(method, target, args);
                    return result instanceof ResultSet rows ? resultSet(rows, (Statement) proxy, runtime) : result;
                });
    }

    private static ResultSet resultSet(ResultSet target, Statement statement, VitrifiRuntime runtime) {
        return (ResultSet) Proxy.newProxyInstance(ResultSet.class.getClassLoader(), new Class<?>[]{ResultSet.class},
                (proxy, method, args) -> {
                    Object wrapper = wrapperMethod(proxy, method, args, runtime);
                    if (wrapper != UNHANDLED) {
                        return wrapper;
                    }
                    if (method.getName().equals("getStatement")) {
                        return statement;
                    }
                    if (runtime.inFeatureScope() && (method.getName().equals("updateRow")
                            || method.getName().equals("insertRow") || method.getName().equals("deleteRow"))) {
                        unsupported(runtime, "Updatable ResultSet effects are outside the instrumented SQL subset");
                    }
                    return call(method, target, args);
                });
    }

    private static DatabaseMetaData metadata(DatabaseMetaData target, Connection connection, VitrifiRuntime runtime) {
        return (DatabaseMetaData) Proxy.newProxyInstance(DatabaseMetaData.class.getClassLoader(), new Class<?>[]{DatabaseMetaData.class},
                (proxy, method, args) -> {
                    Object wrapper = wrapperMethod(proxy, method, args, runtime);
                    if (wrapper != UNHANDLED) {
                        return wrapper;
                    }
                    if (method.getName().equals("getConnection")) {
                        return connection;
                    }
                    return call(method, target, args);
                });
    }

    private static final Object UNHANDLED = new Object();

    private static Object wrapperMethod(Object proxy, Method method, Object[] args, VitrifiRuntime runtime) throws SQLFeatureNotSupportedException {
        if (method.getName().equals("isWrapperFor")) {
            return ((Class<?>) args[0]).isInstance(proxy);
        }
        if (method.getName().equals("unwrap")) {
            Class<?> requested = (Class<?>) args[0];
            if (requested.isInstance(proxy)) {
                return proxy;
            }
            if (runtime.inFeatureScope()) {
                runtime.unsupported("Unwrapping the instrumented JDBC boundary is unsupported");
            }
            throw new SQLFeatureNotSupportedException("Cannot unwrap an instrumented Vitrifi JDBC boundary");
        }
        return UNHANDLED;
    }

    private static String sql(String preparedSql, Object[] args) {
        return args != null && args.length > 0 && args[0] instanceof String ? (String) args[0] : preparedSql;
    }

    private static Object call(Method method, Object target, Object[] args) throws Throwable {
        try {
            return method.invoke(target, args);
        } catch (InvocationTargetException invocation) {
            throw invocation.getCause();
        }
    }

    private static void unsupported(VitrifiRuntime runtime, String message) {
        runtime.unsupported(message);
        throw new IncompleteInstrumentationException(message);
    }
}
