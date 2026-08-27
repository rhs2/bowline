package com.bowline.billing.reports;

import java.time.LocalDate;
import java.util.List;
import java.util.Map;
import org.springframework.jdbc.core.namedparam.NamedParameterJdbcTemplate;
import org.springframework.stereotype.Repository;

/**
 * Reads the {@code ar_aging} view through the read-only role. The view ages against
 * {@code current_date}; the query re-derives {@code days_overdue} and the bucket from
 * the requested report date so a back-dated report buckets consistently. Outstanding
 * amounts are the view's live balances.
 */
@Repository
public class JdbcArAgingRepository implements ArAgingRepository {

    private static final String SQL = """
            select invoice_no,
                   customer_name,
                   due_date,
                   outstanding,
                   greatest(cast(:as_of as date) - due_date, 0) as days_overdue,
                   case when cast(:as_of as date) <= due_date            then 'current'
                        when cast(:as_of as date) - due_date <= 30       then '1-30'
                        when cast(:as_of as date) - due_date <= 60       then '31-60'
                        when cast(:as_of as date) - due_date <= 90       then '61-90'
                        else '90+' end                                   as bucket
              from ar_aging
             where outstanding > 0
             order by due_date, invoice_no
            """;

    private final NamedParameterJdbcTemplate jdbc;

    public JdbcArAgingRepository(NamedParameterJdbcTemplate jdbc) {
        this.jdbc = jdbc;
    }

    @Override
    public List<ArAgingRow> findOutstanding(LocalDate asOf) {
        return jdbc.query(SQL, Map.of("as_of", asOf), (rs, i) -> new ArAgingRow(
                rs.getString("invoice_no"),
                rs.getString("customer_name"),
                rs.getObject("due_date", LocalDate.class),
                rs.getInt("days_overdue"),
                rs.getString("bucket"),
                rs.getBigDecimal("outstanding")));
    }
}
