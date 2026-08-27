package com.bowline.billing.invoice;

import com.bowline.billing.web.InvalidRequestException;
import com.bowline.billing.web.ProblemResponse;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Cross-field rules that bean validation cannot express. Rendering a document whose
 * numbers do not add up would put a wrong invoice in front of a customer, so these are
 * hard failures (422), except the line-sum check which only warns because the header
 * totals are the ledger's truth.
 */
final class InvoiceRules {

    private static final Logger log = LoggerFactory.getLogger(InvoiceRules.class);

    private InvoiceRules() {}

    static void check(InvoiceRenderRequest request) {
        InvoiceRenderRequest.Invoice inv = request.invoice();
        List<ProblemResponse.FieldError> errors = new ArrayList<>();

        if (inv.dueDate().isBefore(inv.issueDate())) {
            errors.add(new ProblemResponse.FieldError("invoice.due_date", "must be on or after issue_date"));
        }
        if (inv.subtotal().add(inv.tax()).compareTo(inv.total()) != 0) {
            errors.add(new ProblemResponse.FieldError("invoice.total", "must equal subtotal + tax"));
        }
        if (inv.amountPaid().compareTo(inv.total()) > 0) {
            errors.add(new ProblemResponse.FieldError("invoice.amount_paid", "must not exceed total"));
        }

        Set<Integer> seen = new HashSet<>();
        BigDecimal lineSum = BigDecimal.ZERO;
        for (int i = 0; i < request.lines().size(); i++) {
            InvoiceRenderRequest.Line line = request.lines().get(i);
            if (!seen.add(line.seq())) {
                errors.add(new ProblemResponse.FieldError("lines[" + i + "].seq", "is duplicated"));
            }
            lineSum = lineSum.add(line.amount());
        }

        if (!errors.isEmpty()) {
            throw new InvalidRequestException("Invoice figures are inconsistent.", errors);
        }
        if (lineSum.compareTo(inv.subtotal()) != 0) {
            log.warn("invoice {}: line amounts sum to {} but subtotal is {}", inv.invoiceNo(), lineSum, inv.subtotal());
        }
    }
}
