package com.bowline.billing.health;

import java.sql.Connection;
import java.time.Duration;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import javax.sql.DataSource;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Answers "can we reach the database right now?" within a hard deadline. The JDBC call
 * runs on its own thread so a hung TCP connect cannot stall the readiness probe.
 */
public class DatabaseReadinessCheck {

    /** Outcome of one probe. */
    public record Result(boolean ready, String detail) {
        public static Result ok() {
            return new Result(true, "ok");
        }

        public static Result failed(String detail) {
            return new Result(false, detail);
        }
    }

    private static final Logger log = LoggerFactory.getLogger(DatabaseReadinessCheck.class);

    private final DataSource dataSource;
    private final Duration timeout;
    private final ExecutorService executor;

    public DatabaseReadinessCheck(DataSource dataSource, Duration timeout) {
        this.dataSource = dataSource;
        this.timeout = timeout;
        this.executor = Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "readiness-probe");
            t.setDaemon(true);
            return t;
        });
    }

    public Result check() {
        Future<Boolean> probe = executor.submit(this::isValid);
        try {
            boolean valid = probe.get(timeout.toMillis(), TimeUnit.MILLISECONDS);
            return valid ? Result.ok() : Result.failed("connection is not valid");
        } catch (TimeoutException e) {
            probe.cancel(true);
            return Result.failed("timed out after " + timeout.toMillis() + " ms");
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return Result.failed("interrupted");
        } catch (Exception e) {
            Throwable cause = e.getCause() == null ? e : e.getCause();
            log.debug("readiness probe failed: {}", cause.toString());
            return Result.failed(cause.getClass().getSimpleName());
        }
    }

    private boolean isValid() throws Exception {
        int seconds = (int) Math.max(1, timeout.toSeconds());
        try (Connection connection = dataSource.getConnection()) {
            return connection.isValid(seconds);
        }
    }
}
