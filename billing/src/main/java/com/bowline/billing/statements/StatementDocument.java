package com.bowline.billing.statements;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/** Everything the statement renderer needs, with running balances already computed. */
public record StatementDocument(
        StatementCustomer customer,
        LocalDate from,
        LocalDate to,
        BigDecimal openingBalance,
        List<Line> lines,
        BigDecimal totalCharges,
        BigDecimal totalPayments,
        BigDecimal closingBalance) {

    /** One printed row: the movement plus the balance after it. */
    public record Line(StatementEntry entry, BigDecimal balance) {}

    public static StatementDocument build(
            StatementCustomer customer, LocalDate from, LocalDate to, BigDecimal opening, List<StatementEntry> entries) {
        BigDecimal balance = opening;
        BigDecimal charges = BigDecimal.ZERO;
        BigDecimal payments = BigDecimal.ZERO;
        List<Line> lines = new ArrayList<>(entries.size());
        for (StatementEntry entry : entries) {
            balance = balance.add(entry.debit()).subtract(entry.credit());
            charges = charges.add(entry.debit());
            payments = payments.add(entry.credit());
            lines.add(new Line(entry, balance));
        }
        return new StatementDocument(customer, from, to, opening, List.copyOf(lines), charges, payments, balance);
    }
}
