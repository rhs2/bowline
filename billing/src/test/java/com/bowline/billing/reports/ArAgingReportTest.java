package com.bowline.billing.reports;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.within;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.content;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.header;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import com.bowline.billing.support.Fixtures;
import com.bowline.billing.support.IntegrationTestBase;
import com.bowline.billing.support.TestFakes;
import java.io.ByteArrayInputStream;
import java.time.LocalDate;
import java.time.ZoneOffset;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.apache.poi.ss.usermodel.Cell;
import org.apache.poi.ss.usermodel.CellType;
import org.apache.poi.ss.usermodel.Row;
import org.apache.poi.ss.usermodel.Sheet;
import org.apache.poi.xssf.usermodel.XSSFWorkbook;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ArAgingReportTest extends IntegrationTestBase {

    private static final String XLSX = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

    @BeforeEach
    void rows() {
        arAging.reset(Fixtures.agingRows());
    }

    private byte[] download(String asOf) throws Exception {
        var request = get("/reports/ar-aging.xlsx").header(TOKEN_HEADER, TOKEN);
        if (asOf != null) {
            request = request.param("as_of", asOf);
        }
        return mvc.perform(request)
                .andExpect(status().isOk())
                .andExpect(content().contentType(XLSX))
                .andReturn().getResponse().getContentAsByteArray();
    }

    @Test
    void workbookHasOneRowPerInvoiceAndTotalsByBucket() throws Exception {
        byte[] bytes = mvc.perform(get("/reports/ar-aging.xlsx").param("as_of", "2026-08-27").header(TOKEN_HEADER, TOKEN))
                .andExpect(status().isOk())
                .andExpect(content().contentType(XLSX))
                .andExpect(header().string("Content-Disposition", "attachment; filename=\"ar-aging-2026-08-27.xlsx\""))
                .andReturn().getResponse().getContentAsByteArray();
        assertThat(arAging.lastAsOf()).isEqualTo(LocalDate.of(2026, 8, 27));

        try (XSSFWorkbook workbook = new XSSFWorkbook(new ByteArrayInputStream(bytes))) {
            Sheet sheet = workbook.getSheet(ArAgingWorkbook.SHEET_NAME);
            assertThat(sheet).isNotNull();

            Row header = sheet.getRow(0);
            for (int c = 0; c < ArAgingWorkbook.HEADERS.length; c++) {
                assertThat(header.getCell(c).getStringCellValue()).isEqualTo(ArAgingWorkbook.HEADERS[c]);
            }

            int dataRows = Fixtures.agingRows().size();
            for (int r = 1; r <= dataRows; r++) {
                ArAgingRow expected = Fixtures.agingRows().get(r - 1);
                Row row = sheet.getRow(r);
                assertThat(row.getCell(ArAgingWorkbook.COL_INVOICE).getStringCellValue()).isEqualTo(expected.invoiceNo());
                assertThat(row.getCell(ArAgingWorkbook.COL_CUSTOMER).getStringCellValue()).isEqualTo(expected.customer());
                Cell due = row.getCell(ArAgingWorkbook.COL_DUE);
                assertThat(due.getCellType()).isEqualTo(CellType.NUMERIC);
                assertThat(due.getLocalDateTimeCellValue().toLocalDate()).isEqualTo(expected.dueDate());
                assertThat(due.getCellStyle().getDataFormatString()).isEqualTo(ArAgingWorkbook.DATE_FORMAT);
                assertThat((int) row.getCell(ArAgingWorkbook.COL_DAYS).getNumericCellValue()).isEqualTo(expected.daysOverdue());
                assertThat(row.getCell(ArAgingWorkbook.COL_BUCKET).getStringCellValue()).isEqualTo(expected.bucket());
                Cell outstanding = row.getCell(ArAgingWorkbook.COL_OUTSTANDING);
                assertThat(outstanding.getNumericCellValue()).isCloseTo(expected.outstanding().doubleValue(), within(0.0001));
                assertThat(outstanding.getCellStyle().getDataFormatString()).isEqualTo(ArAgingWorkbook.MONEY_FORMAT);
            }
            assertThat(sheet.getRow(dataRows + 1)).as("blank spacer row").isNull();

            Row title = sheet.getRow(dataRows + 2);
            assertThat(title.getCell(ArAgingWorkbook.COL_DAYS).getStringCellValue()).isEqualTo(ArAgingWorkbook.TOTALS_TITLE);

            Map<String, Double> totals = new HashMap<>();
            Map<String, Integer> counts = new HashMap<>();
            for (int i = 0; i < ArAgingRow.BUCKETS.size(); i++) {
                Row row = sheet.getRow(dataRows + 3 + i);
                String bucket = row.getCell(ArAgingWorkbook.COL_BUCKET).getStringCellValue();
                assertThat(bucket).isEqualTo(ArAgingRow.BUCKETS.get(i));
                totals.put(bucket, row.getCell(ArAgingWorkbook.COL_OUTSTANDING).getNumericCellValue());
                counts.put(bucket, (int) row.getCell(ArAgingWorkbook.COL_DAYS).getNumericCellValue());
                assertThat(row.getCell(ArAgingWorkbook.COL_OUTSTANDING).getCellStyle().getDataFormatString())
                        .isEqualTo(ArAgingWorkbook.MONEY_FORMAT);
            }
            assertThat(totals.get("current")).isCloseTo(1500.00, within(0.0001));
            assertThat(totals.get("1-30")).isCloseTo(250.50, within(0.0001));
            assertThat(totals.get("31-60")).isCloseTo(900.00, within(0.0001));
            assertThat(totals.get("61-90")).isCloseTo(75.25, within(0.0001));
            assertThat(totals.get("90+")).isCloseTo(4000.00, within(0.0001));
            assertThat(counts).containsEntry("current", 2).containsEntry("90+", 1);

            Row grand = sheet.getRow(dataRows + 3 + ArAgingRow.BUCKETS.size());
            assertThat(grand.getCell(ArAgingWorkbook.COL_BUCKET).getStringCellValue()).isEqualTo(ArAgingWorkbook.GRAND_TOTAL_LABEL);
            assertThat(grand.getCell(ArAgingWorkbook.COL_OUTSTANDING).getNumericCellValue()).isCloseTo(6725.75, within(0.0001));
            assertThat((int) grand.getCell(ArAgingWorkbook.COL_DAYS).getNumericCellValue()).isEqualTo(dataRows);

            Row asOf = sheet.getRow(dataRows + 3 + ArAgingRow.BUCKETS.size() + 2);
            assertThat(asOf.getCell(ArAgingWorkbook.COL_BUCKET).getStringCellValue()).isEqualTo(ArAgingWorkbook.AS_OF_LABEL);
            assertThat(asOf.getCell(ArAgingWorkbook.COL_OUTSTANDING).getLocalDateTimeCellValue().toLocalDate())
                    .isEqualTo(LocalDate.of(2026, 8, 27));

            assertThat(sheet.getPaneInformation().isFreezePane()).isTrue();
            assertThat(workbook.getProperties().getCoreProperties().getTitle()).isEqualTo("AR aging as of 2026-08-27");
        }
    }

    @Test
    void reportDateDefaultsToToday() throws Exception {
        download(null);
        assertThat(arAging.lastAsOf()).isEqualTo(TestFakes.NOW.atZone(ZoneOffset.UTC).toLocalDate());
    }

    @Test
    void emptyLedgerStillProducesAWorkbookWithZeroTotals() throws Exception {
        arAging.reset(List.of());
        byte[] bytes = download("2026-08-27");
        try (XSSFWorkbook workbook = new XSSFWorkbook(new ByteArrayInputStream(bytes))) {
            Sheet sheet = workbook.getSheetAt(0);
            assertThat(sheet.getRow(1)).isNull();
            Row grand = sheet.getRow(2 + 1 + ArAgingRow.BUCKETS.size());
            assertThat(grand.getCell(ArAgingWorkbook.COL_BUCKET).getStringCellValue()).isEqualTo(ArAgingWorkbook.GRAND_TOTAL_LABEL);
            assertThat(grand.getCell(ArAgingWorkbook.COL_OUTSTANDING).getNumericCellValue()).isZero();
        }
    }

    @Test
    void bucketsFollowTheViewDefinition() {
        LocalDate asOf = LocalDate.of(2026, 8, 27);
        assertThat(ArAgingRow.bucketFor(asOf, asOf)).isEqualTo("current");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.plusDays(40))).isEqualTo("current");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(1))).isEqualTo("1-30");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(30))).isEqualTo("1-30");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(31))).isEqualTo("31-60");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(60))).isEqualTo("31-60");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(61))).isEqualTo("61-90");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(90))).isEqualTo("61-90");
        assertThat(ArAgingRow.bucketFor(asOf, asOf.minusDays(91))).isEqualTo("90+");
        assertThat(ArAgingRow.daysOverdue(asOf, asOf.plusDays(3))).isZero();
    }
}
