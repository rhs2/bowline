package com.bowline.billing.employee;

import static org.assertj.core.api.Assertions.assertThat;
import static org.hamcrest.Matchers.hasItem;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.content;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.header;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.IntegrationTestBase;
import com.bowline.billing.support.Pdfs;
import com.bowline.billing.support.TestOutput;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;
import org.springframework.mock.web.MockHttpServletResponse;
import org.springframework.test.web.servlet.ResultActions;

class EmployeeDocumentRenderEndpointTest extends IntegrationTestBase {

    private ResultActions render(Object body) throws Exception {
        return mvc.perform(post("/render/document")
                .header(TOKEN_HEADER, TOKEN)
                .contentType(MediaType.APPLICATION_JSON)
                .content(mapper.writeValueAsString(body)));
    }

    @Test
    void withoutTheInternalTokenNothingIsRendered() throws Exception {
        ObjectNode body = mapper.valueToTree(Fixtures.contract());
        String key = Fixtures.documentKey("contract-guarded.pdf");
        body.put("s3_key", key);

        mvc.perform(post("/render/document")
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(body)))
                .andExpect(status().isUnauthorized())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(jsonPath("$.code").value("unauthorized"));

        mvc.perform(post("/render/document")
                        .header(TOKEN_HEADER, "not-the-token")
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(body)))
                .andExpect(status().isUnauthorized());

        assertThat(TestOutput.DIR.resolve(key)).doesNotExist();
    }

    @Test
    void contractIsStoredUnderTheKeyTheCallerAsksFor() throws Exception {
        String key = Fixtures.documentKey("contract.pdf");
        MockHttpServletResponse response = render(Fixtures.contract())
                .andExpect(status().isOk())
                .andExpect(content().contentTypeCompatibleWith(MediaType.APPLICATION_JSON))
                .andExpect(jsonPath("$.s3_key").value(key))
                .andReturn().getResponse();

        JsonNode body = mapper.readTree(response.getContentAsString());
        long bytes = body.get("bytes").asLong();
        assertThat(bytes).isGreaterThan(2_000);

        Path stored = TestOutput.DIR.resolve(key);
        assertThat(stored).exists();
        byte[] onDisk = Files.readAllBytes(stored);
        assertThat(onDisk.length).isEqualTo(bytes);
        assertThat(Pdfs.isPdf(onDisk)).as("starts with %PDF").isTrue();
        assertThat(Pdfs.flatText(onDisk))
                .contains("EMPLOYMENT CONTRACT")
                .contains("Priya Raman")
                .contains("USD 68,400.00");
    }

    @Test
    void everyKindRendersARealPdfWithItsFigures() throws Exception {
        for (Object[] each : new Object[][] {
                {Fixtures.payslip(), "PAYSLIP", "USD 4,104.00"},
                {Fixtures.certificate(), "CERTIFICATE", "PCSI-DG-88214"},
                {Fixtures.identityDocument(), "IDENTITY RECORD", "X4419078"}}) {
            EmployeeDocumentRequest request = (EmployeeDocumentRequest) each[0];
            String key = render(request)
                    .andExpect(status().isOk())
                    .andExpect(jsonPath("$.s3_key").value(request.s3Key()))
                    .andReturn().getResponse().getContentAsString();
            assertThat(key).contains(request.s3Key());

            byte[] onDisk = Files.readAllBytes(TestOutput.DIR.resolve(request.s3Key()));
            assertThat(Pdfs.isPdf(onDisk)).isTrue();
            assertThat(Pdfs.flatText(onDisk))
                    .as("%s carries its own figures", request.kind())
                    .contains((String) each[1])
                    .contains((String) each[2])
                    .contains("Page 1 of");
        }
    }

    @Test
    void inlineReturnsTheBytesAndStoresNothing() throws Exception {
        EmployeeDocumentRequest request = new EmployeeDocumentRequest(
                "payslip",
                Fixtures.documentKey("payslip-2026-06.pdf"),
                "Payslip 2026-06",
                Fixtures.employee(),
                null,
                Fixtures.payslip().payslip(),
                null,
                null);

        byte[] pdf = mvc.perform(post("/render/document")
                        .param("inline", "1")
                        .header(TOKEN_HEADER, TOKEN)
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(request)))
                .andExpect(status().isOk())
                .andExpect(content().contentType(MediaType.APPLICATION_PDF))
                .andExpect(header().string("Content-Disposition", "inline; filename=\"payslip-2026-06.pdf\""))
                .andReturn().getResponse().getContentAsByteArray();

        assertThat(Pdfs.isPdf(pdf)).isTrue();
        assertThat(Pdfs.flatText(pdf)).contains("USD 4,104.00");
        assertThat(TestOutput.DIR.resolve(request.s3Key())).as("a preview is never stored").doesNotExist();
    }

    @Test
    void renderingTwiceReplacesTheStoredFile() throws Exception {
        EmployeeDocumentRequest request = Fixtures.identityDocument();
        for (int i = 0; i < 2; i++) {
            render(request).andExpect(status().isOk()).andExpect(jsonPath("$.s3_key").value(request.s3Key()));
        }
        Path stored = TestOutput.DIR.resolve(request.s3Key());
        assertThat(stored).exists();
        try (var files = Files.list(stored.getParent())) {
            assertThat(files.filter(p -> p.getFileName().toString().startsWith(".partial-")).count())
                    .as("no temp files left behind").isZero();
        }
    }

    @Test
    void anUnknownKindIsRejected() throws Exception {
        ObjectNode body = mapper.valueToTree(Fixtures.contract());
        body.put("kind", "other");
        render(body)
                .andExpect(status().isUnprocessableEntity())
                .andExpect(jsonPath("$.code").value("validation_failed"))
                .andExpect(jsonPath("$.errors[*].field", hasItem("kind")));
    }

    @Test
    void theDetailBlockMustMatchTheKind() throws Exception {
        ObjectNode body = mapper.valueToTree(Fixtures.contract());
        body.remove("contract");
        render(body)
                .andExpect(status().isUnprocessableEntity())
                .andExpect(jsonPath("$.errors[0].field").value("contract"))
                .andExpect(jsonPath("$.errors[0].message").value("is required for kind contract"));
    }

    @Test
    void payslipFiguresMustAddUp() throws Exception {
        ObjectNode body = mapper.valueToTree(Fixtures.payslip());
        ((ObjectNode) body.get("payslip")).put("net", "9999.00");
        render(body)
                .andExpect(status().isUnprocessableEntity())
                .andExpect(jsonPath("$.errors[0].field").value("payslip.net"))
                .andExpect(jsonPath("$.errors[0].message").value("must equal gross - deductions"));
    }

    @Test
    void keysCannotEscapeTheEmployeePrefix() throws Exception {
        for (String key : new String[] {
                "invoices/INV-2026-000123.pdf",
                "employees/../../etc/passwd.pdf",
                "employees/" + Fixtures.EMPLOYEE_ID + "/contract.exe"}) {
            ObjectNode body = mapper.valueToTree(Fixtures.contract());
            body.put("s3_key", key);
            render(body)
                    .andExpect(status().isUnprocessableEntity())
                    .andExpect(jsonPath("$.errors[*].field", hasItem("s3_key")));
        }
    }

    @Test
    void missingEmployeeDetailsAreReportedWithSnakeCasePaths() throws Exception {
        ObjectNode body = mapper.valueToTree(Fixtures.payslip());
        ((ObjectNode) body.get("employee")).remove("employee_no");
        ((ObjectNode) body.get("payslip")).put("period", "July");
        render(body)
                .andExpect(status().isUnprocessableEntity())
                .andExpect(jsonPath("$.errors[*].field", hasItem("employee.employee_no")))
                .andExpect(jsonPath("$.errors[*].field", hasItem("payslip.period")));
    }
}
