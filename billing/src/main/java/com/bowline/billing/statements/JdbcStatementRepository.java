package com.bowline.billing.statements;

import com.bowline.billing.document.PostalAddress;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.jdbc.core.namedparam.NamedParameterJdbcTemplate;
import org.springframework.stereotype.Repository;

/**
 * Reads customers, issued invoices and payments through the read-only role. Only
 * invoices that were actually issued count ({@code issued}, {@code partially_paid},
 * {@code paid}); drafts and voided invoices never appear on a statement.
 */
@Repository
public class JdbcStatementRepository implements StatementRepository {

    private static final Logger log = LoggerFactory.getLogger(JdbcStatementRepository.class);

    private static final String CUSTOMER_SQL = """
            select id, code, name, contact_name, contact_email, billing_address::text as billing_address, currency
              from customers
             where id = :customer_id
            """;

    private static final String OPENING_SQL = """
            select coalesce((select sum(total) from invoices
                              where customer_id = :customer_id
                                and status in ('issued','partially_paid','paid')
                                and issue_date < :from_date), 0)
                 - coalesce((select sum(p.amount) from payments p
                               join invoices i on i.id = p.invoice_id
                              where i.customer_id = :customer_id
                                and i.status in ('issued','partially_paid','paid')
                                and p.received_on < :from_date), 0) as opening
            """;

    private static final String ENTRIES_SQL = """
            select entry_date, kind, reference, invoice_no, due_date, method, amount
              from (
                select i.issue_date as entry_date, 'invoice' as kind, i.invoice_no as reference,
                       i.invoice_no, i.due_date, null::text as method, i.total as amount, 0 as ord
                  from invoices i
                 where i.customer_id = :customer_id
                   and i.status in ('issued','partially_paid','paid')
                   and i.issue_date between :from_date and :to_date
                union all
                select p.received_on, 'payment', p.reference,
                       i.invoice_no, i.due_date, p.method, p.amount, 1
                  from payments p
                  join invoices i on i.id = p.invoice_id
                 where i.customer_id = :customer_id
                   and i.status in ('issued','partially_paid','paid')
                   and p.received_on between :from_date and :to_date
              ) movements
             order by entry_date, ord, invoice_no
            """;

    private final NamedParameterJdbcTemplate jdbc;
    private final ObjectMapper mapper;

    public JdbcStatementRepository(NamedParameterJdbcTemplate jdbc, ObjectMapper mapper) {
        this.jdbc = jdbc;
        this.mapper = mapper;
    }

    @Override
    public Optional<StatementCustomer> findCustomer(UUID customerId) {
        List<StatementCustomer> found = jdbc.query(CUSTOMER_SQL, Map.of("customer_id", customerId), (rs, i) ->
                new StatementCustomer(
                        rs.getObject("id", UUID.class),
                        rs.getString("code"),
                        rs.getString("name"),
                        rs.getString("contact_name"),
                        rs.getString("contact_email"),
                        parseAddress(rs.getString("billing_address")),
                        rs.getString("currency")));
        return found.stream().findFirst();
    }

    @Override
    public BigDecimal openingBalance(UUID customerId, LocalDate from) {
        BigDecimal opening = jdbc.queryForObject(
                OPENING_SQL, Map.of("customer_id", customerId, "from_date", from), BigDecimal.class);
        return opening == null ? BigDecimal.ZERO : opening;
    }

    @Override
    public List<StatementEntry> findEntries(UUID customerId, LocalDate from, LocalDate to) {
        Map<String, Object> params = Map.of("customer_id", customerId, "from_date", from, "to_date", to);
        return jdbc.query(ENTRIES_SQL, params, (rs, i) -> {
            LocalDate date = rs.getObject("entry_date", LocalDate.class);
            String invoiceNo = rs.getString("invoice_no");
            BigDecimal amount = rs.getBigDecimal("amount");
            if ("invoice".equals(rs.getString("kind"))) {
                return StatementEntry.invoice(date, invoiceNo, rs.getObject("due_date", LocalDate.class), amount);
            }
            return StatementEntry.payment(date, rs.getString("reference"), invoiceNo, rs.getString("method"), amount);
        });
    }

    private PostalAddress parseAddress(String json) {
        if (json == null || json.isBlank()) {
            return null;
        }
        try {
            return mapper.readValue(json, PostalAddress.class);
        } catch (JsonProcessingException e) {
            log.warn("unreadable billing_address json, statement will omit the address: {}", e.getOriginalMessage());
            return null;
        }
    }
}
