// Package outbox implements the transactional outbox worker that turns rows of
// the notifications table into delivered email.
//
// The API writes a notification row in the same transaction as the business
// change it belongs to. The worker claims due rows with FOR UPDATE SKIP LOCKED,
// hands each one to a Sender, and records the outcome. Every step is idempotent
// enough to survive a crash: a row claimed by a worker that dies becomes due
// again when its lease expires.
package outbox

import (
	"context"
	"errors"
	"math/rand/v2"
	"time"
)

// Status is the value of notifications.status.
type Status string

// The four states a notification row moves through.
const (
	StatusPending Status = "pending"
	StatusSending Status = "sending"
	StatusSent    Status = "sent"
	StatusFailed  Status = "failed"
)

// Notification is one row of the notifications table as the worker sees it.
type Notification struct {
	ID          string
	RecipientID string
	Channel     string
	ToAddress   string
	Subject     string
	BodyText    string
	// Attempts is the number of delivery attempts made before this claim.
	Attempts  int
	CreatedAt time.Time
}

// Failure records the outcome of a delivery attempt that did not succeed.
type Failure struct {
	// Attempts is the new total, including the attempt that just failed.
	Attempts int
	// Error is the message stored in last_error.
	Error string
	// NextAttemptAt is when the row becomes due again. Ignored when Parked.
	NextAttemptAt time.Time
	// Parked marks the row failed for good: no further attempts are made.
	Parked bool
}

// Store is the persistence side of the outbox. The Postgres implementation
// lives in pgstore.go; tests use an in-memory fake.
type Store interface {
	// Claim marks up to limit due rows as sending and returns them. A claimed
	// row is invisible to other claims for the lease duration; after that it is
	// considered abandoned and becomes due again unless it was marked sent,
	// marked failed or released in the meantime.
	Claim(ctx context.Context, limit int, lease time.Duration) ([]Notification, error)
	// MarkSent records a successful delivery.
	MarkSent(ctx context.Context, id string, sentAt time.Time) error
	// MarkFailed records a failed attempt, either scheduling a retry or parking
	// the row as failed.
	MarkFailed(ctx context.Context, id string, f Failure) error
	// Release hands claimed rows back to the queue immediately. The worker
	// calls it for rows it did not get to before a shutdown.
	Release(ctx context.Context, ids []string) error
	// Depth counts the rows still waiting to be delivered (pending or sending).
	Depth(ctx context.Context) (int64, error)
}

// Sender delivers one notification. Errors wrapped with Permanent are not
// retried.
type Sender interface {
	Send(ctx context.Context, n Notification) error
}

// PermanentError marks a delivery failure that will not succeed on retry, such
// as an unparseable recipient address or a 5xx SMTP reply.
type PermanentError struct {
	Err error
}

func (e *PermanentError) Error() string { return "permanent: " + e.Err.Error() }

// Unwrap exposes the underlying error to errors.Is and errors.As.
func (e *PermanentError) Unwrap() error { return e.Err }

// Permanent wraps err so the worker parks the notification instead of
// retrying it. A nil err returns nil.
func Permanent(err error) error {
	if err == nil {
		return nil
	}
	return &PermanentError{Err: err}
}

// IsPermanent reports whether err carries a PermanentError anywhere in its chain.
func IsPermanent(err error) bool {
	var p *PermanentError
	return errors.As(err, &p)
}

// Retry schedule: 30s, 1m, 2m, 4m, 8m, 16m, 32m, then one hour, each reduced
// by up to a quarter of jitter so retries from many rows do not line up.
const (
	BaseBackoff = 30 * time.Second
	MaxBackoff  = time.Hour
	maxJitter   = 0.25
)

// Backoff returns the delay before the next attempt once attempts have failed.
// jitter must be in [0, 1) and scales the deduction from the base delay; the
// result never exceeds MaxBackoff.
func Backoff(attempts int, jitter float64) time.Duration {
	if attempts < 1 {
		attempts = 1
	}
	exp := attempts - 1
	if exp > 7 { // 30s * 2^7 already exceeds the cap; avoid a large shift
		exp = 7
	}
	base := BaseBackoff << exp
	if base > MaxBackoff {
		base = MaxBackoff
	}
	if jitter < 0 || jitter >= 1 {
		jitter = 0
	}
	return base - time.Duration(float64(base)*maxJitter*jitter)
}

// DefaultBackoff is Backoff with random jitter.
func DefaultBackoff(attempts int) time.Duration {
	return Backoff(attempts, rand.Float64())
}
