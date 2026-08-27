package com.bowline.billing.statements;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;
import java.util.Optional;
import java.util.UUID;

/** Data behind a customer statement; the JDBC implementation reads invoices and payments. */
public interface StatementRepository {

    Optional<StatementCustomer> findCustomer(UUID customerId);

    /** Invoices issued minus payments received strictly before {@code from}. */
    BigDecimal openingBalance(UUID customerId, LocalDate from);

    /** Invoices issued and payments received within the window, in chronological order. */
    List<StatementEntry> findEntries(UUID customerId, LocalDate from, LocalDate to);
}
