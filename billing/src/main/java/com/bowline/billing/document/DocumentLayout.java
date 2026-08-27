package com.bowline.billing.document;

import com.bowline.billing.config.BillingProperties;
import com.lowagie.text.Chunk;
import com.lowagie.text.Document;
import com.lowagie.text.Element;
import com.lowagie.text.PageSize;
import com.lowagie.text.Paragraph;
import com.lowagie.text.Phrase;
import com.lowagie.text.pdf.PdfPCell;
import com.lowagie.text.pdf.PdfPTable;
import com.lowagie.text.pdf.draw.LineSeparator;

/**
 * Page furniture every document the service renders has in common: the A4 page box,
 * the letterhead, the horizontal rule under it and the small labelled paragraph.
 * Invoices, statements and personnel files all sit on this, so they line up on the
 * page and a change to the house style happens in one place.
 */
public final class DocumentLayout {

    /** Side margins; the bottom leaves room for the footer drawn by {@link PageFooter}. */
    public static final float MARGIN = 42f;

    private DocumentLayout() {}

    /** An empty A4 portrait document with the house margins. */
    public static Document a4() {
        return new Document(PageSize.A4, MARGIN, MARGIN, MARGIN + 6, MARGIN + 24);
    }

    /** The caption {@link PageFooter} prints on the left of every page. */
    public static String footerCaption(BillingProperties.Company company) {
        return company.name() + "  |  " + company.address();
    }

    /**
     * Company name and address on the left, the document type and its reference on
     * the right, for example "INVOICE" over "INV-2026-000123".
     */
    public static PdfPTable letterhead(BillingProperties.Company company, String documentType, String reference) {
        PdfPTable table = new PdfPTable(new float[] {3f, 2f});
        table.setWidthPercentage(100);

        Phrase left = new Phrase();
        left.add(new Chunk(company.name() + "\n", PdfStyles.HEADING));
        left.add(new Chunk(company.address(), PdfStyles.SMALL));
        table.addCell(PdfStyles.plain(left));

        Phrase right = new Phrase();
        right.add(new Chunk(documentType + "\n", PdfStyles.TITLE));
        if (reference != null && !reference.isBlank()) {
            right.add(new Chunk(reference, PdfStyles.BODY_BOLD));
        }
        PdfPCell rightCell = PdfStyles.plain(right);
        rightCell.setHorizontalAlignment(Element.ALIGN_RIGHT);
        table.addCell(rightCell);
        table.setSpacingAfter(4);
        return table;
    }

    /** The full-width hairline that closes the letterhead. */
    public static Paragraph rule() {
        Paragraph p = new Paragraph();
        p.add(new Chunk(new LineSeparator(0.6f, 100, PdfStyles.RULE, Element.ALIGN_CENTER, 0)));
        p.setSpacingAfter(10);
        return p;
    }

    /** A small capitalised label with a paragraph of body text under it. */
    public static Paragraph labelled(String label, String body) {
        Paragraph p = new Paragraph();
        p.add(new Chunk(label.toUpperCase() + "\n", PdfStyles.LABEL));
        p.add(new Chunk(body, PdfStyles.BODY));
        p.setLeading(0, 1.3f);
        return p;
    }
}
