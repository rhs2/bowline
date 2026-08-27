package com.bowline.billing.reports;

import java.time.Clock;
import java.time.LocalDate;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.format.annotation.DateTimeFormat;
import org.springframework.http.ContentDisposition;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

/** {@code GET /reports/ar-aging.xlsx?as_of=YYYY-MM-DD} (defaults to today, UTC). */
@RestController
public class ArAgingController {

    public static final MediaType XLSX =
            MediaType.parseMediaType("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");

    private static final Logger log = LoggerFactory.getLogger(ArAgingController.class);

    private final ArAgingRepository repository;
    private final ArAgingWorkbook workbook;
    private final Clock clock;

    public ArAgingController(ArAgingRepository repository, ArAgingWorkbook workbook, Clock clock) {
        this.repository = repository;
        this.workbook = workbook;
        this.clock = clock;
    }

    @GetMapping("/reports/ar-aging.xlsx")
    public ResponseEntity<byte[]> arAging(
            @RequestParam(name = "as_of", required = false) @DateTimeFormat(iso = DateTimeFormat.ISO.DATE) LocalDate asOf) {
        LocalDate reportDate = asOf == null ? LocalDate.now(clock) : asOf;
        List<ArAgingRow> rows = repository.findOutstanding(reportDate);
        byte[] bytes = workbook.build(reportDate, rows);
        log.info("ar aging as of {}: {} rows, {} bytes", reportDate, rows.size(), bytes.length);
        return ResponseEntity.ok()
                .contentType(XLSX)
                .header(HttpHeaders.CONTENT_DISPOSITION, ContentDisposition.attachment()
                        .filename("ar-aging-" + reportDate + ".xlsx").build().toString())
                .body(bytes);
    }
}
