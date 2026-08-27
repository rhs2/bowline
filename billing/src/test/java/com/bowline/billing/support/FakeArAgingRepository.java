package com.bowline.billing.support;

import com.bowline.billing.reports.ArAgingRepository;
import com.bowline.billing.reports.ArAgingRow;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/** In-memory AR aging source; tests set the rows and inspect the requested date. */
public class FakeArAgingRepository implements ArAgingRepository {

    private final List<ArAgingRow> rows = new ArrayList<>();
    private LocalDate lastAsOf;

    public void reset(List<ArAgingRow> newRows) {
        rows.clear();
        rows.addAll(newRows);
        lastAsOf = null;
    }

    public LocalDate lastAsOf() {
        return lastAsOf;
    }

    @Override
    public List<ArAgingRow> findOutstanding(LocalDate asOf) {
        lastAsOf = asOf;
        return List.copyOf(rows);
    }
}
