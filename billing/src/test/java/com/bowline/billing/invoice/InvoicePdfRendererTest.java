package com.bowline.billing.invoice;

import static org.assertj.core.api.Assertions.assertThat;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.Pdfs;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

/** Pure unit tests of the layout: no Spring context. */
class InvoicePdfRendererTest {

    private final InvoicePdfRenderer renderer = new InvoicePdfRenderer(Fixtures.properties());

    @Test
    void singlePageInvoiceRendersEveryLine() {
        byte[] pdf = renderer.render(Fixtures.invoice());
        assertThat(Pdfs.isPdf(pdf)).isTrue();
        assertThat(Pdfs.pages(pdf)).isEqualTo(1);
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("Customs brokerage and documentation")
                .contains("Warehouse handling, per pallet")
                .contains("1,850.00")
                .contains("12.5")
                .contains("10%")
                .contains("Attn: Dana Whitfield")
                .contains("ap@acme.example")
                .contains("SEA Shanghai to Port City")
                .contains("Harbour Bank");
    }

    @Test
    void longInvoiceSpillsOverPagesWithNumberedFooters() {
        byte[] pdf = renderer.render(Fixtures.longInvoice(120));
        int pages = Pdfs.pages(pdf);
        assertThat(pages).isGreaterThanOrEqualTo(3);
        String text = Pdfs.flatText(pdf);
        assertThat(text).contains("USD 1,200.00");
        assertThat(Pattern.compile("Page 1 of\\s*" + pages).matcher(text).find())
                .as("footer shows the total page count: %s", pages).isTrue();
        assertThat(Pattern.compile("Page " + pages + " of\\s*" + pages).matcher(text).find()).isTrue();
        assertThat(text).contains("Handling charge, item 120");
    }
}
