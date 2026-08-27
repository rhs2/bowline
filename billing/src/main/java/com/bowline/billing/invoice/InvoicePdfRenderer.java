package com.bowline.billing.invoice;

import static com.bowline.billing.document.PdfStyles.BODY;
import static com.bowline.billing.document.PdfStyles.BODY_BOLD;
import static com.bowline.billing.document.PdfStyles.LABEL;
import static com.bowline.billing.document.PdfStyles.SMALL;
import static com.bowline.billing.document.PdfStyles.TOTAL;
import static com.bowline.billing.document.PdfStyles.body;
import static com.bowline.billing.document.PdfStyles.header;
import static com.bowline.billing.document.PdfStyles.plain;
import static com.bowline.billing.document.PdfStyles.plainRight;

import com.bowline.billing.config.BillingProperties;
import com.bowline.billing.document.Dates;
import com.bowline.billing.document.DocumentLayout;
import com.bowline.billing.document.Money;
import com.bowline.billing.document.PageFooter;
import com.bowline.billing.document.PdfStyles;
import com.lowagie.text.Chunk;
import com.lowagie.text.Document;
import com.lowagie.text.DocumentException;
import com.lowagie.text.Element;
import com.lowagie.text.Font;
import com.lowagie.text.Paragraph;
import com.lowagie.text.Phrase;
import com.lowagie.text.Rectangle;
import com.lowagie.text.pdf.PdfPCell;
import com.lowagie.text.pdf.PdfPTable;
import com.lowagie.text.pdf.PdfWriter;
import java.io.ByteArrayOutputStream;
import java.math.BigDecimal;
import java.time.temporal.ChronoUnit;
import org.springframework.stereotype.Component;

/**
 * Lays out an A4 invoice with OpenPDF: company header, bill-to block, invoice
 * metadata, the lines table (header repeated on every page), totals, payment terms,
 * notes and a footer with page numbers.
 */
@Component
public class InvoicePdfRenderer {

    private static final float[] LINE_WIDTHS = {0.55f, 4.6f, 0.9f, 1.4f, 0.8f, 1.5f};

    private final BillingProperties.Company company;

    public InvoicePdfRenderer(BillingProperties properties) {
        this.company = properties.company();
    }

    public byte[] render(InvoiceRenderRequest request) {
        InvoiceRenderRequest.Invoice inv = request.invoice();
        ByteArrayOutputStream out = new ByteArrayOutputStream(16 * 1024);
        Document document = DocumentLayout.a4();
        try {
            PdfWriter writer = PdfWriter.getInstance(document, out);
            writer.setPageEvent(new PageFooter(DocumentLayout.footerCaption(company)));
            document.addTitle("Invoice " + inv.invoiceNo());
            document.addSubject("Invoice " + inv.invoiceNo() + " for " + request.customer().name());
            document.addAuthor(company.name());
            document.addCreator("Bowline billing");
            document.open();

            document.add(DocumentLayout.letterhead(company, "INVOICE", inv.invoiceNo()));
            document.add(DocumentLayout.rule());
            document.add(partiesBlock(request));
            document.add(linesTable(request));
            document.add(totalsTable(inv));
            document.add(termsParagraph(inv));
            if (inv.notes() != null && !inv.notes().isBlank()) {
                document.add(notesParagraph(inv.notes()));
            }
            document.close();
        } catch (DocumentException e) {
            throw new IllegalStateException("invoice layout failed", e);
        }
        return out.toByteArray();
    }

    private static PdfPTable partiesBlock(InvoiceRenderRequest request) {
        PdfPTable table = new PdfPTable(new float[] {2.6f, 2.4f});
        table.setWidthPercentage(100);
        table.addCell(billTo(request.customer()));
        table.addCell(metadata(request));
        table.setSpacingAfter(14);
        return table;
    }

    private static PdfPCell billTo(InvoiceRenderRequest.Customer customer) {
        Phrase phrase = new Phrase();
        phrase.add(new Chunk("BILL TO\n", LABEL));
        phrase.add(new Chunk(customer.name() + "\n", BODY_BOLD));
        if (customer.code() != null && !customer.code().isBlank()) {
            phrase.add(new Chunk("Account " + customer.code() + "\n", SMALL));
        }
        if (customer.billingAddress() != null) {
            for (String line : customer.billingAddress().lines()) {
                phrase.add(new Chunk(line + "\n", BODY));
            }
        }
        InvoiceRenderRequest.Contact contact = customer.contact();
        if (contact != null) {
            if (notBlank(contact.name())) {
                phrase.add(new Chunk("Attn: " + contact.name() + "\n", BODY));
            }
            if (notBlank(contact.email())) {
                phrase.add(new Chunk(contact.email() + "\n", BODY));
            }
            if (notBlank(contact.phone())) {
                phrase.add(new Chunk(contact.phone() + "\n", BODY));
            }
        }
        PdfPCell cell = plain(phrase);
        cell.setLeading(0, 1.25f);
        return cell;
    }

    private static PdfPCell metadata(InvoiceRenderRequest request) {
        InvoiceRenderRequest.Invoice inv = request.invoice();
        PdfPTable meta = new PdfPTable(new float[] {0.8f, 1.9f});
        meta.setWidthPercentage(100);
        metaRow(meta, "Issue date", Dates.format(inv.issueDate()));
        metaRow(meta, "Due date", Dates.format(inv.dueDate()));
        metaRow(meta, "Currency", inv.currency());
        InvoiceRenderRequest.Shipment shipment = request.shipment();
        if (shipment != null && notBlank(shipment.reference())) {
            metaRow(meta, "Shipment", shipment.reference());
            String route = route(shipment);
            if (!route.isEmpty()) {
                metaRow(meta, "Route", route);
            }
        }
        PdfPCell cell = new PdfPCell(meta);
        cell.setBorder(Rectangle.NO_BORDER);
        cell.setPadding(0);
        return cell;
    }

    private static String route(InvoiceRenderRequest.Shipment shipment) {
        StringBuilder sb = new StringBuilder();
        if (notBlank(shipment.mode())) {
            sb.append(shipment.mode().toUpperCase()).append(' ');
        }
        if (notBlank(shipment.origin()) && notBlank(shipment.destination())) {
            sb.append(shipment.origin()).append(" to ").append(shipment.destination());
        } else if (notBlank(shipment.origin())) {
            sb.append("from ").append(shipment.origin());
        } else if (notBlank(shipment.destination())) {
            sb.append("to ").append(shipment.destination());
        }
        return sb.toString().trim();
    }

    private static void metaRow(PdfPTable table, String label, String value) {
        table.addCell(plainRight(label.toUpperCase(), LABEL));
        table.addCell(plainRight(value, BODY));
    }

    private static PdfPTable linesTable(InvoiceRenderRequest request) {
        PdfPTable table = new PdfPTable(LINE_WIDTHS);
        table.setWidthPercentage(100);
        table.setHeaderRows(1);
        table.setSplitLate(false);
        table.addCell(header("#", Element.ALIGN_LEFT));
        table.addCell(header("Description", Element.ALIGN_LEFT));
        table.addCell(header("Qty", Element.ALIGN_RIGHT));
        table.addCell(header("Unit price", Element.ALIGN_RIGHT));
        table.addCell(header("Tax", Element.ALIGN_RIGHT));
        table.addCell(header("Amount", Element.ALIGN_RIGHT));

        boolean stripe = false;
        for (InvoiceRenderRequest.Line line : request.lines()) {
            table.addCell(body(String.valueOf(line.seq()), Element.ALIGN_LEFT, stripe));
            table.addCell(body(line.description(), Element.ALIGN_LEFT, stripe));
            table.addCell(body(Money.quantity(line.quantity()), Element.ALIGN_RIGHT, stripe));
            table.addCell(body(Money.amount(line.unitPrice()), Element.ALIGN_RIGHT, stripe));
            table.addCell(body(Money.percent(line.taxRate()), Element.ALIGN_RIGHT, stripe));
            table.addCell(body(Money.amount(line.amount()), Element.ALIGN_RIGHT, stripe));
            stripe = !stripe;
        }
        table.setSpacingAfter(8);
        return table;
    }

    private static PdfPTable totalsTable(InvoiceRenderRequest.Invoice inv) {
        PdfPTable table = new PdfPTable(new float[] {1.6f, 1.4f});
        table.setWidthPercentage(42);
        table.setHorizontalAlignment(Element.ALIGN_RIGHT);
        table.setKeepTogether(true);
        String cur = inv.currency();
        totalRow(table, "Subtotal", Money.amount(inv.subtotal(), cur), BODY, false);
        totalRow(table, "Tax", Money.amount(inv.tax(), cur), BODY, false);
        totalRow(table, "Total", Money.amount(inv.total(), cur), BODY_BOLD, true);
        if (inv.amountPaid().compareTo(BigDecimal.ZERO) > 0) {
            totalRow(table, "Amount paid", Money.amount(inv.amountPaid().negate(), cur), BODY, false);
        }
        totalRow(table, "Balance due", Money.amount(inv.balanceDue(), cur), TOTAL, true);
        table.setSpacingAfter(16);
        return table;
    }

    private static void totalRow(PdfPTable table, String label, String value, Font font, boolean ruled) {
        PdfPCell labelCell = plain(label, font);
        PdfPCell valueCell = plainRight(value, font);
        labelCell.setPadding(4);
        valueCell.setPadding(4);
        if (ruled) {
            for (PdfPCell cell : new PdfPCell[] {labelCell, valueCell}) {
                cell.setBorder(Rectangle.TOP);
                cell.setBorderColor(PdfStyles.RULE);
                cell.setBorderWidth(0.6f);
            }
        }
        table.addCell(labelCell);
        table.addCell(valueCell);
    }

    private static Paragraph termsParagraph(InvoiceRenderRequest.Invoice inv) {
        long days = ChronoUnit.DAYS.between(inv.issueDate(), inv.dueDate());
        String terms = days == 0 ? "Payment is due on receipt" : "Net " + days + " days: payment is due by " + Dates.format(inv.dueDate());
        String balance = inv.balanceDue().compareTo(BigDecimal.ZERO) == 0
                ? "This invoice has been paid in full; no payment is required."
                : "Please quote " + inv.invoiceNo() + " with your remittance.";
        Paragraph p = DocumentLayout.labelled("Payment terms", terms + ". " + balance);
        p.setSpacingAfter(10);
        return p;
    }

    private static Paragraph notesParagraph(String notes) {
        return DocumentLayout.labelled("Notes", notes.trim());
    }

    private static boolean notBlank(String s) {
        return s != null && !s.isBlank();
    }
}
