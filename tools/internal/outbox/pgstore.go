package outbox

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PGStore is the Postgres Store. It only selects and updates the notifications
// table, which is all the bowline_notify role is granted. Every query casts the
// uuid and citext columns to text so no extension type has to be registered.
type PGStore struct {
	pool *pgxpool.Pool
}

// NewPGStore wraps a connection pool opened with DATABASE_URL_NOTIFY.
func NewPGStore(pool *pgxpool.Pool) *PGStore { return &PGStore{pool: pool} }

const (
	claimSelectSQL = `
SELECT id::text, recipient_id::text, channel, to_address::text, subject, body_text, attempts, created_at
FROM notifications
WHERE channel = 'email'
  AND status IN ('pending', 'sending')
  AND next_attempt_at <= now()
ORDER BY next_attempt_at
LIMIT $1
FOR UPDATE SKIP LOCKED`

	// The lease is written into next_attempt_at, so a row abandoned by a
	// crashed worker becomes due again through the same predicate as any other
	// row, without a separate reclaim sweep.
	claimUpdateSQL = `
UPDATE notifications
SET status = 'sending', next_attempt_at = now() + make_interval(secs => $2)
WHERE id = ANY($1::uuid[])`

	markSentSQL = `
UPDATE notifications
SET status = 'sent', sent_at = $2, last_error = NULL
WHERE id = $1::uuid`

	markFailedSQL = `
UPDATE notifications
SET status = $2, attempts = $3, last_error = $4, next_attempt_at = COALESCE($5::timestamptz, now())
WHERE id = $1::uuid`

	releaseSQL = `
UPDATE notifications
SET status = 'pending', next_attempt_at = now()
WHERE id = ANY($1::uuid[]) AND status = 'sending'`

	depthSQL = `
SELECT count(*) FROM notifications
WHERE channel = 'email' AND status IN ('pending', 'sending')`

	statsSQL = `
SELECT status, count(*) FROM notifications GROUP BY status`
)

// Claim implements Store. It runs SELECT ... FOR UPDATE SKIP LOCKED and the
// status update in one transaction, so concurrent workers never claim the same
// row and a worker that dies between the two statements leaves nothing behind.
func (s *PGStore) Claim(ctx context.Context, limit int, lease time.Duration) (batch []Notification, err error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin: %w", err)
	}
	defer func() {
		if err != nil {
			_ = tx.Rollback(ctx)
		}
	}()

	rows, err := tx.Query(ctx, claimSelectSQL, limit)
	if err != nil {
		return nil, fmt.Errorf("select due rows: %w", err)
	}
	batch, err = pgx.CollectRows(rows, scanNotification)
	if err != nil {
		return nil, fmt.Errorf("scan due rows: %w", err)
	}
	if len(batch) == 0 {
		if err = tx.Commit(ctx); err != nil {
			return nil, fmt.Errorf("commit: %w", err)
		}
		return nil, nil
	}

	ids := make([]string, len(batch))
	for i, n := range batch {
		ids[i] = n.ID
	}
	tag, err := tx.Exec(ctx, claimUpdateSQL, ids, lease.Seconds())
	if err != nil {
		return nil, fmt.Errorf("mark sending: %w", err)
	}
	if got := tag.RowsAffected(); got != int64(len(ids)) {
		err = fmt.Errorf("mark sending: updated %d of %d locked rows", got, len(ids))
		return nil, err
	}
	if err = tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit: %w", err)
	}
	return batch, nil
}

func scanNotification(row pgx.CollectableRow) (Notification, error) {
	var n Notification
	err := row.Scan(&n.ID, &n.RecipientID, &n.Channel, &n.ToAddress, &n.Subject, &n.BodyText, &n.Attempts, &n.CreatedAt)
	return n, err
}

// MarkSent implements Store.
func (s *PGStore) MarkSent(ctx context.Context, id string, sentAt time.Time) error {
	tag, err := s.pool.Exec(ctx, markSentSQL, id, sentAt)
	if err != nil {
		return fmt.Errorf("mark sent: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("mark sent: notification %s no longer exists", id)
	}
	return nil
}

// MarkFailed implements Store.
func (s *PGStore) MarkFailed(ctx context.Context, id string, f Failure) error {
	status := StatusPending
	var next *time.Time
	if f.Parked {
		status = StatusFailed
	} else {
		t := f.NextAttemptAt
		next = &t
	}
	tag, err := s.pool.Exec(ctx, markFailedSQL, id, string(status), f.Attempts, f.Error, next)
	if err != nil {
		return fmt.Errorf("mark failed: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("mark failed: notification %s no longer exists", id)
	}
	return nil
}

// Release implements Store.
func (s *PGStore) Release(ctx context.Context, ids []string) error {
	if len(ids) == 0 {
		return nil
	}
	if _, err := s.pool.Exec(ctx, releaseSQL, ids); err != nil {
		return fmt.Errorf("release: %w", err)
	}
	return nil
}

// Depth implements Store.
func (s *PGStore) Depth(ctx context.Context) (int64, error) {
	var n int64
	if err := s.pool.QueryRow(ctx, depthSQL).Scan(&n); err != nil {
		return 0, fmt.Errorf("outbox depth: %w", err)
	}
	return n, nil
}

// Stats is a per-status row count of the whole table.
type Stats struct {
	Pending int64
	Sending int64
	Sent    int64
	Failed  int64
}

// Depth is the number of rows still waiting for delivery.
func (st Stats) Depth() int64 { return st.Pending + st.Sending }

// Stats counts rows by status. Used by bowctl; not part of the Store interface.
func (s *PGStore) Stats(ctx context.Context) (Stats, error) {
	rows, err := s.pool.Query(ctx, statsSQL)
	if err != nil {
		return Stats{}, fmt.Errorf("outbox stats: %w", err)
	}
	defer rows.Close()
	var st Stats
	for rows.Next() {
		var status string
		var n int64
		if err := rows.Scan(&status, &n); err != nil {
			return Stats{}, fmt.Errorf("outbox stats: %w", err)
		}
		switch Status(status) {
		case StatusPending:
			st.Pending = n
		case StatusSending:
			st.Sending = n
		case StatusSent:
			st.Sent = n
		case StatusFailed:
			st.Failed = n
		}
	}
	if err := rows.Err(); err != nil {
		return Stats{}, fmt.Errorf("outbox stats: %w", err)
	}
	return st, nil
}

// Ping checks that the database answers. Used by the readiness probe.
func (s *PGStore) Ping(ctx context.Context) error {
	return s.pool.Ping(ctx)
}
