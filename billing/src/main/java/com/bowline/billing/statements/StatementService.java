package com.bowline.billing.statements;

import com.bowline.billing.web.InvalidRequestException;
import com.bowline.billing.web.NotFoundException;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;
import java.util.UUID;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;

/** Assembles and renders a customer statement for a date window. */
@Service
public class StatementService {

    private static final Logger log = LoggerFactory.getLogger(StatementService.class);

    private final StatementRepository repository;
    private final StatementPdfRenderer renderer;

    public StatementService(StatementRepository repository, StatementPdfRenderer renderer) {
        this.repository = repository;
        this.renderer = renderer;
    }

    /** A rendered statement plus the customer it belongs to (for the file name). */
    public record Rendered(StatementCustomer customer, byte[] pdf) {}

    public Rendered render(UUID customerId, LocalDate from, LocalDate to) {
        if (from.isAfter(to)) {
            throw new InvalidRequestException("from", "must be on or before to");
        }
        StatementCustomer customer = repository.findCustomer(customerId)
                .orElseThrow(() -> new NotFoundException("No customer with id " + customerId));
        BigDecimal opening = repository.openingBalance(customerId, from);
        List<StatementEntry> entries = repository.findEntries(customerId, from, to);
        StatementDocument document = StatementDocument.build(customer, from, to, opening, entries);
        byte[] pdf = renderer.render(document);
        log.info("statement for customer {} ({} to {}): {} entries, closing {}, {} bytes",
                customer.code(), from, to, entries.size(), document.closingBalance(), pdf.length);
        return new Rendered(customer, pdf);
    }
}
