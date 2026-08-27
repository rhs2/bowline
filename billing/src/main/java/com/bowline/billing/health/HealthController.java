package com.bowline.billing.health;

import com.bowline.billing.config.BillingProperties;
import java.util.Map;
import javax.sql.DataSource;
import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

/**
 * Liveness and readiness probes. {@code /healthz} only proves the process answers;
 * {@code /readyz} additionally checks the read-only datasource within one second.
 */
@RestController
public class HealthController {

    private final DatabaseReadinessCheck readiness;

    public HealthController(DataSource dataSource, BillingProperties properties) {
        this.readiness = new DatabaseReadinessCheck(dataSource, properties.readinessTimeout());
    }

    @GetMapping(value = "/healthz", produces = "application/json")
    public Map<String, String> healthz() {
        return Map.of("status", "ok");
    }

    @GetMapping(value = "/readyz", produces = "application/json")
    public ResponseEntity<Map<String, String>> readyz() {
        DatabaseReadinessCheck.Result result = readiness.check();
        Map<String, String> body = Map.of(
                "status", result.ready() ? "ok" : "unavailable",
                "database", result.detail());
        return ResponseEntity.status(result.ready() ? HttpStatus.OK : HttpStatus.SERVICE_UNAVAILABLE).body(body);
    }
}
