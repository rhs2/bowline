package com.bowline.billing.statements;

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
import org.springframework.stereotype.Component;

/**
 * Lays out a customer statement of account: header, customer and period, a summary
 * (opening, charges, payments, closing) and the chronological table with a running
 * balance.
 */
@Component
public class StatementPdfRenderer {

    /**
     * Date, Details, Charges, Payments, Balance. A reference and its description used to
     * sit in columns of their own, which left neither wide enough for real content on A4
     * portrait: "Payment" broke across two lines and a description such as "Payment by
     * bank transfer against INV-2026-000101" wrapped three times. They share one Details
     * column instead, the reference in bold above the description, so every line of a
     * movement stays on one line.
     */
    private static final float[] COLUMNS = {1.50f, 6.10f, 1.05f, 1.20f, 1.10f};

    private final BillingProperties.Company company;

    public StatementPdfRenderer(BillingProperties properties) {
        this.company = properties.company();
    }

    public byte[] render(StatementDocument statement) {
        ByteArrayOutputStream out = new ByteArrayOutputStream(16 * 1024);
        Document document = DocumentLayout.a4();
        try {
            PdfWriter writer = PdfWriter.getInstance(document, out);
            writer.setPageEvent(new PageFooter(DocumentLayout.footerCaption(company)));
            document.addTitle("Statement for " + statement.customer().name());
            document.addAuthor(company.name());
            document.addCreator("Bowline billing");
            document.open();

            document.add(DocumentLayout.letterhead(company, "STATEMENT", period(statement)));
            document.add(DocumentLayout.rule());
            document.add(partiesBlock(statement));
            document.add(summaryTable(statement));
            document.add(movementsTable(statement));
            document.add(closingNote(statement));
            document.close();
        } catch (DocumentException e) {
            throw new IllegalStateException("statement layout failed", e);
        }
        return out.toByteArray();
    }

    private static String period(StatementDocument statement) {
        return Dates.format(statement.from()) + " to " + Dates.format(statement.to());
    }

    private static PdfPTable partiesBlock(StatementDocument statement) {
        StatementCustomer customer = statement.customer();
        PdfPTable table = new PdfPTable(new float[] {3f, 2f});
        table.setWidthPercentage(100);

        Phrase phrase = new Phrase();
        phrase.add(new Chunk("CUSTOMER\n", LABEL));
        phrase.add(new Chunk(customer.name() + "\n", BODY_BOLD));
        if (notBlank(customer.code())) {
            phrase.add(new Chunk("Account " + customer.code() + "\n", SMALL));
        }
        if (customer.billingAddress() != null) {
            for (String line : customer.billingAddress().lines()) {
                phrase.add(new Chunk(line + "\n", BODY));
            }
        }
        if (notBlank(customer.contactName())) {
            phrase.add(new Chunk("Attn: " + customer.contactName() + "\n", BODY));
        }
        if (notBlank(customer.contactEmail())) {
            phrase.add(new Chunk(customer.contactEmail() + "\n", BODY));
        }
        PdfPCell left = plain(phrase);
        left.setLeading(0, 1.25f);
        table.addCell(left);

        PdfPTable meta = new PdfPTable(new float[] {1.1f, 1.6f});
        meta.setWidthPercentage(100);
        meta.addCell(plainRight("PERIOD FROM", LABEL));
        meta.addCell(plainRight(Dates.format(statement.from()), BODY));
        meta.addCell(plainRight("PERIOD TO", LABEL));
        meta.addCell(plainRight(Dates.format(statement.to()), BODY));
        meta.addCell(plainRight("CURRENCY", LABEL));
        meta.addCell(plainRight(currency(statement), BODY));
        PdfPCell right = new PdfPCell(meta);
        right.setBorder(Rectangle.NO_BORDER);
        right.setPadding(0);
        table.addCell(right);
        table.setSpacingAfter(12);
        return table;
    }

    private static PdfPTable summaryTable(StatementDocument statement) {
        String cur = currency(statement);
        PdfPTable table = new PdfPTable(4);
        table.setWidthPercentage(100);
        table.addCell(summaryCell("OPENING BALANCE", Money.amount(statement.openingBalance(), cur), BODY_BOLD));
        table.addCell(summaryCell("CHARGES", Money.amount(statement.totalCharges(), cur), BODY_BOLD));
        table.addCell(summaryCell("PAYMENTS", Money.amount(statement.totalPayments(), cur), BODY_BOLD));
        table.addCell(summaryCell("CLOSING BALANCE", Money.amount(statement.closingBalance(), cur), TOTAL));
        table.setSpacingAfter(12);
        return table;
    }

    private static PdfPCell summaryCell(String label, String value, Font valueFont) {
        Phrase phrase = new Phrase();
        phrase.add(new Chunk(label + "\n", LABEL));
        phrase.add(new Chunk(value, valueFont));
        PdfPCell cell = new PdfPCell(phrase);
        cell.setBackgroundColor(PdfStyles.STRIPE_FILL);
        cell.setBorder(Rectangle.BOX);
        cell.setBorderColor(PdfStyles.HEADER_FILL);
        cell.setPadding(7);
        cell.setLeading(0, 1.4f);
        return cell;
    }

    private static PdfPTable movementsTable(StatementDocument statement) {
        PdfPTable table = new PdfPTable(COLUMNS);
        table.setWidthPercentage(100);
        table.setHeaderRows(1);
        table.setSplitLate(false);
        table.addCell(header("Date", Element.ALIGN_LEFT));
        table.addCell(header("Details", Element.ALIGN_LEFT));
        table.addCell(header("Charges", Element.ALIGN_RIGHT));
        table.addCell(header("Payments", Element.ALIGN_RIGHT));
        table.addCell(header("Balance", Element.ALIGN_RIGHT));

        boolean stripe = false;
        addRow(table, Dates.format(statement.from()), details("Opening balance", null), "", "",
                Money.amount(statement.openingBalance()), stripe, BODY_BOLD);
        stripe = !stripe;

        if (statement.lines().isEmpty()) {
            PdfPCell empty = body("No invoices or payments in this period.", Element.ALIGN_LEFT, stripe);
            empty.setColspan(COLUMNS.length);
            table.addCell(empty);
            stripe = !stripe;
        }
        for (StatementDocument.Line line : statement.lines()) {
            StatementEntry e = line.entry();
            boolean invoice = e.kind() == StatementEntry.Kind.INVOICE;
            addRow(table,
                    Dates.format(e.date()),
                    details(e.reference(), e.description()),
                    invoice ? Money.amount(e.debit()) : "",
                    invoice ? "" : Money.amount(e.credit()),
                    Money.amount(line.balance()),
                    stripe, BODY);
            stripe = !stripe;
        }
        addRow(table, Dates.format(statement.to()), details("Closing balance", null),
                Money.amount(statement.totalCharges()), Money.amount(statement.totalPayments()),
                Money.amount(statement.closingBalance()), stripe, BODY_BOLD);
        table.setSpacingAfter(12);
        return table;
    }

    /** Reference in bold with the description underneath; either half may be absent. */
    private static Phrase details(String reference, String description) {
        Phrase phrase = new Phrase();
        if (notBlank(reference)) {
            phrase.add(new Chunk(reference, BODY_BOLD));
        }
        if (notBlank(description)) {
            phrase.add(new Chunk(phrase.isEmpty() ? description : "\n" + description, BODY));
        }
        return phrase;
    }

    private static void addRow(PdfPTable table, String date, Phrase details,
            String charge, String payment, String balance, boolean stripe, Font font) {
        PdfPCell dateCell = body(date, Element.ALIGN_LEFT, stripe);
        dateCell.setPhrase(new Phrase(date, font));
        table.addCell(dateCell);

        PdfPCell detailsCell = body("", Element.ALIGN_LEFT, stripe);
        detailsCell.setPhrase(details);
        detailsCell.setLeading(0, 1.3f);
        table.addCell(detailsCell);

        for (String amount : new String[] {charge, payment, balance}) {
            PdfPCell cell = body(amount, Element.ALIGN_RIGHT, stripe);
            cell.setPhrase(new Phrase(amount, font));
            table.addCell(cell);
        }
    }

    private static Paragraph closingNote(StatementDocument statement) {
        String cur = currency(statement);
        String text;
        if (statement.closingBalance().compareTo(BigDecimal.ZERO) > 0) {
            text = "Balance outstanding as at " + Dates.format(statement.to()) + ": "
                    + Money.amount(statement.closingBalance(), cur)
                    + ". Please quote the invoice number with each remittance.";
        } else if (statement.closingBalance().compareTo(BigDecimal.ZERO) < 0) {
            text = "Your account is in credit by " + Money.amount(statement.closingBalance().negate(), cur)
                    + " as at " + Dates.format(statement.to()) + ".";
        } else {
            text = "Your account is settled in full as at " + Dates.format(statement.to()) + ". Thank you.";
        }
        Paragraph p = new Paragraph(text, BODY);
        p.setLeading(0, 1.3f);
        return p;
    }

    private static String currency(StatementDocument statement) {
        String cur = statement.customer().currency();
        return cur == null || cur.isBlank() ? "USD" : cur.trim();
    }

    private static boolean notBlank(String s) {
        return s != null && !s.isBlank();
    }
}
