package com.bowline.billing.employee;

import static com.bowline.billing.document.PdfStyles.BODY;
import static com.bowline.billing.document.PdfStyles.BODY_BOLD;
import static com.bowline.billing.document.PdfStyles.HEADING;
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
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import org.springframework.stereotype.Component;

/**
 * Lays out the four personnel files on the same A4 furniture the invoice and the
 * statement use: letterhead, rule, an employee block, the body the kind calls for and
 * the page footer. Every document says on the page that it is demonstration data.
 */
@Component
public class EmployeeDocumentPdfRenderer {

    /** Printed in the footer of every page, and again at the foot of the document. */
    static final String DEMO_NOTICE = "Generated demo document for a fictional company";

    private static final String DEFAULT_CURRENCY = "USD";
    private static final int DEFAULT_WEEKLY_HOURS = 40;
    private static final int DEFAULT_NOTICE_DAYS = 30;

    private final BillingProperties.Company company;

    public EmployeeDocumentPdfRenderer(BillingProperties properties) {
        this.company = properties.company();
    }

    public byte[] render(EmployeeDocumentKind kind, EmployeeDocumentRequest request) {
        EmployeeDocumentRequest.Employee employee = request.employee();
        ByteArrayOutputStream out = new ByteArrayOutputStream(8 * 1024);
        Document document = DocumentLayout.a4();
        try {
            PdfWriter writer = PdfWriter.getInstance(document, out);
            writer.setPageEvent(new PageFooter(DocumentLayout.footerCaption(company) + "  |  " + DEMO_NOTICE));
            document.addTitle(request.title());
            document.addSubject(heading(kind) + " for " + employee.name());
            document.addAuthor(company.name());
            document.addCreator("Bowline billing");
            document.open();

            document.add(DocumentLayout.letterhead(company, heading(kind), reference(kind, request)));
            document.add(DocumentLayout.rule());
            document.add(employeeBlock(kind, request));
            switch (kind) {
                case CONTRACT -> contract(document, request);
                case PAYSLIP -> payslip(document, request);
                case CERTIFICATE -> certificate(document, request);
                case ID -> identity(document, request);
            }
            document.add(demoNote());
            document.close();
        } catch (DocumentException e) {
            throw new IllegalStateException(kind.wireName() + " layout failed", e);
        }
        return out.toByteArray();
    }

    private static String heading(EmployeeDocumentKind kind) {
        return switch (kind) {
            case CONTRACT -> "EMPLOYMENT CONTRACT";
            case PAYSLIP -> "PAYSLIP";
            case CERTIFICATE -> "CERTIFICATE";
            case ID -> "IDENTITY RECORD";
        };
    }

    private static String reference(EmployeeDocumentKind kind, EmployeeDocumentRequest request) {
        if (kind == EmployeeDocumentKind.PAYSLIP && request.payslip() != null) {
            return request.payslip().period();
        }
        return request.employee().employeeNo();
    }

    // -----------------------------------------------------------------------
    // Shared blocks
    // -----------------------------------------------------------------------

    /** Who the file is about on the left, the document's own metadata on the right. */
    private static PdfPTable employeeBlock(EmployeeDocumentKind kind, EmployeeDocumentRequest request) {
        EmployeeDocumentRequest.Employee employee = request.employee();
        PdfPTable table = new PdfPTable(new float[] {2.7f, 2.3f});
        table.setWidthPercentage(100);

        Phrase phrase = new Phrase();
        phrase.add(new Chunk("EMPLOYEE\n", LABEL));
        phrase.add(new Chunk(employee.name() + "\n", BODY_BOLD));
        phrase.add(new Chunk("Employee number " + employee.employeeNo() + "\n", SMALL));
        addLine(phrase, employee.positionTitle());
        addLine(phrase, employee.department());
        addLine(phrase, employee.site());
        addLine(phrase, employee.email());
        PdfPCell left = plain(phrase);
        left.setLeading(0, 1.25f);
        table.addCell(left);

        List<String[]> meta = new ArrayList<>();
        meta.add(new String[] {"Document", request.title()});
        switch (kind) {
            case CONTRACT -> {
                EmployeeDocumentRequest.Contract contract = request.contract();
                meta.add(new String[] {"Start date", Dates.format(contract.startDate())});
                meta.add(new String[] {"Employment", humanise(contract.employmentType())});
                meta.add(new String[] {"Pay grade", contract.payGrade()});
            }
            case PAYSLIP -> {
                EmployeeDocumentRequest.Payslip payslip = request.payslip();
                meta.add(new String[] {"Pay period", payslip.period()});
                meta.add(new String[] {"Pay date", Dates.format(payslip.payDate())});
            }
            case CERTIFICATE -> {
                EmployeeDocumentRequest.Certificate certificate = request.certificate();
                meta.add(new String[] {"Issued on", Dates.format(certificate.issuedOn())});
                meta.add(new String[] {"Expires on", expiry(certificate.expiresOn())});
            }
            case ID -> {
                EmployeeDocumentRequest.Identity identity = request.identity();
                meta.add(new String[] {"Issued on", Dates.format(identity.issuedOn())});
                meta.add(new String[] {"Expires on", expiry(identity.expiresOn())});
            }
        }
        addLine(meta, "Manager", employee.managerName());

        PdfPTable metaTable = new PdfPTable(new float[] {1.1f, 1.7f});
        metaTable.setWidthPercentage(100);
        for (String[] row : meta) {
            if (notBlank(row[1])) {
                metaTable.addCell(plainRight(row[0].toUpperCase(Locale.ENGLISH), LABEL));
                metaTable.addCell(plainRight(row[1], BODY));
            }
        }
        PdfPCell right = new PdfPCell(metaTable);
        right.setBorder(Rectangle.NO_BORDER);
        right.setPadding(0);
        table.addCell(right);
        table.setSpacingAfter(14);
        return table;
    }

    /** A two column "term and detail" table; rows with no value are left out. */
    private static PdfPTable detailsTable(String firstHeader, String secondHeader, List<String[]> rows) {
        PdfPTable table = new PdfPTable(new float[] {2.2f, 4.8f});
        table.setWidthPercentage(100);
        table.setHeaderRows(1);
        table.setSplitLate(false);
        table.addCell(header(firstHeader, Element.ALIGN_LEFT));
        table.addCell(header(secondHeader, Element.ALIGN_LEFT));
        boolean stripe = false;
        for (String[] row : rows) {
            if (!notBlank(row[1])) {
                continue;
            }
            table.addCell(body(row[0], Element.ALIGN_LEFT, stripe));
            table.addCell(body(row[1], Element.ALIGN_LEFT, stripe));
            stripe = !stripe;
        }
        table.setSpacingAfter(14);
        return table;
    }

    /** A numbered clause: bold heading, then the text of the clause. */
    private static Paragraph clause(int number, String title, String text) {
        Paragraph p = new Paragraph();
        p.add(new Chunk(number + ". " + title + "  ", BODY_BOLD));
        p.add(new Chunk(text, BODY));
        p.setLeading(0, 1.35f);
        p.setSpacingAfter(7);
        return p;
    }

    /** Two signature lines side by side, for the company and for the employee. */
    private static PdfPTable signatures(String employeeName) {
        PdfPTable table = new PdfPTable(new float[] {1f, 1f});
        table.setWidthPercentage(100);
        table.setSpacingBefore(18);
        table.setKeepTogether(true);
        table.addCell(signature("For and on behalf of the company", "Authorised signatory"));
        table.addCell(signature("Employee", employeeName));
        return table;
    }

    private static PdfPCell signature(String label, String who) {
        Phrase phrase = new Phrase();
        phrase.add(new Chunk("\n\n" + "_".repeat(34) + "\n", BODY));
        phrase.add(new Chunk(label + "\n", LABEL));
        phrase.add(new Chunk(who + "\n", BODY));
        phrase.add(new Chunk("Date: " + "_".repeat(18), SMALL));
        PdfPCell cell = plain(phrase);
        cell.setLeading(0, 1.3f);
        cell.setPaddingRight(18);
        return cell;
    }

    private static Paragraph demoNote() {
        Paragraph p = new Paragraph(
                DEMO_NOTICE + ". Bowline Logistics is not a real business and this file carries no legal weight.",
                SMALL);
        p.setSpacingBefore(16);
        p.setLeading(0, 1.3f);
        return p;
    }

    // -----------------------------------------------------------------------
    // Employment contract
    // -----------------------------------------------------------------------

    private void contract(Document document, EmployeeDocumentRequest request) throws DocumentException {
        EmployeeDocumentRequest.Contract contract = request.contract();
        EmployeeDocumentRequest.Employee employee = request.employee();
        String currency = currency(contract.currency());
        String salary = Money.amount(contract.salary(), currency);
        int weeklyHours = contract.weeklyHours() == null ? DEFAULT_WEEKLY_HOURS : contract.weeklyHours();
        int noticeDays = contract.noticeDays() == null ? DEFAULT_NOTICE_DAYS : contract.noticeDays();
        String department = notBlank(contract.department()) ? contract.department() : employee.department();
        String site = notBlank(contract.site()) ? contract.site() : employee.site();

        document.add(DocumentLayout.labelled(
                "Agreement",
                "This agreement is made between " + company.name() + " of " + company.address()
                        + " (the company) and " + employee.name() + " (the employee), and sets out the terms on which "
                        + "the employee is engaged."));

        List<String[]> terms = new ArrayList<>();
        terms.add(new String[] {"Position", contract.title()});
        terms.add(new String[] {"Department", department});
        terms.add(new String[] {"Reports to", employee.managerName()});
        terms.add(new String[] {"Place of work", site});
        terms.add(new String[] {"Start date", Dates.format(contract.startDate())});
        terms.add(new String[] {"Employment type", humanise(contract.employmentType())});
        terms.add(new String[] {"Pay grade", contract.payGrade()});
        terms.add(new String[] {"Annual salary", salary});
        terms.add(new String[] {"Hours per week", String.valueOf(weeklyHours)});
        terms.add(new String[] {"Notice period", noticeDays + " days"});
        PdfPTable table = detailsTable("Term", "Detail", terms);
        table.setSpacingBefore(12);
        document.add(table);

        document.add(clause(1, "Appointment",
                "The company appoints the employee as " + contract.title()
                        + (notBlank(department) ? " in the " + department + " department" : "")
                        + ", with effect from " + Dates.format(contract.startDate())
                        + ". The appointment is " + humanise(contract.employmentType()).toLowerCase(Locale.ENGLISH)
                        + " and continues until ended by either party under clause 6."));
        document.add(clause(2, "Duties",
                "The employee will carry out the duties of the position faithfully and diligently, follow reasonable "
                        + "instructions from the company, and observe the policies in the employee handbook as they "
                        + "are amended from time to time."));
        document.add(clause(3, "Remuneration",
                "The salary is " + salary + " a year"
                        + (notBlank(contract.payGrade()) ? " at pay grade " + contract.payGrade() : "")
                        + ", paid monthly in arrears on the last working day of the month, less deductions the company "
                        + "is required by law to make. The salary is reviewed once a year."));
        document.add(clause(4, "Hours and place of work",
                "Normal working time is " + weeklyHours + " hours a week"
                        + (notBlank(site) ? ", worked at " + site : "")
                        + ". The employee may be asked to work reasonable additional hours where operations require it."));
        document.add(clause(5, "Leave and absence",
                "The employee is entitled to the annual leave, public holidays and sick leave set out in the handbook. "
                        + "Leave is requested through the platform and approved by the employee's manager."));
        document.add(clause(6, "Notice and termination",
                "Either party may end this agreement by giving " + noticeDays
                        + " days written notice. The company may end it without notice for gross misconduct."));
        document.add(clause(7, "Confidentiality",
                "The employee will keep confidential the company's commercial information, customer records and "
                        + "operating data, during the engagement and after it ends, except where disclosure is "
                        + "required by law."));
        document.add(clause(8, "Entire agreement",
                "This document, with the employee handbook, is the whole of the agreement between the parties and "
                        + "replaces any earlier understanding."));
        document.add(signatures(employee.name()));
    }

    // -----------------------------------------------------------------------
    // Payslip
    // -----------------------------------------------------------------------

    private void payslip(Document document, EmployeeDocumentRequest request) throws DocumentException {
        EmployeeDocumentRequest.Payslip payslip = request.payslip();
        String currency = currency(payslip.currency());

        document.add(summaryStrip(payslip, currency));

        PdfPTable table = new PdfPTable(new float[] {5f, 2f});
        table.setWidthPercentage(100);
        table.setHeaderRows(1);
        table.setSplitLate(false);
        table.addCell(header("Description", Element.ALIGN_LEFT));
        table.addCell(header("Amount", Element.ALIGN_RIGHT));

        amountRow(table, "Basic pay for " + payslip.period(), Money.amount(payslip.gross(), currency), BODY, false);
        amountRow(table, "Gross pay", Money.amount(payslip.gross(), currency), BODY_BOLD, true);
        amountRow(table, "Tax and statutory contributions", Money.amount(payslip.deductions().negate(), currency),
                BODY, false);
        amountRow(table, "Total deductions", Money.amount(payslip.deductions().negate(), currency), BODY_BOLD, true);
        amountRow(table, "Net pay", Money.amount(payslip.net(), currency), TOTAL, false);
        table.setSpacingAfter(14);
        document.add(table);

        List<String[]> details = new ArrayList<>();
        details.add(new String[] {"Pay period", payslip.period()});
        details.add(new String[] {"Period start", Dates.format(payslip.periodStart())});
        details.add(new String[] {"Period end", Dates.format(payslip.periodEnd())});
        details.add(new String[] {"Pay date", Dates.format(payslip.payDate())});
        details.add(new String[] {"Payment method", humanise(payslip.payMethod())});
        details.add(new String[] {"Currency", currency});
        document.add(detailsTable("Pay details", "Value", details));

        String paidOn = notBlank(Dates.format(payslip.payDate())) ? " on " + Dates.format(payslip.payDate()) : "";
        document.add(DocumentLayout.labelled(
                "Payment",
                "Net pay of " + Money.amount(payslip.net(), currency) + " was paid"
                        + (notBlank(payslip.payMethod()) ? " by " + humanise(payslip.payMethod()).toLowerCase(Locale.ENGLISH) : "")
                        + paidOn + " to the account the company holds on file. Keep this payslip for your records."));
    }

    /** Gross, deductions and net across the top, the way the statement summarises a period. */
    private static PdfPTable summaryStrip(EmployeeDocumentRequest.Payslip payslip, String currency) {
        PdfPTable table = new PdfPTable(3);
        table.setWidthPercentage(100);
        table.addCell(summaryCell("GROSS PAY", Money.amount(payslip.gross(), currency), BODY_BOLD));
        table.addCell(summaryCell("DEDUCTIONS", Money.amount(payslip.deductions(), currency), BODY_BOLD));
        table.addCell(summaryCell("NET PAY", Money.amount(payslip.net(), currency), TOTAL));
        table.setSpacingAfter(14);
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

    private static void amountRow(PdfPTable table, String label, String amount, Font font, boolean ruled) {
        PdfPCell labelCell = body(label, Element.ALIGN_LEFT, false);
        labelCell.setPhrase(new Phrase(label, font));
        PdfPCell amountCell = body(amount, Element.ALIGN_RIGHT, false);
        amountCell.setPhrase(new Phrase(amount, font));
        amountCell.setHorizontalAlignment(Element.ALIGN_RIGHT);
        if (ruled) {
            for (PdfPCell cell : new PdfPCell[] {labelCell, amountCell}) {
                cell.setBackgroundColor(PdfStyles.STRIPE_FILL);
            }
        }
        table.addCell(labelCell);
        table.addCell(amountCell);
    }

    // -----------------------------------------------------------------------
    // Certificate
    // -----------------------------------------------------------------------

    private void certificate(Document document, EmployeeDocumentRequest request) throws DocumentException {
        EmployeeDocumentRequest.Certificate certificate = request.certificate();
        EmployeeDocumentRequest.Employee employee = request.employee();

        Paragraph attestation = new Paragraph();
        attestation.setAlignment(Element.ALIGN_CENTER);
        attestation.setLeading(0, 1.5f);
        attestation.setSpacingBefore(18);
        attestation.add(new Chunk("This is to certify that\n\n", BODY));
        attestation.add(new Chunk(employee.name() + "\n", HEADING));
        attestation.add(new Chunk("employee number " + employee.employeeNo() + "\n\n", SMALL));
        attestation.add(new Chunk("holds the qualification\n\n", BODY));
        attestation.add(new Chunk(certificate.name() + "\n", BODY_BOLD));
        attestation.setSpacingAfter(20);
        document.add(attestation);

        List<String[]> details = new ArrayList<>();
        details.add(new String[] {"Qualification", certificate.name()});
        details.add(new String[] {"Issued by", certificate.issuer()});
        details.add(new String[] {"Issued on", Dates.format(certificate.issuedOn())});
        details.add(new String[] {"Valid until", expiry(certificate.expiresOn())});
        details.add(new String[] {"Reference", certificate.reference()});
        details.add(new String[] {"Held by", employee.name()});
        details.add(new String[] {"Position", employee.positionTitle()});
        document.add(detailsTable("Detail", "Value", details));

        document.add(DocumentLayout.labelled(
                "Verification",
                "The People team verified the original certificate and holds it on the employee's file. "
                        + (certificate.expiresOn() == null
                                ? "The qualification does not carry an expiry date."
                                : "The employee must renew it before " + Dates.format(certificate.expiresOn()) + ".")));
        document.add(signatures(employee.name()));
    }

    // -----------------------------------------------------------------------
    // Identity record
    // -----------------------------------------------------------------------

    private void identity(Document document, EmployeeDocumentRequest request) throws DocumentException {
        EmployeeDocumentRequest.Identity identity = request.identity();
        EmployeeDocumentRequest.Employee employee = request.employee();

        document.add(DocumentLayout.labelled(
                "Purpose",
                "This record notes the identity document the company holds for " + employee.name()
                        + " so that the right to work and the payroll record can be verified. "
                        + "The original document is not reproduced here."));

        List<String[]> details = new ArrayList<>();
        details.add(new String[] {"Document type", identity.documentType()});
        details.add(new String[] {"Document number", identity.number()});
        details.add(new String[] {"Issuing authority", identity.issuingAuthority()});
        details.add(new String[] {"Issued on", Dates.format(identity.issuedOn())});
        details.add(new String[] {"Valid until", expiry(identity.expiresOn())});
        details.add(new String[] {"Held for", employee.name()});
        details.add(new String[] {"Employee number", employee.employeeNo()});
        details.add(new String[] {"Department", employee.department()});
        details.add(new String[] {"Date of joining", Dates.format(employee.hireDate())});
        PdfPTable table = detailsTable("Detail", "Value", details);
        table.setSpacingBefore(12);
        document.add(table);

        document.add(DocumentLayout.labelled(
                "Retention",
                "Only the details above are retained. The record is reviewed when the document expires and is "
                        + "destroyed when the retention period in the data policy ends."));
        document.add(signatures(employee.name()));
    }

    // -----------------------------------------------------------------------
    // Small helpers
    // -----------------------------------------------------------------------

    private static void addLine(Phrase phrase, String value) {
        if (notBlank(value)) {
            phrase.add(new Chunk(value + "\n", BODY));
        }
    }

    private static void addLine(List<String[]> rows, String label, String value) {
        if (notBlank(value)) {
            rows.add(new String[] {label, value});
        }
    }

    private static String expiry(LocalDate date) {
        return date == null ? "Does not expire" : Dates.format(date);
    }

    private static String currency(String value) {
        return notBlank(value) ? value.trim() : DEFAULT_CURRENCY;
    }

    /** {@code full_time -> "Full time"}. */
    private static String humanise(String value) {
        if (!notBlank(value)) {
            return "";
        }
        String spaced = value.trim().replace('_', ' ');
        return Character.toUpperCase(spaced.charAt(0)) + spaced.substring(1);
    }

    private static boolean notBlank(String value) {
        return value != null && !value.isBlank();
    }
}
