package com.bowline.billing.statements;

import java.math.BigDecimal;
import java.time.LocalDate;

/**
 * One movement on a customer account: an issued invoice (a charge) or a payment
 * (a credit). {@code debit} and {@code credit} are never both non-zero.
 */
public record StatementEntry(
        LocalDate date,
        Kind kind,
        String reference,
        String description,
        BigDecimal debit,
        BigDecimal credit) {

    public enum Kind {
        INVOICE,
        PAYMENT
    }

    public static StatementEntry invoice(LocalDate issueDate, String invoiceNo, LocalDate dueDate, BigDecimal total) {
        return new StatementEntry(issueDate, Kind.INVOICE, invoiceNo, "Invoice, due " + dueDate, total, BigDecimal.ZERO);
    }

    public static StatementEntry payment(
            LocalDate receivedOn, String reference, String invoiceNo, String method, BigDecimal amount) {
        String ref = reference == null || reference.isBlank() ? invoiceNo : reference;
        String how = method == null ? "payment" : method.replace('_', ' ');
        return new StatementEntry(receivedOn, Kind.PAYMENT, ref, "Payment by " + how + " against " + invoiceNo,
                BigDecimal.ZERO, amount);
    }
}
