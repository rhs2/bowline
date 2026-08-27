// Command notify is the Bowline transactional outbox worker. It claims due rows
// of the notifications table, delivers them over SMTP, records the outcome,
// and exposes Prometheus metrics and health probes.
package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/collectors"
	"github.com/prometheus/client_golang/prometheus/promhttp"

	"github.com/rhs2/bowline/tools/internal/env"
	"github.com/rhs2/bowline/tools/internal/mail"
	"github.com/rhs2/bowline/tools/internal/outbox"
)

// version is set at build time with -ldflags "-X main.version=...".
var version = "dev"

const (
	shutdownGrace = 30 * time.Second
	startupPing   = 15 * time.Second
	readyzTimeout = 2 * time.Second
)

func main() {
	if err := run(); err != nil {
		slog.Error("notify exited", "error", err)
		os.Exit(1)
	}
}

func run() error {
	cfg, err := loadConfig(env.OS)
	if err != nil {
		return fmt.Errorf("configuration: %w", err)
	}
	logger := newLogger(cfg.LogFormat, cfg.LogLevel)
	slog.SetDefault(logger)
	logger.Info("starting notify",
		"version", version,
		"smtp", net.JoinHostPort(cfg.Mail.Host, fmt.Sprint(cfg.Mail.Port)),
		"starttls", cfg.Mail.StartTLS,
		"auth", cfg.Mail.Username != "",
		"metrics_bind", cfg.MetricsBind)

	sigCtx, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer stop()
	ctx, cancel := context.WithCancel(sigCtx)
	defer cancel()

	pool, err := openPool(ctx, cfg.DatabaseURL)
	if err != nil {
		return err
	}
	defer pool.Close()
	store := outbox.NewPGStore(pool)

	sender, err := mail.NewSMTPSender(cfg.Mail)
	if err != nil {
		return err
	}

	reg := prometheus.NewRegistry()
	reg.MustRegister(
		collectors.NewGoCollector(),
		collectors.NewProcessCollector(collectors.ProcessCollectorOpts{}),
	)
	metrics := outbox.NewMetrics(reg)
	worker := outbox.New(store, sender, cfg.Worker,
		outbox.WithMetrics(metrics),
		outbox.WithLogger(logger))

	ln, err := net.Listen("tcp", cfg.MetricsBind)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", cfg.MetricsBind, err)
	}
	srv := &http.Server{
		Handler:           newMux(reg, store),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      10 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	fatal := make(chan error, 2)
	go func() {
		if err := srv.Serve(ln); err != nil && !errors.Is(err, http.ErrServerClosed) {
			fatal <- fmt.Errorf("metrics server: %w", err)
		}
	}()
	workerDone := make(chan struct{})
	go func() {
		defer close(workerDone)
		if err := worker.Run(ctx); err != nil {
			fatal <- fmt.Errorf("worker: %w", err)
		}
	}()

	var exitErr error
	select {
	case <-ctx.Done():
		logger.Info("shutdown signal received; finishing the message in flight")
	case exitErr = <-fatal:
		logger.Error("fatal error, shutting down", "error", exitErr)
	}
	stop() // a second signal now terminates the process the default way
	cancel()

	graceCtx, cancelGrace := context.WithTimeout(context.Background(), shutdownGrace)
	defer cancelGrace()
	select {
	case <-workerDone:
	case <-graceCtx.Done():
		logger.Warn("worker did not stop within the grace period", "grace", shutdownGrace)
	}
	if err := srv.Shutdown(graceCtx); err != nil {
		logger.Warn("metrics server shutdown", "error", err)
	}
	logger.Info("notify stopped")
	return exitErr
}

func openPool(ctx context.Context, dsn string) (*pgxpool.Pool, error) {
	poolCfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return nil, fmt.Errorf("DATABASE_URL_NOTIFY: %w", err)
	}
	poolCfg.MaxConns = 4
	poolCfg.ConnConfig.RuntimeParams["application_name"] = "bowline-notify"
	if poolCfg.ConnConfig.ConnectTimeout == 0 {
		poolCfg.ConnConfig.ConnectTimeout = 10 * time.Second
	}
	pool, err := pgxpool.NewWithConfig(ctx, poolCfg)
	if err != nil {
		return nil, fmt.Errorf("open database pool: %w", err)
	}
	pingCtx, cancel := context.WithTimeout(ctx, startupPing)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("database unreachable: %w", err)
	}
	return pool, nil
}

func newMux(reg *prometheus.Registry, store *outbox.PGStore) http.Handler {
	mux := http.NewServeMux()
	mux.Handle("GET /metrics", promhttp.HandlerFor(reg, promhttp.HandlerOpts{}))
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		_, _ = w.Write([]byte("ok\n"))
	})
	mux.HandleFunc("GET /readyz", func(w http.ResponseWriter, r *http.Request) {
		ctx, cancel := context.WithTimeout(r.Context(), readyzTimeout)
		defer cancel()
		if err := store.Ping(ctx); err != nil {
			http.Error(w, "database: "+err.Error(), http.StatusServiceUnavailable)
			return
		}
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		_, _ = w.Write([]byte("ready\n"))
	})
	return mux
}

func newLogger(format, level string) *slog.Logger {
	var lvl slog.Level
	if err := lvl.UnmarshalText([]byte(level)); err != nil {
		lvl = slog.LevelInfo
	}
	opts := &slog.HandlerOptions{Level: lvl}
	switch format {
	case "pretty", "text":
		return slog.New(slog.NewTextHandler(os.Stderr, opts))
	default:
		return slog.New(slog.NewJSONHandler(os.Stdout, opts))
	}
}
