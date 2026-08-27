package outbox

import (
	"context"
	"fmt"
	"log/slog"
	"time"
)

// Config tunes the worker. Zero values are replaced by the defaults below.
type Config struct {
	// PollInterval is the pause between polls when the queue is empty.
	PollInterval time.Duration
	// BatchSize is the maximum number of rows claimed per poll.
	BatchSize int
	// MaxAttempts is the number of attempts after which a row is parked as failed.
	MaxAttempts int
	// Lease is how long a claimed row stays invisible before it is treated as
	// abandoned by a crashed worker and reclaimed.
	Lease time.Duration
	// SendTimeout bounds one SMTP delivery attempt.
	SendTimeout time.Duration
	// StoreTimeout bounds one store call.
	StoreTimeout time.Duration
	// DepthInterval is how often the outbox depth gauge is refreshed.
	DepthInterval time.Duration
}

// Defaults applied by Config.withDefaults.
const (
	DefaultPollInterval  = 2 * time.Second
	DefaultBatchSize     = 50
	DefaultMaxAttempts   = 8
	DefaultLease         = 10 * time.Minute
	DefaultSendTimeout   = 30 * time.Second
	DefaultStoreTimeout  = 10 * time.Second
	DefaultDepthInterval = 15 * time.Second

	maxStoredErrorLen = 2000
)

func (c Config) withDefaults() Config {
	if c.PollInterval <= 0 {
		c.PollInterval = DefaultPollInterval
	}
	if c.BatchSize <= 0 {
		c.BatchSize = DefaultBatchSize
	}
	if c.MaxAttempts <= 0 {
		c.MaxAttempts = DefaultMaxAttempts
	}
	if c.Lease <= 0 {
		c.Lease = DefaultLease
	}
	if c.SendTimeout <= 0 {
		c.SendTimeout = DefaultSendTimeout
	}
	if c.StoreTimeout <= 0 {
		c.StoreTimeout = DefaultStoreTimeout
	}
	if c.DepthInterval <= 0 {
		c.DepthInterval = DefaultDepthInterval
	}
	return c
}

// Worker polls the Store and delivers notifications through the Sender.
type Worker struct {
	store   Store
	sender  Sender
	cfg     Config
	metrics *Metrics
	log     *slog.Logger
	now     func() time.Time
	backoff func(attempts int) time.Duration
}

// Option customises a Worker.
type Option func(*Worker)

// WithMetrics uses m instead of a fresh unregistered set.
func WithMetrics(m *Metrics) Option { return func(w *Worker) { w.metrics = m } }

// WithLogger sets the logger; the default is slog.Default.
func WithLogger(l *slog.Logger) Option { return func(w *Worker) { w.log = l } }

// WithClock replaces time.Now, which lets tests control next_attempt_at.
func WithClock(now func() time.Time) Option { return func(w *Worker) { w.now = now } }

// WithBackoff replaces DefaultBackoff, which lets tests remove the jitter.
func WithBackoff(f func(attempts int) time.Duration) Option {
	return func(w *Worker) { w.backoff = f }
}

// New builds a Worker. Unset Config fields take their defaults.
func New(store Store, sender Sender, cfg Config, opts ...Option) *Worker {
	w := &Worker{
		store:   store,
		sender:  sender,
		cfg:     cfg.withDefaults(),
		now:     time.Now,
		backoff: DefaultBackoff,
	}
	for _, o := range opts {
		o(w)
	}
	if w.metrics == nil {
		w.metrics = NewMetrics(nil)
	}
	if w.log == nil {
		w.log = slog.Default()
	}
	return w
}

// Config returns the effective configuration after defaults were applied.
func (w *Worker) Config() Config { return w.cfg }

// Run polls until ctx is cancelled and returns nil on a clean stop. A batch in
// progress finishes the message currently being sent, releases the rest of the
// batch, and then Run returns.
func (w *Worker) Run(ctx context.Context) error {
	w.log.Info("outbox worker started",
		"poll_interval", w.cfg.PollInterval,
		"batch_size", w.cfg.BatchSize,
		"max_attempts", w.cfg.MaxAttempts,
		"lease", w.cfg.Lease)

	w.RefreshDepth(ctx)
	poll := time.NewTicker(w.cfg.PollInterval)
	defer poll.Stop()
	depth := time.NewTicker(w.cfg.DepthInterval)
	defer depth.Stop()

	for {
		w.drain(ctx)
		select {
		case <-ctx.Done():
			w.log.Info("outbox worker stopped")
			return nil
		case <-poll.C:
		case <-depth.C:
			w.RefreshDepth(ctx)
		}
	}
}

// drain polls back to back while full batches keep coming, so a backlog is
// worked off without waiting a poll interval between batches.
func (w *Worker) drain(ctx context.Context) {
	for ctx.Err() == nil {
		n, err := w.Poll(ctx)
		if err != nil {
			if ctx.Err() == nil {
				w.metrics.PollErrors.Inc()
				w.log.Error("poll failed", "error", err)
			}
			return
		}
		if n < w.cfg.BatchSize {
			return
		}
	}
}

// Poll claims one batch and delivers it, returning the number of rows claimed.
// If ctx is cancelled part way through, the rows not yet attempted are released
// back to the queue and ctx.Err() is returned.
func (w *Worker) Poll(ctx context.Context) (int, error) {
	claimCtx, cancel := context.WithTimeout(ctx, w.cfg.StoreTimeout)
	batch, err := w.store.Claim(claimCtx, w.cfg.BatchSize, w.cfg.Lease)
	cancel()
	if err != nil {
		return 0, fmt.Errorf("claim batch: %w", err)
	}
	w.metrics.BatchSize.Observe(float64(len(batch)))
	if len(batch) == 0 {
		return 0, nil
	}
	w.log.Debug("claimed batch", "size", len(batch))

	for i, n := range batch {
		if ctx.Err() != nil {
			w.release(ctx, batch[i:])
			return len(batch), ctx.Err()
		}
		w.deliver(ctx, n)
	}
	return len(batch), nil
}

// RefreshDepth samples the outbox depth into the gauge.
func (w *Worker) RefreshDepth(ctx context.Context) {
	dctx, cancel := context.WithTimeout(ctx, w.cfg.StoreTimeout)
	defer cancel()
	depth, err := w.store.Depth(dctx)
	if err != nil {
		if ctx.Err() == nil {
			w.log.Warn("outbox depth query failed", "error", err)
		}
		return
	}
	w.metrics.Depth.Set(float64(depth))
}

// deliver sends one notification and records the outcome. The parent context
// may be cancelled for shutdown while the message is in flight; the SMTP
// conversation and the status write run to completion under their own
// timeouts so that a message the server accepted is always recorded as sent.
func (w *Worker) deliver(ctx context.Context, n Notification) {
	detached := context.WithoutCancel(ctx)
	attempt := n.Attempts + 1
	log := w.log.With("notification_id", n.ID, "recipient_id", n.RecipientID, "attempt", attempt)

	sendCtx, cancelSend := context.WithTimeout(detached, w.cfg.SendTimeout)
	start := time.Now()
	sendErr := w.sender.Send(sendCtx, n)
	elapsed := time.Since(start)
	cancelSend()
	w.metrics.SendLatency.Observe(elapsed.Seconds())

	storeCtx, cancelStore := context.WithTimeout(detached, w.cfg.StoreTimeout)
	defer cancelStore()

	if sendErr == nil {
		if err := w.store.MarkSent(storeCtx, n.ID, w.now()); err != nil {
			log.Error("message accepted by the mail server but the status write failed; the row will be redelivered when its lease expires", "error", err)
			return
		}
		w.metrics.Sent.Inc()
		log.Info("notification sent", "to", n.ToAddress, "latency_ms", elapsed.Milliseconds())
		return
	}

	f := Failure{Attempts: attempt, Error: truncate(sendErr.Error(), maxStoredErrorLen)}
	switch {
	case IsPermanent(sendErr):
		f.Parked = true
	case attempt >= w.cfg.MaxAttempts:
		f.Parked = true
	default:
		f.NextAttemptAt = w.now().Add(w.backoff(attempt))
	}
	if err := w.store.MarkFailed(storeCtx, n.ID, f); err != nil {
		log.Error("status write failed after a send error", "send_error", sendErr, "error", err)
		return
	}
	if f.Parked {
		w.metrics.Failed.Inc()
		log.Error("notification parked as failed", "error", sendErr, "permanent", IsPermanent(sendErr))
		return
	}
	w.metrics.Retried.Inc()
	log.Warn("notification send failed, retry scheduled", "error", sendErr, "next_attempt_at", f.NextAttemptAt)
}

// release returns rows that were claimed but never attempted to the queue.
func (w *Worker) release(ctx context.Context, rest []Notification) {
	ids := make([]string, len(rest))
	for i, n := range rest {
		ids[i] = n.ID
	}
	rctx, cancel := context.WithTimeout(context.WithoutCancel(ctx), w.cfg.StoreTimeout)
	defer cancel()
	if err := w.store.Release(rctx, ids); err != nil {
		w.log.Error("could not release unsent rows; they will be reclaimed when their lease expires", "count", len(ids), "error", err)
		return
	}
	w.log.Info("released unsent rows for shutdown", "count", len(ids))
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
