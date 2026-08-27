package main

import (
	"strings"
	"testing"
	"time"

	"github.com/rhs2/bowline/tools/internal/env"
)

func TestLoadConfigDefaultsAndOverrides(t *testing.T) {
	cfg, err := loadConfig(env.Map(map[string]string{
		"DATABASE_URL_NOTIFY": "postgres://n:p@localhost:5432/bowline",
		"MAIL_FROM":           `"Bowline <no-reply@bowline.example>"`,
	}))
	if err != nil {
		t.Fatalf("loadConfig: %v", err)
	}
	if cfg.Mail.From != "Bowline <no-reply@bowline.example>" {
		t.Errorf("MAIL_FROM quotes were not stripped: %q", cfg.Mail.From)
	}
	if cfg.Mail.Host != "localhost" || cfg.Mail.Port != 1025 || cfg.Mail.StartTLS {
		t.Errorf("mail defaults = %+v", cfg.Mail)
	}
	if cfg.Worker.PollInterval != 2*time.Second || cfg.Worker.BatchSize != 50 || cfg.Worker.MaxAttempts != 8 {
		t.Errorf("worker defaults = %+v", cfg.Worker)
	}
	if cfg.MetricsBind != "0.0.0.0:9101" || cfg.LogFormat != "json" {
		t.Errorf("service defaults = %+v", cfg)
	}

	cfg, err = loadConfig(env.Map(map[string]string{
		"DATABASE_URL_NOTIFY":     "postgres://n:p@db/bowline",
		"MAIL_FROM":               "no-reply@bowline.example",
		"SMTP_HOST":               "email-smtp.us-east-1.amazonaws.com",
		"SMTP_PORT":               "587",
		"SMTP_USERNAME":           "AKIA",
		"SMTP_PASSWORD":           "secret",
		"SMTP_STARTTLS":           "1",
		"NOTIFY_POLL_INTERVAL_MS": "500",
		"NOTIFY_BATCH_SIZE":       "10",
		"NOTIFY_MAX_ATTEMPTS":     "3",
		"NOTIFY_METRICS_BIND":     "127.0.0.1:9999",
		"LOG_FORMAT":              "pretty",
	}))
	if err != nil {
		t.Fatalf("loadConfig: %v", err)
	}
	if !cfg.Mail.StartTLS || cfg.Mail.Port != 587 || cfg.Mail.Username != "AKIA" {
		t.Errorf("mail = %+v", cfg.Mail)
	}
	if cfg.Worker.PollInterval != 500*time.Millisecond || cfg.Worker.BatchSize != 10 || cfg.Worker.MaxAttempts != 3 {
		t.Errorf("worker = %+v", cfg.Worker)
	}
}

func TestLoadConfigReportsEveryProblem(t *testing.T) {
	_, err := loadConfig(env.Map(map[string]string{
		"SMTP_PORT":           "abc",
		"NOTIFY_BATCH_SIZE":   "0",
		"NOTIFY_MAX_ATTEMPTS": "0",
		"SMTP_USERNAME":       "user",
	}))
	if err == nil {
		t.Fatal("expected errors")
	}
	for _, want := range []string{"DATABASE_URL_NOTIFY is required", "MAIL_FROM is required", "SMTP_PORT", "NOTIFY_BATCH_SIZE", "NOTIFY_MAX_ATTEMPTS", "SMTP_PASSWORD is required"} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("error lacks %q:\n%v", want, err)
		}
	}
}
