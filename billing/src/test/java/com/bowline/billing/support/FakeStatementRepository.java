package com.bowline.billing.support;

import com.bowline.billing.statements.StatementCustomer;
import com.bowline.billing.statements.StatementEntry;
import com.bowline.billing.statements.StatementRepository;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;

/** In-memory statement source: customers, an opening balance and entries per customer. */
public class FakeStatementRepository implements StatementRepository {

    private final Map<UUID, StatementCustomer> customers = new HashMap<>();
    private final Map<UUID, BigDecimal> openings = new HashMap<>();
    private final Map<UUID, List<StatementEntry>> entries = new HashMap<>();

    public void reset() {
        customers.clear();
        openings.clear();
        entries.clear();
    }

    public void addCustomer(StatementCustomer customer, BigDecimal opening, List<StatementEntry> movements) {
        customers.put(customer.id(), customer);
        openings.put(customer.id(), opening);
        entries.put(customer.id(), new ArrayList<>(movements));
    }

    @Override
    public Optional<StatementCustomer> findCustomer(UUID customerId) {
        return Optional.ofNullable(customers.get(customerId));
    }

    @Override
    public BigDecimal openingBalance(UUID customerId, LocalDate from) {
        return openings.getOrDefault(customerId, BigDecimal.ZERO);
    }

    @Override
    public List<StatementEntry> findEntries(UUID customerId, LocalDate from, LocalDate to) {
        return entries.getOrDefault(customerId, List.of()).stream()
                .filter(e -> !e.date().isBefore(from) && !e.date().isAfter(to))
                .toList();
    }
}
