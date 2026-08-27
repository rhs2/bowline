package com.bowline.billing.reports;

import java.time.LocalDate;
import java.util.List;

/** Source of AR aging rows; the JDBC implementation reads the {@code ar_aging} view. */
public interface ArAgingRepository {

    /** Outstanding invoices aged as of {@code asOf}, ordered by due date then invoice number. */
    List<ArAgingRow> findOutstanding(LocalDate asOf);
}
