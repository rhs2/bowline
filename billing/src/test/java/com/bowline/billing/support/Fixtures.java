package com.bowline.billing.support;

import com.bowline.billing.config.BillingProperties;
import com.bowline.billing.document.PostalAddress;
import com.bowline.billing.employee.EmployeeDocumentRequest;
import com.bowline.billing.invoice.InvoiceRenderRequest;
import com.bowline.billing.reports.ArAgingRow;
import com.bowline.billing.statements.StatementCustomer;
import com.bowline.billing.statements.StatementEntry;
import java.math.BigDecimal;
import java.time.Duration;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/** Sample documents shared by the suites. Figures are chosen so totals are easy to spot. */
public final class Fixtures {

    public static final String INVOICE_NO = "INV-2026-000123";
    public static final UUID CUSTOMER_ID = UUID.fromString("7d5a2f0e-3b1c-4c8e-9a6d-1f2e3d4c5b6a");
    public static final LocalDate AS_OF = LocalDate.of(2026, 8, 27);

    private Fixtures() {}

    public static BillingProperties properties() {
        return new BillingProperties(
                "test-internal-token",
                BillingProperties.PdfOutput.LOCAL,
                TestOutput.DIR.toString(),
                Duration.ofSeconds(1),
                new BillingProperties.Company("Bowline Logistics", "1 Harbour Way, Port City"),
                new BillingProperties.S3("", "us-east-1", "bowline-pdfs", "bowline-documents", "", "", false));
    }

    public static PostalAddress address() {
        return new PostalAddress("Unit 12, 400 Wharf Road", null, "Port City", "PC", "40010", "Freelandia");
    }

    /** Subtotal 4,280.00, tax 58.00, total 4,338.00, paid 1,000.00, balance 3,338.00. */
    public static InvoiceRenderRequest invoice() {
        return invoice(INVOICE_NO);
    }

    public static InvoiceRenderRequest invoice(String invoiceNo) {
        InvoiceRenderRequest.Invoice header = new InvoiceRenderRequest.Invoice(
                invoiceNo,
                LocalDate.of(2026, 8, 1),
                LocalDate.of(2026, 8, 31),
                "USD",
                new BigDecimal("4280.00"),
                new BigDecimal("58.00"),
                new BigDecimal("4338.00"),
                new BigDecimal("1000.00"),
                "Thank you for shipping with Bowline. Bank: Harbour Bank, account 00-1234-5678.");
        InvoiceRenderRequest.Customer customer = new InvoiceRenderRequest.Customer(
                "Acme Trading Co.",
                "ACME",
                new InvoiceRenderRequest.Contact("Dana Whitfield", "ap@acme.example", "+1 555 0100"),
                address());
        InvoiceRenderRequest.Shipment shipment = new InvoiceRenderRequest.Shipment(
                "BWL-2026-000456", "sea", "Shanghai", "Port City");
        List<InvoiceRenderRequest.Line> lines = List.of(
                line(1, "Sea freight, Shanghai to Port City, 2 x 40ft HC", "2", "1850.00", "0", "3700.00"),
                line(2, "Customs brokerage and documentation", "1", "350.00", "0.1", "350.00"),
                line(3, "Warehouse handling, per pallet", "12.5", "18.40", "0.1", "230.00"));
        return new InvoiceRenderRequest(header, customer, shipment, lines);
    }

    /** An invoice with enough lines to spill onto several pages. */
    public static InvoiceRenderRequest longInvoice(int lineCount) {
        InvoiceRenderRequest base = invoice("INV-2026-000999");
        List<InvoiceRenderRequest.Line> lines = new ArrayList<>(lineCount);
        BigDecimal subtotal = BigDecimal.ZERO;
        for (int i = 1; i <= lineCount; i++) {
            BigDecimal amount = new BigDecimal("10.00");
            lines.add(line(i, "Handling charge, item " + i, "1", "10.00", "0", "10.00"));
            subtotal = subtotal.add(amount);
        }
        InvoiceRenderRequest.Invoice header = new InvoiceRenderRequest.Invoice(
                base.invoice().invoiceNo(), base.invoice().issueDate(), base.invoice().dueDate(), "USD",
                subtotal, BigDecimal.ZERO, subtotal, BigDecimal.ZERO, null);
        return new InvoiceRenderRequest(header, base.customer(), null, lines);
    }

    public static InvoiceRenderRequest.Line line(
            int seq, String description, String qty, String unit, String rate, String amount) {
        return new InvoiceRenderRequest.Line(seq, description, new BigDecimal(qty), new BigDecimal(unit),
                new BigDecimal(rate), new BigDecimal(amount));
    }

    /**
     * Six invoices as of 2026-08-27: current 1,500.00 (two rows), 1-30 250.50,
     * 31-60 900.00, 61-90 75.25, 90+ 4,000.00; total 6,725.75.
     */
    public static List<ArAgingRow> agingRows() {
        return List.of(
                ArAgingRow.aged("INV-2026-000201", "Acme Trading Co.", LocalDate.of(2026, 9, 10), AS_OF, new BigDecimal("1000.00")),
                ArAgingRow.aged("INV-2026-000202", "Blue Harbour Foods", LocalDate.of(2026, 8, 27), AS_OF, new BigDecimal("500.00")),
                ArAgingRow.aged("INV-2026-000190", "Acme Trading Co.", LocalDate.of(2026, 8, 10), AS_OF, new BigDecimal("250.50")),
                ArAgingRow.aged("INV-2026-000170", "Corvid Electronics", LocalDate.of(2026, 7, 5), AS_OF, new BigDecimal("900.00")),
                ArAgingRow.aged("INV-2026-000150", "Blue Harbour Foods", LocalDate.of(2026, 6, 1), AS_OF, new BigDecimal("75.25")),
                ArAgingRow.aged("INV-2026-000090", "Delta Mining", LocalDate.of(2026, 3, 1), AS_OF, new BigDecimal("4000.00")));
    }

    // -----------------------------------------------------------------------
    // Personnel files
    // -----------------------------------------------------------------------

    public static final UUID EMPLOYEE_ID = UUID.fromString("1c9e6f2a-8d43-4f7b-b0c5-2a9d8e7f6b5c");
    public static final String EMPLOYEE_NO = "BWL-000482";

    public static EmployeeDocumentRequest.Employee employee() {
        return new EmployeeDocumentRequest.Employee(
                EMPLOYEE_NO,
                "Priya Raman",
                "priya.raman@bowline.example",
                "Warehouse Supervisor",
                "Warehouse Operations",
                "Port City Terminal",
                "G5",
                "Marcus Elliot",
                LocalDate.of(2022, 3, 14),
                "full_time");
    }

    /** Key under the employee prefix, the shape {@code employee_documents.s3_key} holds. */
    public static String documentKey(String file) {
        return "employees/" + EMPLOYEE_ID + "/" + file;
    }

    /** Annual salary 68,400.00, starting 14 Mar 2022. */
    public static EmployeeDocumentRequest contract() {
        return new EmployeeDocumentRequest(
                "contract",
                documentKey("contract.pdf"),
                "Employment contract, " + EMPLOYEE_NO,
                employee(),
                new EmployeeDocumentRequest.Contract(
                        "Warehouse Supervisor",
                        "Warehouse Operations",
                        LocalDate.of(2022, 3, 14),
                        new BigDecimal("68400.00"),
                        "USD",
                        "full_time",
                        "G5",
                        "Port City Terminal",
                        40,
                        30),
                null, null, null);
    }

    /** Gross 5,700.00, deductions 1,596.00, net 4,104.00 for July 2026. */
    public static EmployeeDocumentRequest payslip() {
        return new EmployeeDocumentRequest(
                "payslip",
                documentKey("payslip-2026-07.pdf"),
                "Payslip 2026-07",
                employee(),
                null,
                new EmployeeDocumentRequest.Payslip(
                        "2026-07",
                        LocalDate.of(2026, 7, 1),
                        LocalDate.of(2026, 7, 31),
                        LocalDate.of(2026, 7, 31),
                        new BigDecimal("5700.00"),
                        new BigDecimal("1596.00"),
                        new BigDecimal("4104.00"),
                        "USD",
                        "bank_transfer"),
                null, null);
    }

    public static EmployeeDocumentRequest certificate() {
        return new EmployeeDocumentRequest(
                "certificate",
                documentKey("certificate.pdf"),
                "Forklift and dangerous goods certificate",
                employee(),
                null, null,
                new EmployeeDocumentRequest.Certificate(
                        "Forklift and dangerous goods handling",
                        "Port City Safety Institute",
                        LocalDate.of(2025, 5, 12),
                        LocalDate.of(2028, 5, 11),
                        "PCSI-DG-88214"),
                null);
    }

    public static EmployeeDocumentRequest identityDocument() {
        return new EmployeeDocumentRequest(
                "id",
                documentKey("id.pdf"),
                "Identity document",
                employee(),
                null, null, null,
                new EmployeeDocumentRequest.Identity(
                        "Passport",
                        "X4419078",
                        "Freelandia Passport Office",
                        LocalDate.of(2021, 9, 2),
                        LocalDate.of(2031, 9, 1)));
    }

    public static StatementCustomer statementCustomer() {
        return new StatementCustomer(CUSTOMER_ID, "ACME", "Acme Trading Co.", "Dana Whitfield", "ap@acme.example",
                address(), "USD");
    }

    /** Opening 500.00, then +4,338.00, -1,000.00, +1,200.00: closing 5,038.00. */
    public static List<StatementEntry> statementEntries() {
        return List.of(
                StatementEntry.invoice(LocalDate.of(2026, 7, 5), "INV-2026-000101", LocalDate.of(2026, 8, 4), new BigDecimal("4338.00")),
                StatementEntry.payment(LocalDate.of(2026, 7, 20), "TRX-889", "INV-2026-000101", "bank_transfer", new BigDecimal("1000.00")),
                StatementEntry.invoice(LocalDate.of(2026, 8, 10), "INV-2026-000150", LocalDate.of(2026, 9, 9), new BigDecimal("1200.00")));
    }
}
