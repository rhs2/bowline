package com.bowline.billing.support;

import java.time.Clock;
import java.time.Instant;
import java.time.ZoneOffset;
import org.springframework.boot.test.context.TestConfiguration;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Primary;

/** Replaces the JDBC repositories and the clock so the suite needs no database. */
@TestConfiguration
public class TestFakes {

    /** "Today" for every test: 2026-08-27 UTC. */
    public static final Instant NOW = Instant.parse("2026-08-27T09:00:00Z");

    @Bean
    @Primary
    public FakeArAgingRepository fakeArAgingRepository() {
        return new FakeArAgingRepository();
    }

    @Bean
    @Primary
    public FakeStatementRepository fakeStatementRepository() {
        return new FakeStatementRepository();
    }

    @Bean
    @Primary
    public Clock fixedClock() {
        return Clock.fixed(NOW, ZoneOffset.UTC);
    }
}
