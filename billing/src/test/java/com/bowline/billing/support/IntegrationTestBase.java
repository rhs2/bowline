package com.bowline.billing.support;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.actuate.observability.AutoConfigureObservability;
import org.springframework.boot.test.autoconfigure.web.servlet.AutoConfigureMockMvc;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.context.annotation.Import;
import org.springframework.test.context.DynamicPropertyRegistry;
import org.springframework.test.context.DynamicPropertySource;
import org.springframework.test.context.TestPropertySource;
import org.springframework.test.web.servlet.MockMvc;

/**
 * Boots the whole service once for the MockMvc suites: local PDF output into a temp
 * directory, fake repositories, a datasource URL nothing listens on (only /readyz
 * touches it, and it must report unavailable).
 *
 * <p>Spring Boot switches metrics export off inside tests, which would take the
 * Prometheus registry and therefore {@code /metrics} out of the context.
 * {@code @AutoConfigureObservability} puts it back so the scrape endpoint is covered
 * by the suite exactly as it is served in production. Tracing stays off: no tracer is
 * on the classpath.
 */
@SpringBootTest
@AutoConfigureMockMvc
@AutoConfigureObservability(tracing = false)
@Import(TestFakes.class)
@TestPropertySource(properties = {
        "billing.internal-token=" + IntegrationTestBase.TOKEN,
        "billing.pdf-output=local",
        "billing.company.name=Bowline Logistics",
        "billing.company.address=1 Harbour Way, Port City",
        "spring.datasource.url=jdbc:postgresql://127.0.0.1:1/bowline_unused",
        "spring.datasource.username=nobody",
        "spring.datasource.password=",
        "spring.datasource.hikari.connection-timeout=250"
})
public abstract class IntegrationTestBase {

    public static final String TOKEN = "test-internal-token";
    public static final String TOKEN_HEADER = "X-Internal-Token";

    @Autowired
    protected MockMvc mvc;

    @Autowired
    protected ObjectMapper mapper;

    @Autowired
    protected FakeArAgingRepository arAging;

    @Autowired
    protected FakeStatementRepository statements;

    @DynamicPropertySource
    static void outputDirectory(DynamicPropertyRegistry registry) {
        registry.add("billing.local-output-dir", () -> TestOutput.DIR.toString());
    }
}
