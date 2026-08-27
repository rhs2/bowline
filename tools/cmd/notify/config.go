package main

import (
	"errors"
	"fmt"

	"github.com/rhs2/bowline/tools/internal/env"
	"github.com/rhs2/bowline/tools/internal/mail"
	"github.com/rhs2/bowline/tools/internal/outbox"
)

// config is everything notify reads from the environment. The variable names
// match .env.example at the repository root.
type config struct {
	DatabaseURL string
	MetricsBind string
	LogFormat   string
	LogLevel    string
	Worker      outbox.Config
	Mail        mail.Config
}

func loadConfig(lookup env.Lookup) (config, error) {
	r := env.New(lookup)
	cfg := config{
		DatabaseURL: r.Require("DATABASE_URL_NOTIFY"),
		MetricsBind: r.String("NOTIFY_METRICS_BIND", "0.0.0.0:9101"),
		LogFormat:   r.String("LOG_FORMAT", "json"),
		LogLevel:    r.String("LOG_LEVEL", "info"),
		Worker: outbox.Config{
			PollInterval: r.Millis("NOTIFY_POLL_INTERVAL_MS", outbox.DefaultPollInterval),
			BatchSize:    r.Int("NOTIFY_BATCH_SIZE", outbox.DefaultBatchSize),
			MaxAttempts:  r.Int("NOTIFY_MAX_ATTEMPTS", outbox.DefaultMaxAttempts),
		},
		Mail: mail.Config{
			Host:     r.String("SMTP_HOST", "localhost"),
			Port:     r.Int("SMTP_PORT", 1025),
			Username: r.String("SMTP_USERNAME", ""),
			Password: r.String("SMTP_PASSWORD", ""),
			StartTLS: r.Bool("SMTP_STARTTLS", false),
			From:     env.Unquote(r.Require("MAIL_FROM")),
		},
	}
	var errs []error
	if err := r.Err(); err != nil {
		errs = append(errs, err)
	}
	if cfg.Worker.PollInterval <= 0 {
		errs = append(errs, errors.New("NOTIFY_POLL_INTERVAL_MS must be positive"))
	}
	if cfg.Worker.BatchSize < 1 || cfg.Worker.BatchSize > 1000 {
		errs = append(errs, fmt.Errorf("NOTIFY_BATCH_SIZE must be between 1 and 1000 (got %d)", cfg.Worker.BatchSize))
	}
	if cfg.Worker.MaxAttempts < 1 {
		errs = append(errs, fmt.Errorf("NOTIFY_MAX_ATTEMPTS must be at least 1 (got %d)", cfg.Worker.MaxAttempts))
	}
	if cfg.Mail.Username != "" && cfg.Mail.Password == "" {
		errs = append(errs, errors.New("SMTP_PASSWORD is required when SMTP_USERNAME is set"))
	}
	return cfg, errors.Join(errs...)
}
