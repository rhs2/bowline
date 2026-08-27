package com.bowline.billing.reports;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.apache.poi.ss.usermodel.BorderStyle;
import org.apache.poi.ss.usermodel.Cell;
import org.apache.poi.ss.usermodel.CellStyle;
import org.apache.poi.ss.usermodel.FillPatternType;
import org.apache.poi.ss.usermodel.Font;
import org.apache.poi.ss.usermodel.HorizontalAlignment;
import org.apache.poi.ss.usermodel.IndexedColors;
import org.apache.poi.ss.usermodel.Row;
import org.apache.poi.ss.usermodel.Sheet;
import org.apache.poi.ss.util.CellRangeAddress;
import org.apache.poi.xssf.usermodel.XSSFWorkbook;
import org.springframework.stereotype.Component;

/**
 * Builds the AR aging spreadsheet with Apache POI. Sheet layout (all zero based):
 *
 * <pre>
 * row 0                    header: Invoice | Customer | Due date | Days overdue | Bucket | Outstanding
 * rows 1..n                one row per outstanding invoice
 * row n+2                  "Totals by bucket" title
 * rows n+3..n+7            Invoices (count) | Bucket | Outstanding, one per bucket in aging order
 * row n+8                  Total outstanding
 * row n+10                 "As of" and the report date
 * </pre>
 *
 * Totals are written as values (not formulas) so any reader sees them without a
 * recalculation pass. Column widths are fixed because auto-sizing needs AWT fonts,
 * which a headless container may not have.
 */
@Component
public class ArAgingWorkbook {

    public static final String SHEET_NAME = "AR aging";
    public static final String[] HEADERS = {"Invoice", "Customer", "Due date", "Days overdue", "Bucket", "Outstanding"};
    public static final String TOTALS_TITLE = "Totals by bucket";
    public static final String GRAND_TOTAL_LABEL = "Total outstanding";
    public static final String AS_OF_LABEL = "As of";
    public static final String MONEY_FORMAT = "#,##0.00";
    public static final String DATE_FORMAT = "yyyy-mm-dd";

    static final int COL_INVOICE = 0;
    static final int COL_CUSTOMER = 1;
    static final int COL_DUE = 2;
    static final int COL_DAYS = 3;
    static final int COL_BUCKET = 4;
    static final int COL_OUTSTANDING = 5;

    public byte[] build(LocalDate asOf, List<ArAgingRow> rows) {
        try (XSSFWorkbook workbook = new XSSFWorkbook()) {
            Styles styles = new Styles(workbook);
            Sheet sheet = workbook.createSheet(SHEET_NAME);
            writeHeader(sheet, styles);
            int next = writeRows(sheet, styles, rows);
            next = writeTotals(sheet, styles, rows, next + 1);
            writeAsOf(sheet, styles, asOf, next + 1);
            layout(sheet, rows.size());

            workbook.getProperties().getCoreProperties().setTitle("AR aging as of " + asOf);
            ByteArrayOutputStream out = new ByteArrayOutputStream(32 * 1024);
            workbook.write(out);
            return out.toByteArray();
        } catch (IOException e) {
            throw new IllegalStateException("could not write workbook", e);
        }
    }

    private static void writeHeader(Sheet sheet, Styles styles) {
        Row header = sheet.createRow(0);
        for (int c = 0; c < HEADERS.length; c++) {
            Cell cell = header.createCell(c);
            cell.setCellValue(HEADERS[c]);
            cell.setCellStyle(styles.header);
        }
    }

    /** Returns the index of the first row after the data block. */
    private static int writeRows(Sheet sheet, Styles styles, List<ArAgingRow> rows) {
        int r = 1;
        for (ArAgingRow row : rows) {
            Row out = sheet.createRow(r++);
            out.createCell(COL_INVOICE).setCellValue(row.invoiceNo());
            out.createCell(COL_CUSTOMER).setCellValue(row.customer());
            Cell due = out.createCell(COL_DUE);
            due.setCellValue(row.dueDate());
            due.setCellStyle(styles.date);
            Cell days = out.createCell(COL_DAYS);
            days.setCellValue(row.daysOverdue());
            days.setCellStyle(styles.integer);
            out.createCell(COL_BUCKET).setCellValue(row.bucket());
            Cell outstanding = out.createCell(COL_OUTSTANDING);
            outstanding.setCellValue(row.outstanding().doubleValue());
            outstanding.setCellStyle(styles.money);
        }
        return r;
    }

    /** Returns the index of the first row after the totals block. */
    private static int writeTotals(Sheet sheet, Styles styles, List<ArAgingRow> rows, int start) {
        Map<String, BigDecimal> sums = new LinkedHashMap<>();
        Map<String, Integer> counts = new LinkedHashMap<>();
        for (String bucket : ArAgingRow.BUCKETS) {
            sums.put(bucket, BigDecimal.ZERO);
            counts.put(bucket, 0);
        }
        BigDecimal grand = BigDecimal.ZERO;
        for (ArAgingRow row : rows) {
            sums.merge(row.bucket(), row.outstanding(), BigDecimal::add);
            counts.merge(row.bucket(), 1, Integer::sum);
            grand = grand.add(row.outstanding());
        }

        int r = start;
        Row title = sheet.createRow(r++);
        Cell titleCell = title.createCell(COL_DAYS);
        titleCell.setCellValue(TOTALS_TITLE);
        titleCell.setCellStyle(styles.bold);

        for (Map.Entry<String, BigDecimal> entry : sums.entrySet()) {
            Row out = sheet.createRow(r++);
            Cell count = out.createCell(COL_DAYS);
            count.setCellValue(counts.get(entry.getKey()));
            count.setCellStyle(styles.integer);
            out.createCell(COL_BUCKET).setCellValue(entry.getKey());
            Cell amount = out.createCell(COL_OUTSTANDING);
            amount.setCellValue(entry.getValue().doubleValue());
            amount.setCellStyle(styles.money);
        }

        Row total = sheet.createRow(r++);
        Cell count = total.createCell(COL_DAYS);
        count.setCellValue(rows.size());
        count.setCellStyle(styles.integerBold);
        Cell label = total.createCell(COL_BUCKET);
        label.setCellValue(GRAND_TOTAL_LABEL);
        label.setCellStyle(styles.bold);
        Cell amount = total.createCell(COL_OUTSTANDING);
        amount.setCellValue(grand.doubleValue());
        amount.setCellStyle(styles.moneyBold);
        return r;
    }

    private static void writeAsOf(Sheet sheet, Styles styles, LocalDate asOf, int r) {
        Row row = sheet.createRow(r);
        Cell label = row.createCell(COL_BUCKET);
        label.setCellValue(AS_OF_LABEL);
        label.setCellStyle(styles.bold);
        Cell date = row.createCell(COL_OUTSTANDING);
        date.setCellValue(asOf);
        date.setCellStyle(styles.date);
    }

    private static void layout(Sheet sheet, int dataRows) {
        sheet.setColumnWidth(COL_INVOICE, 18 * 256);
        sheet.setColumnWidth(COL_CUSTOMER, 36 * 256);
        sheet.setColumnWidth(COL_DUE, 13 * 256);
        sheet.setColumnWidth(COL_DAYS, 14 * 256);
        sheet.setColumnWidth(COL_BUCKET, 18 * 256);
        sheet.setColumnWidth(COL_OUTSTANDING, 16 * 256);
        sheet.createFreezePane(0, 1);
        if (dataRows > 0) {
            sheet.setAutoFilter(new CellRangeAddress(0, dataRows, COL_INVOICE, COL_OUTSTANDING));
        }
    }

    /** Cell styles are workbook scoped, so they are created once per build. */
    private static final class Styles {
        final CellStyle header;
        final CellStyle bold;
        final CellStyle date;
        final CellStyle integer;
        final CellStyle integerBold;
        final CellStyle money;
        final CellStyle moneyBold;

        Styles(XSSFWorkbook wb) {
            Font boldFont = wb.createFont();
            boldFont.setBold(true);

            header = wb.createCellStyle();
            header.setFont(boldFont);
            header.setFillForegroundColor(IndexedColors.GREY_25_PERCENT.getIndex());
            header.setFillPattern(FillPatternType.SOLID_FOREGROUND);
            header.setBorderBottom(BorderStyle.THIN);

            bold = wb.createCellStyle();
            bold.setFont(boldFont);

            short dateFmt = wb.createDataFormat().getFormat(DATE_FORMAT);
            date = wb.createCellStyle();
            date.setDataFormat(dateFmt);
            date.setAlignment(HorizontalAlignment.RIGHT);

            short intFmt = wb.createDataFormat().getFormat("0");
            integer = wb.createCellStyle();
            integer.setDataFormat(intFmt);
            integerBold = wb.createCellStyle();
            integerBold.setDataFormat(intFmt);
            integerBold.setFont(boldFont);

            short moneyFmt = wb.createDataFormat().getFormat(MONEY_FORMAT);
            money = wb.createCellStyle();
            money.setDataFormat(moneyFmt);
            moneyBold = wb.createCellStyle();
            moneyBold.setDataFormat(moneyFmt);
            moneyBold.setFont(boldFont);
            moneyBold.setBorderTop(BorderStyle.THIN);
        }
    }
}
