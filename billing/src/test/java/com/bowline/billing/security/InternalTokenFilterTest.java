package com.bowline.billing.security;

import static org.hamcrest.Matchers.containsString;
import static org.hamcrest.Matchers.not;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.content;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.header;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.IntegrationTestBase;
import java.util.List;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;

class InternalTokenFilterTest extends IntegrationTestBase {

    @BeforeEach
    void rows() {
        arAging.reset(List.of());
    }

    @Test
    void missingTokenIsRejectedWithProblemJson() throws Exception {
        mvc.perform(get("/reports/ar-aging.xlsx"))
                .andExpect(status().isUnauthorized())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(header().exists("X-Request-Id"))
                .andExpect(jsonPath("$.status").value(401))
                .andExpect(jsonPath("$.code").value("unauthorized"))
                .andExpect(jsonPath("$.title").value("Unauthorized"))
                .andExpect(jsonPath("$.request_id").isString());
    }

    @Test
    void wrongTokenIsRejected() throws Exception {
        mvc.perform(get("/reports/ar-aging.xlsx").header(TOKEN_HEADER, "not-the-token"))
                .andExpect(status().isUnauthorized())
                .andExpect(jsonPath("$.code").value("unauthorized"));
    }

    @Test
    void wrongTokenOnRenderIsRejectedBeforeTheBodyIsRead() throws Exception {
        mvc.perform(post("/render/invoice")
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(Fixtures.invoice()))
                        .header(TOKEN_HEADER, "not-the-token"))
                .andExpect(status().isUnauthorized());
    }

    @Test
    void correctTokenIsAccepted() throws Exception {
        mvc.perform(get("/reports/ar-aging.xlsx").header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isOk());
    }

    @Test
    void callerRequestIdIsEchoed() throws Exception {
        mvc.perform(get("/reports/ar-aging.xlsx").header("X-Request-Id", "req-abc-123"))
                .andExpect(status().isUnauthorized())
                .andExpect(header().string("X-Request-Id", "req-abc-123"))
                .andExpect(jsonPath("$.request_id").value("req-abc-123"));
    }

    @Test
    void probesAndMetricsNeedNoToken() throws Exception {
        mvc.perform(get("/healthz"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.status").value("ok"));
        mvc.perform(get("/metrics"))
                .andExpect(status().isOk())
                .andExpect(content().string(containsString("jvm_memory_used_bytes")))
                .andExpect(content().string(not(containsString("unauthorized"))));
    }

    @Test
    void readinessReportsTheUnreachableDatabase() throws Exception {
        mvc.perform(get("/readyz"))
                .andExpect(status().isServiceUnavailable())
                .andExpect(jsonPath("$.status").value("unavailable"))
                .andExpect(jsonPath("$.database").isString());
    }
}
