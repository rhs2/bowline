package com.bowline.billing.statements;

import static org.assertj.core.api.Assertions.assertThat;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.content;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.header;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.IntegrationTestBase;
import com.bowline.billing.support.Pdfs;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;
import java.util.UUID;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;

class StatementEndpointTest extends IntegrationTestBase {

    @BeforeEach
    void customers() {
        statements.reset();
        statements.addCustomer(Fixtures.statementCustomer(), new BigDecimal("500.00"), Fixtures.statementEntries());
    }

    @Test
    void statementListsInvoicesAndPaymentsWithARunningBalance() throws Exception {
        byte[] pdf = mvc.perform(get("/statements/" + Fixtures.CUSTOMER_ID + ".pdf")
                        .param("from", "2026-07-01").param("to", "2026-08-31")
                        .header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isOk())
                .andExpect(content().contentType(MediaType.APPLICATION_PDF))
                .andExpect(header().string("Content-Disposition",
                        "inline; filename=\"statement-ACME-2026-07-01-2026-08-31.pdf\""))
                .andReturn().getResponse().getContentAsByteArray();

        assertThat(Pdfs.isPdf(pdf)).isTrue();
        assertThat(pdf.length).isGreaterThan(2_000);
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("STATEMENT")
                .contains("Acme Trading Co.")
                .contains("Account ACME")
                .contains("1 Jul 2026 to 31 Aug 2026")
                .contains("Opening balance")
                .contains("INV-2026-000101")
                .contains("TRX-889")
                .contains("Payment by bank transfer against INV-2026-000101")
                .contains("INV-2026-000150")
                .contains("Closing balance")
                .contains("USD 500.00")
                .contains("USD 5,538.00")
                .contains("USD 1,000.00")
                .contains("USD 5,038.00")
                .contains("Balance outstanding as at 31 Aug 2026: USD 5,038.00")
                .contains("Page 1 of");
    }

    @Test
    void windowFiltersTheEntries() throws Exception {
        byte[] pdf = mvc.perform(get("/statements/" + Fixtures.CUSTOMER_ID + ".pdf")
                        .param("from", "2026-08-01").param("to", "2026-08-31")
                        .header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isOk())
                .andReturn().getResponse().getContentAsByteArray();
        String text = Pdfs.flatText(pdf);
        assertThat(text).contains("INV-2026-000150").doesNotContain("INV-2026-000101");
    }

    @Test
    void quietPeriodSaysSo() throws Exception {
        byte[] pdf = mvc.perform(get("/statements/" + Fixtures.CUSTOMER_ID + ".pdf")
                        .param("from", "2025-01-01").param("to", "2025-01-31")
                        .header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isOk())
                .andReturn().getResponse().getContentAsByteArray();
        assertThat(Pdfs.flatText(pdf)).contains("No invoices or payments in this period.");
    }

    @Test
    void unknownCustomerIsA404Problem() throws Exception {
        mvc.perform(get("/statements/" + UUID.randomUUID() + ".pdf")
                        .param("from", "2026-07-01").param("to", "2026-08-31")
                        .header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isNotFound())
                .andExpect(content().contentTypeCompatibleWith("application/problem+json"))
                .andExpect(jsonPath("$.code").value("not_found"));
    }

    @Test
    void backwardsWindowIsA422Problem() throws Exception {
        mvc.perform(get("/statements/" + Fixtures.CUSTOMER_ID + ".pdf")
                        .param("from", "2026-08-31").param("to", "2026-07-01")
                        .header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isUnprocessableEntity())
                .andExpect(jsonPath("$.code").value("validation_failed"))
                .andExpect(jsonPath("$.errors[0].field").value("from"));
    }

    @Test
    void runningBalancesAreComputedInOrder() {
        StatementDocument doc = StatementDocument.build(
                Fixtures.statementCustomer(), LocalDate.of(2026, 7, 1), LocalDate.of(2026, 8, 31),
                new BigDecimal("500.00"), Fixtures.statementEntries());
        assertThat(doc.lines()).extracting(l -> l.balance().toPlainString())
                .containsExactly("4838.00", "3838.00", "5038.00");
        assertThat(doc.totalCharges()).isEqualByComparingTo("5538.00");
        assertThat(doc.totalPayments()).isEqualByComparingTo("1000.00");
        assertThat(doc.closingBalance()).isEqualByComparingTo("5038.00");

        StatementDocument empty = StatementDocument.build(
                Fixtures.statementCustomer(), LocalDate.of(2026, 7, 1), LocalDate.of(2026, 8, 31),
                BigDecimal.ZERO, List.of());
        assertThat(empty.lines()).isEmpty();
        assertThat(empty.closingBalance()).isEqualByComparingTo("0");
    }
}
