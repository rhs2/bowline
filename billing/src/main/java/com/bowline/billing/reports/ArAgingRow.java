package com.bowline.billing.reports;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.time.temporal.ChronoUnit;
import java.util.List;

/** One outstanding invoice as aged on the report date. Buckets follow the {@code ar_aging} view. */
public record ArAgingRow(
        String invoiceNo,
        String customer,
        LocalDate dueDate,
        int daysOverdue,
        String bucket,
        BigDecimal outstanding) {

    public static final String CURRENT = "current";
    public static final String DAYS_1_30 = "1-30";
    public static final String DAYS_31_60 = "31-60";
    public static final String DAYS_61_90 = "61-90";
    public static final String DAYS_90_PLUS = "90+";

    /** Bucket order used on the spreadsheet. */
    public static final List<String> BUCKETS = List.of(CURRENT, DAYS_1_30, DAYS_31_60, DAYS_61_90, DAYS_90_PLUS);

    /** Same rule as the SQL view, applied to an arbitrary report date. */
    public static String bucketFor(LocalDate asOf, LocalDate dueDate) {
        int overdue = daysOverdue(asOf, dueDate);
        if (overdue <= 0) {
            return CURRENT;
        }
        if (overdue <= 30) {
            return DAYS_1_30;
        }
        if (overdue <= 60) {
            return DAYS_31_60;
        }
        if (overdue <= 90) {
            return DAYS_61_90;
        }
        return DAYS_90_PLUS;
    }

    public static int daysOverdue(LocalDate asOf, LocalDate dueDate) {
        return (int) Math.max(0, ChronoUnit.DAYS.between(dueDate, asOf));
    }

    /** Convenience for callers that only know the dates. */
    public static ArAgingRow aged(String invoiceNo, String customer, LocalDate dueDate, LocalDate asOf, BigDecimal outstanding) {
        return new ArAgingRow(invoiceNo, customer, dueDate, daysOverdue(asOf, dueDate), bucketFor(asOf, dueDate), outstanding);
    }
}
