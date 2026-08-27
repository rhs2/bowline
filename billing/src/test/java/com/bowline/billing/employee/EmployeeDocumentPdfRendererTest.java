package com.bowline.billing.employee;

import static org.assertj.core.api.Assertions.assertThat;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.Pdfs;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

/** Pure unit tests of the four personnel layouts: no Spring context. */
class EmployeeDocumentPdfRendererTest {

    private final EmployeeDocumentPdfRenderer renderer = new EmployeeDocumentPdfRenderer(Fixtures.properties());

    @Test
    void contractCarriesTheTermsAndTheClauses() {
        byte[] pdf = renderer.render(EmployeeDocumentKind.CONTRACT, Fixtures.contract());

        assertThat(Pdfs.isPdf(pdf)).as("starts with %PDF").isTrue();
        // The clauses and the signature block run onto a second page; both are numbered.
        assertThat(Pdfs.pages(pdf)).isEqualTo(2);
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("Bowline Logistics")
                .contains("1 Harbour Way, Port City")
                .contains("EMPLOYMENT CONTRACT")
                .contains("Priya Raman")
                .contains("Employee number BWL-000482")
                .contains("Warehouse Supervisor")
                .contains("Warehouse Operations")
                .contains("Marcus Elliot")
                .contains("14 Mar 2022")
                .contains("Full time")
                .contains("USD 68,400.00")
                .contains("30 days")
                .contains("1. Appointment")
                .contains("6. Notice and termination")
                .contains("For and on behalf of the company");
        assertThat(Pattern.compile("Page 1 of\\s*2").matcher(text).find()).as("numbered footer").isTrue();
        assertThat(Pattern.compile("Page 2 of\\s*2").matcher(text).find()).isTrue();
    }

    @Test
    void payslipAddsUpAndNamesThePeriod() {
        byte[] pdf = renderer.render(EmployeeDocumentKind.PAYSLIP, Fixtures.payslip());

        assertThat(Pdfs.isPdf(pdf)).isTrue();
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("PAYSLIP")
                .contains("2026-07")
                .contains("Priya Raman")
                .contains("GROSS PAY")
                .contains("USD 5,700.00")
                .contains("USD 1,596.00")
                .contains("USD -1,596.00")
                .contains("NET PAY")
                .contains("USD 4,104.00")
                .contains("1 Jul 2026")
                .contains("31 Jul 2026")
                .contains("Bank transfer");
    }

    @Test
    void certificateAttestsTheQualification() {
        byte[] pdf = renderer.render(EmployeeDocumentKind.CERTIFICATE, Fixtures.certificate());

        assertThat(Pdfs.isPdf(pdf)).isTrue();
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("CERTIFICATE")
                .contains("This is to certify that")
                .contains("Priya Raman")
                .contains("Forklift and dangerous goods handling")
                .contains("Port City Safety Institute")
                .contains("12 May 2025")
                .contains("11 May 2028")
                .contains("PCSI-DG-88214");
    }

    @Test
    void identityRecordListsTheDocumentOnFile() {
        byte[] pdf = renderer.render(EmployeeDocumentKind.ID, Fixtures.identityDocument());

        assertThat(Pdfs.isPdf(pdf)).isTrue();
        String text = Pdfs.flatText(pdf);
        assertThat(text)
                .contains("IDENTITY RECORD")
                .contains("Passport")
                .contains("X4419078")
                .contains("Freelandia Passport Office")
                .contains("2 Sep 2021")
                .contains("1 Sep 2031")
                .contains("BWL-000482");
    }

    @Test
    void everyKindSaysItIsDemonstrationData() {
        for (EmployeeDocumentRequest request :
                new EmployeeDocumentRequest[] {
                    Fixtures.contract(), Fixtures.payslip(), Fixtures.certificate(), Fixtures.identityDocument()
                }) {
            EmployeeDocumentKind kind = EmployeeDocumentKind.of(request.kind()).orElseThrow();
            String text = Pdfs.flatText(renderer.render(kind, request));
            assertThat(text)
                    .as("%s notes that it is demo data", kind.wireName())
                    .contains(EmployeeDocumentPdfRenderer.DEMO_NOTICE);
        }
    }

    @Test
    void optionalFieldsAreLeftOutRatherThanPrintedEmpty() {
        EmployeeDocumentRequest full = Fixtures.certificate();
        EmployeeDocumentRequest sparse = new EmployeeDocumentRequest(
                full.kind(),
                full.s3Key(),
                full.title(),
                new EmployeeDocumentRequest.Employee(
                        "BWL-000999", "Sam Okoro", null, null, null, null, null, null, null, null),
                null, null,
                new EmployeeDocumentRequest.Certificate(
                        "Dangerous goods awareness", null, full.certificate().issuedOn(), null, null),
                null);

        byte[] pdf = renderer.render(EmployeeDocumentKind.CERTIFICATE, sparse);
        String text = Pdfs.flatText(pdf);
        assertThat(Pdfs.isPdf(pdf)).isTrue();
        assertThat(text)
                .contains("Sam Okoro")
                .contains("Dangerous goods awareness")
                .contains("Does not expire")
                .doesNotContain("Issued by")
                .doesNotContain("Reference");
    }
}
