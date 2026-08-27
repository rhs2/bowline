package com.bowline.billing.invoice;

import static org.assertj.core.api.Assertions.assertThat;
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
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;
import org.springframework.mock.web.MockHttpServletResponse;

class InvoiceRenderEndpointTest extends IntegrationTestBase {

    @Test
    void inlineRenderReturnsAPdfWithTheRequestedFigures() throws Exception {
        MockHttpServletResponse response = mvc.perform(post("/render/invoice")
                        .param("inline", "1")
                        .header(TOKEN_HEADER, TOKEN)
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(Fixtures.invoice("INV-2026-000777"))))
                .andExpect(status().isOk())
                .andExpect(content().contentType(MediaType.APPLICATION_PDF))
                .andExpect(header().string("Content-Disposition", "inline; filename=\"INV-2026-000777.pdf\""))
                .andReturn().getResponse();

        byte[] pdf = response.getContentAsByteArray();
        assertThat(Pdfs.isPdf(pdf)).as("starts with %PDF").isTrue();
        assertThat(pdf.length).isGreaterThan(2_000).isLessThan(200_000);
        assertThat(Pdfs.pages(pdf)).isEqualTo(1);

        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("Bowline Logistics")
                .contains("1 Harbour Way, Port City")
                .contains("INVOICE")
                .contains("INV-2026-000777")
                .contains("Acme Trading Co.")
                .contains("Unit 12, 400 Wharf Road")
                .contains("BWL-2026-000456")
                .contains("Sea freight, Shanghai to Port City")
                .contains("USD 4,280.00")
                .contains("USD 58.00")
                .contains("USD 4,338.00")
                .contains("USD -1,000.00")
                .contains("USD 3,338.00")
                .contains("Net 30 days")
                .contains("31 Aug 2026")
                .contains("Page 1 of");

        Path notStored = TestOutput.DIR.resolve("invoices/INV-2026-000777.pdf");
        assertThat(notStored).as("a preview is never stored").doesNotExist();
    }

    @Test
    void renderStoresThePdfAndReturnsItsKey() throws Exception {
        MockHttpServletResponse response = mvc.perform(post("/render/invoice")
                        .header(TOKEN_HEADER, TOKEN)
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(Fixtures.invoice())))
                .andExpect(status().isOk())
                .andExpect(content().contentTypeCompatibleWith(MediaType.APPLICATION_JSON))
                .andExpect(jsonPath("$.s3_key").value("invoices/INV-2026-000123.pdf"))
                .andReturn().getResponse();

        JsonNode body = mapper.readTree(response.getContentAsString());
        long bytes = body.get("bytes").asLong();
        assertThat(bytes).isGreaterThan(2_000);

        Path stored = TestOutput.DIR.resolve("invoices/INV-2026-000123.pdf");
        assertThat(stored).exists();
        byte[] onDisk = Files.readAllBytes(stored);
        assertThat(onDisk.length).isEqualTo(bytes);
        assertThat(Pdfs.isPdf(onDisk)).isTrue();
        assertThat(Pdfs.flatText(onDisk)).contains("INV-2026-000123").contains("USD 3,338.00");
    }

    @Test
    void renderingTwiceReplacesTheStoredFile() throws Exception {
        String body = mapper.writeValueAsString(Fixtures.invoice("INV-2026-000124"));
        for (int i = 0; i < 2; i++) {
            mvc.perform(post("/render/invoice")
                            .header(TOKEN_HEADER, TOKEN)
                            .contentType(MediaType.APPLICATION_JSON)
                            .content(body))
                    .andExpect(status().isOk())
                    .andExpect(jsonPath("$.s3_key").value("invoices/INV-2026-000124.pdf"));
        }
        assertThat(TestOutput.DIR.resolve("invoices/INV-2026-000124.pdf")).exists();
        try (var files = Files.list(TestOutput.DIR.resolve("invoices"))) {
            assertThat(files.filter(p -> p.getFileName().toString().startsWith(".partial-")).count())
                    .as("no temp files left behind").isZero();
        }
    }

    @Test
    void fullyPaidInvoiceSaysSo() throws Exception {
        InvoiceRenderRequest paid = Fixtures.invoice("INV-2026-000125");
        InvoiceRenderRequest.Invoice h = paid.invoice();
        InvoiceRenderRequest.Invoice settled = new InvoiceRenderRequest.Invoice(
                h.invoiceNo(), h.issueDate(), h.dueDate(), h.currency(), h.subtotal(), h.tax(), h.total(), h.total(), null);
        byte[] pdf = mvc.perform(post("/render/invoice")
                        .param("inline", "true")
                        .header(TOKEN_HEADER, TOKEN)
                        .contentType(MediaType.APPLICATION_JSON)
                        .content(mapper.writeValueAsString(new InvoiceRenderRequest(settled, paid.customer(), null, paid.lines()))))
                .andExpect(status().isOk())
                .andReturn().getResponse().getContentAsByteArray();
        assertThat(Pdfs.flatText(pdf)).contains("USD 0.00").contains("paid in full");
    }
}
