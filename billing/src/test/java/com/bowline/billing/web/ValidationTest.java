package com.bowline.billing.web;

import static org.hamcrest.Matchers.hasItem;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.content;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.IntegrationTestBase;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.ResultActions;

class ValidationTest extends IntegrationTestBase {

    private ObjectNode validBody() {
        return mapper.valueToTree(Fixtures.invoice());
    }

    private ResultActions render(ObjectNode body) throws Exception {
        return mvc.perform(post("/render/invoice")
                .header(TOKEN_HEADER, TOKEN)
                .contentType(MediaType.APPLICATION_JSON)
                .content(mapper.writeValueAsString(body)));
    }

    private static ResultActions expectValidationProblem(ResultActions actions) throws Exception {
        return actions
                .andExpect(status().isUnprocessableEntity())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(jsonPath("$.status").value(422))
                .andExpect(jsonPath("$.code").value("validation_failed"))
                .andExpect(jsonPath("$.request_id").isString());
    }

    @Test
    void missingInvoiceNumberIsReportedWithItsSnakeCasePath() throws Exception {
        ObjectNode body = validBody();
        ((ObjectNode) body.get("invoice")).remove("invoice_no");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[*].field", hasItem("invoice.invoice_no")));
    }

    @Test
    void badLineFiguresAreReportedPerLine() throws Exception {
        ObjectNode body = validBody();
        ObjectNode line = (ObjectNode) body.get("lines").get(1);
        line.put("unit_price", "-5.00");
        line.put("tax_rate", "1.5");
        line.put("description", "");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[*].field", hasItem("lines[1].unit_price")))
                .andExpect(jsonPath("$.errors[*].field", hasItem("lines[1].tax_rate")))
                .andExpect(jsonPath("$.errors[*].field", hasItem("lines[1].description")));
    }

    @Test
    void emptyLinesAreRejected() throws Exception {
        ObjectNode body = validBody();
        body.putArray("lines");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[*].field", hasItem("lines")));
    }

    @Test
    void totalsThatDoNotAddUpAreRejected() throws Exception {
        ObjectNode body = validBody();
        ((ObjectNode) body.get("invoice")).put("total", "9999.00");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[0].field").value("invoice.total"))
                .andExpect(jsonPath("$.errors[0].message").value("must equal subtotal + tax"));
    }

    @Test
    void overpaymentAndBackwardsDatesAreRejected() throws Exception {
        ObjectNode body = validBody();
        ObjectNode invoice = (ObjectNode) body.get("invoice");
        invoice.put("amount_paid", "5000.00");
        invoice.put("due_date", "2026-07-01");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[*].field", hasItem("invoice.amount_paid")))
                .andExpect(jsonPath("$.errors[*].field", hasItem("invoice.due_date")));
    }

    @Test
    void duplicateLineSequenceIsRejected() throws Exception {
        ObjectNode body = validBody();
        ((ObjectNode) body.get("lines").get(2)).put("seq", 1);
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[0].field").value("lines[2].seq"));
    }

    @Test
    void invoiceNumberCannotEscapeTheKeyPrefix() throws Exception {
        ObjectNode body = validBody();
        ((ObjectNode) body.get("invoice")).put("invoice_no", "../../etc/passwd");
        expectValidationProblem(render(body))
                .andExpect(jsonPath("$.errors[0].field").value("invoice.invoice_no"));
    }

    @Test
    void malformedJsonIsA400() throws Exception {
        mvc.perform(post("/render/invoice")
                        .header(TOKEN_HEADER, TOKEN)
                        .contentType(MediaType.APPLICATION_JSON)
                        .content("{\"invoice\": "))
                .andExpect(status().isBadRequest())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(jsonPath("$.code").value("malformed_request"));
    }

    @Test
    void unknownRouteIsA404Problem() throws Exception {
        mvc.perform(get("/nope").header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isNotFound())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(jsonPath("$.code").value("not_found"));
    }

    @Test
    void wrongMethodIsA405Problem() throws Exception {
        mvc.perform(get("/render/invoice").header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isMethodNotAllowed())
                .andExpect(jsonPath("$.code").value("method_not_allowed"));
    }

    @Test
    void unparseableReportDateIsA422() throws Exception {
        expectValidationProblem(mvc.perform(get("/reports/ar-aging.xlsx").param("as_of", "yesterday").header(TOKEN_HEADER, TOKEN)))
                .andExpect(jsonPath("$.errors[0].field").value("as_of"));
    }

    @Test
    void statementParametersAreValidated() throws Exception {
        expectValidationProblem(mvc.perform(get("/statements/not-a-uuid.pdf")
                        .param("from", "2026-01-01").param("to", "2026-02-01").header(TOKEN_HEADER, TOKEN)))
                .andExpect(jsonPath("$.errors[0].field").value("customerId"));
        expectValidationProblem(mvc.perform(get("/statements/" + Fixtures.CUSTOMER_ID + ".pdf")
                        .param("to", "2026-02-01").header(TOKEN_HEADER, TOKEN)))
                .andExpect(jsonPath("$.errors[0].field").value("from"));
    }
}
