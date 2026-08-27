package com.bowline.billing.health;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.anyInt;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.when;

import java.sql.Connection;
import java.sql.SQLException;
import java.time.Duration;
import javax.sql.DataSource;
import org.junit.jupiter.api.Test;

class DatabaseReadinessCheckTest {

    @Test
    void reachableDatabaseIsReady() throws Exception {
        DataSource dataSource = mock(DataSource.class);
        Connection connection = mock(Connection.class);
        when(dataSource.getConnection()).thenReturn(connection);
        when(connection.isValid(anyInt())).thenReturn(true);

        DatabaseReadinessCheck.Result result = new DatabaseReadinessCheck(dataSource, Duration.ofSeconds(1)).check();
        assertThat(result.ready()).isTrue();
        assertThat(result.detail()).isEqualTo("ok");
    }

    @Test
    void connectionFailureIsNotReady() throws Exception {
        DataSource dataSource = mock(DataSource.class);
        when(dataSource.getConnection()).thenThrow(new SQLException("connection refused"));

        DatabaseReadinessCheck.Result result = new DatabaseReadinessCheck(dataSource, Duration.ofSeconds(1)).check();
        assertThat(result.ready()).isFalse();
        assertThat(result.detail()).isEqualTo("SQLException");
    }

    @Test
    void slowDatabaseTimesOutWithinTheDeadline() throws Exception {
        DataSource dataSource = mock(DataSource.class);
        when(dataSource.getConnection()).thenAnswer(invocation -> {
            Thread.sleep(5_000);
            return mock(Connection.class);
        });

        long started = System.nanoTime();
        DatabaseReadinessCheck.Result result = new DatabaseReadinessCheck(dataSource, Duration.ofMillis(200)).check();
        long elapsedMs = (System.nanoTime() - started) / 1_000_000;

        assertThat(result.ready()).isFalse();
        assertThat(result.detail()).startsWith("timed out");
        assertThat(elapsedMs).isLessThan(2_000);
    }

    @Test
    void invalidConnectionIsNotReady() throws Exception {
        DataSource dataSource = mock(DataSource.class);
        Connection connection = mock(Connection.class);
        when(dataSource.getConnection()).thenReturn(connection);
        when(connection.isValid(anyInt())).thenReturn(false);

        DatabaseReadinessCheck.Result result = new DatabaseReadinessCheck(dataSource, Duration.ofSeconds(1)).check();
        assertThat(result.ready()).isFalse();
        assertThat(result.detail()).contains("not valid");
    }
}
