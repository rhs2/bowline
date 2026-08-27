package outbox

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"sort"
	"sync"
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus/testutil"
)

// clock is a manual clock so tests can move time forward deterministically.
type clock struct {
	mu sync.Mutex
	t  time.Time
}

func newClock() *clock {
	return &clock{t: time.Date(2026, 8, 27, 12, 0, 0, 0, time.UTC)}
}

func (c *clock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.t
}

func (c *clock) Advance(d time.Duration) {
	c.mu.Lock()
	c.t = c.t.Add(d)
	c.mu.Unlock()
}

type fakeRow struct {
	n         Notification
	status    Status
	next      time.Time
	lastError string
	sentAt    time.Time
}

// fakeStore models the notifications table in memory with the same claim
// semantics as the Postgres store: due rows are pending or sending rows whose
// next_attempt_at is not in the future.
type fakeStore struct {
	mu          sync.Mutex
	clock       *clock
	rows        map[string]*fakeRow
	claims      int
	claimErr    error
	markSentErr error
	released    [][]string
}

func newFakeStore(c *clock) *fakeStore {
	return &fakeStore{clock: c, rows: map[string]*fakeRow{}}
}

func (s *fakeStore) add(id string, status Status, next time.Time, attempts int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.rows[id] = &fakeRow{
		n: Notification{
			ID: id, RecipientID: "emp-" + id, Channel: "email",
			ToAddress: id + "@test.invalid", Subject: "Subject " + id,
			BodyText: "Body " + id, Attempts: attempts, CreatedAt: s.clock.Now(),
		},
		status: status,
		next:   next,
	}
}

func (s *fakeStore) row(t *testing.T, id string) fakeRow {
	t.Helper()
	s.mu.Lock()
	defer s.mu.Unlock()
	r, ok := s.rows[id]
	if !ok {
		t.Fatalf("row %s missing", id)
	}
	return *r
}

func (s *fakeStore) Claim(_ context.Context, limit int, lease time.Duration) ([]Notification, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.claims++
	if s.claimErr != nil {
		return nil, s.claimErr
	}
	now := s.clock.Now()
	var due []*fakeRow
	for _, r := range s.rows {
		if (r.status == StatusPending || r.status == StatusSending) && !r.next.After(now) {
			due = append(due, r)
		}
	}
	sort.Slice(due, func(i, j int) bool {
		if due[i].next.Equal(due[j].next) {
			return due[i].n.ID < due[j].n.ID
		}
		return due[i].next.Before(due[j].next)
	})
	if len(due) > limit {
		due = due[:limit]
	}
	out := make([]Notification, 0, len(due))
	for _, r := range due {
		r.status = StatusSending
		r.next = now.Add(lease)
		out = append(out, r.n)
	}
	return out, nil
}

func (s *fakeStore) MarkSent(_ context.Context, id string, sentAt time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.markSentErr != nil {
		return s.markSentErr
	}
	r, ok := s.rows[id]
	if !ok {
		return errors.New("row not found")
	}
	r.status = StatusSent
	r.sentAt = sentAt
	return nil
}

func (s *fakeStore) MarkFailed(_ context.Context, id string, f Failure) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, ok := s.rows[id]
	if !ok {
		return errors.New("row not found")
	}
	r.n.Attempts = f.Attempts
	r.lastError = f.Error
	if f.Parked {
		r.status = StatusFailed
		return nil
	}
	r.status = StatusPending
	r.next = f.NextAttemptAt
	return nil
}

func (s *fakeStore) Release(_ context.Context, ids []string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.released = append(s.released, ids)
	for _, id := range ids {
		if r, ok := s.rows[id]; ok && r.status == StatusSending {
			r.status = StatusPending
			r.next = s.clock.Now()
		}
	}
	return nil
}

func (s *fakeStore) Depth(context.Context) (int64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var n int64
	for _, r := range s.rows {
		if r.status == StatusPending || r.status == StatusSending {
			n++
		}
	}
	return n, nil
}

type fakeSender struct {
	mu   sync.Mutex
	sent []string
	fn   func(ctx context.Context, n Notification) error
}

func (s *fakeSender) Send(ctx context.Context, n Notification) error {
	s.mu.Lock()
	s.sent = append(s.sent, n.ID)
	s.mu.Unlock()
	if s.fn != nil {
		return s.fn(ctx, n)
	}
	return nil
}

func (s *fakeSender) ids() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.sent...)
}

func newTestWorker(store Store, sender Sender, cfg Config, clk *clock) *Worker {
	return New(store, sender, cfg,
		WithClock(clk.Now),
		WithBackoff(func(a int) time.Duration { return Backoff(a, 0) }),
		WithLogger(slog.New(slog.NewTextHandler(io.Discard, nil))),
	)
}

func TestPollDeliversPendingRows(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	for _, id := range []string{"a", "b", "c"} {
		store.add(id, StatusPending, clk.Now(), 0)
	}
	sender := &fakeSender{}
	w := newTestWorker(store, sender, Config{}, clk)

	n, err := w.Poll(context.Background())
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if n != 3 {
		t.Fatalf("claimed %d rows, want 3", n)
	}
	for _, id := range []string{"a", "b", "c"} {
		r := store.row(t, id)
		if r.status != StatusSent {
			t.Errorf("row %s status %s, want sent", id, r.status)
		}
		if !r.sentAt.Equal(clk.Now()) {
			t.Errorf("row %s sent_at %v, want %v", id, r.sentAt, clk.Now())
		}
	}
	if got := sender.ids(); len(got) != 3 {
		t.Errorf("sender saw %v", got)
	}
	if got := testutil.ToFloat64(w.metrics.Sent); got != 3 {
		t.Errorf("sent counter %v, want 3", got)
	}
	w.RefreshDepth(context.Background())
	if got := testutil.ToFloat64(w.metrics.Depth); got != 0 {
		t.Errorf("depth gauge %v, want 0", got)
	}
}

func TestFailedSendSchedulesRetryWithBackoff(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	store.add("a", StatusPending, clk.Now(), 0)
	failures := 2
	var seenAttempts []int
	sender := &fakeSender{fn: func(_ context.Context, n Notification) error {
		seenAttempts = append(seenAttempts, n.Attempts)
		if failures > 0 {
			failures--
			return errors.New("451 4.3.0 try again later")
		}
		return nil
	}}
	w := newTestWorker(store, sender, Config{}, clk)
	ctx := context.Background()

	// First attempt fails: attempts becomes 1, next try in 30s.
	if _, err := w.Poll(ctx); err != nil {
		t.Fatal(err)
	}
	r := store.row(t, "a")
	if r.status != StatusPending || r.n.Attempts != 1 {
		t.Fatalf("after first failure: status %s attempts %d", r.status, r.n.Attempts)
	}
	if r.lastError == "" || r.lastError != "451 4.3.0 try again later" {
		t.Errorf("last_error = %q", r.lastError)
	}
	if want := clk.Now().Add(30 * time.Second); !r.next.Equal(want) {
		t.Errorf("next attempt %v, want %v", r.next, want)
	}

	// Not due yet: nothing is claimed.
	if n, _ := w.Poll(ctx); n != 0 {
		t.Fatalf("claimed %d rows before the retry was due", n)
	}

	// Second attempt fails: backoff doubles to 60s.
	clk.Advance(30 * time.Second)
	if _, err := w.Poll(ctx); err != nil {
		t.Fatal(err)
	}
	r = store.row(t, "a")
	if r.n.Attempts != 2 {
		t.Fatalf("attempts = %d, want 2", r.n.Attempts)
	}
	if want := clk.Now().Add(60 * time.Second); !r.next.Equal(want) {
		t.Errorf("next attempt %v, want %v", r.next, want)
	}

	// Third attempt succeeds.
	clk.Advance(60 * time.Second)
	if _, err := w.Poll(ctx); err != nil {
		t.Fatal(err)
	}
	r = store.row(t, "a")
	if r.status != StatusSent {
		t.Fatalf("status %s, want sent", r.status)
	}
	if want := []int{0, 1, 2}; len(seenAttempts) != 3 || seenAttempts[0] != want[0] || seenAttempts[1] != want[1] || seenAttempts[2] != want[2] {
		t.Errorf("sender saw attempts %v, want %v", seenAttempts, want)
	}
	if got := testutil.ToFloat64(w.metrics.Retried); got != 2 {
		t.Errorf("retried counter %v, want 2", got)
	}
	if got := testutil.ToFloat64(w.metrics.Sent); got != 1 {
		t.Errorf("sent counter %v, want 1", got)
	}
}

func TestParkAfterMaxAttempts(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	store.add("a", StatusPending, clk.Now(), 0)
	sender := &fakeSender{fn: func(context.Context, Notification) error {
		return errors.New("connection refused")
	}}
	w := newTestWorker(store, sender, Config{MaxAttempts: 3}, clk)
	ctx := context.Background()

	for i := 0; i < 3; i++ {
		if _, err := w.Poll(ctx); err != nil {
			t.Fatal(err)
		}
		clk.Advance(time.Hour)
	}
	r := store.row(t, "a")
	if r.status != StatusFailed {
		t.Fatalf("status %s, want failed", r.status)
	}
	if r.n.Attempts != 3 {
		t.Errorf("attempts = %d, want 3", r.n.Attempts)
	}
	if r.lastError != "connection refused" {
		t.Errorf("last_error = %q", r.lastError)
	}
	if n, _ := w.Poll(ctx); n != 0 {
		t.Errorf("parked row was claimed again (%d rows)", n)
	}
	if got := testutil.ToFloat64(w.metrics.Failed); got != 1 {
		t.Errorf("failed counter %v, want 1", got)
	}
	if got := testutil.ToFloat64(w.metrics.Retried); got != 2 {
		t.Errorf("retried counter %v, want 2", got)
	}
}

func TestPermanentErrorParksImmediately(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	store.add("a", StatusPending, clk.Now(), 0)
	sender := &fakeSender{fn: func(context.Context, Notification) error {
		return Permanent(errors.New("550 5.1.1 no such user"))
	}}
	w := newTestWorker(store, sender, Config{MaxAttempts: 8}, clk)

	if _, err := w.Poll(context.Background()); err != nil {
		t.Fatal(err)
	}
	r := store.row(t, "a")
	if r.status != StatusFailed || r.n.Attempts != 1 {
		t.Fatalf("status %s attempts %d, want failed after one attempt", r.status, r.n.Attempts)
	}
	if got := testutil.ToFloat64(w.metrics.Failed); got != 1 {
		t.Errorf("failed counter %v, want 1", got)
	}
	if got := testutil.ToFloat64(w.metrics.Retried); got != 0 {
		t.Errorf("retried counter %v, want 0", got)
	}
}

func TestStaleSendingRowsAreReclaimed(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	// Claimed by a worker that died; its lease ran out a second ago.
	store.add("stale", StatusSending, clk.Now().Add(-time.Second), 1)
	// Claimed by a live worker five minutes ago; lease still running.
	store.add("fresh", StatusSending, clk.Now().Add(5*time.Minute), 0)
	sender := &fakeSender{}
	w := newTestWorker(store, sender, Config{}, clk)

	n, err := w.Poll(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("claimed %d rows, want 1", n)
	}
	if got := sender.ids(); len(got) != 1 || got[0] != "stale" {
		t.Fatalf("sender saw %v, want [stale]", got)
	}
	if r := store.row(t, "stale"); r.status != StatusSent {
		t.Errorf("stale row status %s, want sent", r.status)
	}
	if r := store.row(t, "fresh"); r.status != StatusSending {
		t.Errorf("fresh row status %s, want sending (untouched)", r.status)
	}
}

func TestShutdownMidBatchReleasesUnsentRows(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	for _, id := range []string{"a", "b", "c", "d", "e"} {
		store.add(id, StatusPending, clk.Now(), 0)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	sender := &fakeSender{fn: func(sendCtx context.Context, n Notification) error {
		if n.ID == "b" {
			cancel() // SIGTERM arrives while b is on the wire
		}
		if sendCtx.Err() != nil {
			return errors.New("send context must not be cancelled by shutdown")
		}
		return nil
	}}
	w := newTestWorker(store, sender, Config{BatchSize: 5}, clk)

	n, err := w.Poll(ctx)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("Poll err = %v, want context.Canceled", err)
	}
	if n != 5 {
		t.Errorf("claimed %d, want 5", n)
	}
	if got := sender.ids(); len(got) != 2 || got[0] != "a" || got[1] != "b" {
		t.Errorf("sender saw %v, want [a b]", got)
	}
	for _, id := range []string{"a", "b"} {
		if r := store.row(t, id); r.status != StatusSent {
			t.Errorf("row %s status %s, want sent", id, r.status)
		}
	}
	for _, id := range []string{"c", "d", "e"} {
		r := store.row(t, id)
		if r.status != StatusPending {
			t.Errorf("row %s status %s, want pending", id, r.status)
		}
		if !r.next.Equal(clk.Now()) {
			t.Errorf("row %s next %v, want now (released, not leased)", id, r.next)
		}
	}
	if len(store.released) != 1 || len(store.released[0]) != 3 {
		t.Errorf("released calls = %v, want one call with 3 ids", store.released)
	}
}

func TestRunDrainsBacklogAndStopsOnCancel(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	for i := 0; i < 7; i++ {
		store.add(string(rune('a'+i)), StatusPending, clk.Now(), 0)
	}
	sender := &fakeSender{}
	w := newTestWorker(store, sender, Config{BatchSize: 3, PollInterval: 5 * time.Millisecond, DepthInterval: 5 * time.Millisecond}, clk)

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- w.Run(ctx) }()

	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		d, _ := store.Depth(context.Background())
		if d == 0 && testutil.ToFloat64(w.metrics.Depth) == 0 {
			break
		}
		time.Sleep(2 * time.Millisecond)
	}
	if d, _ := store.Depth(context.Background()); d != 0 {
		t.Fatalf("depth %d after run, want 0", d)
	}
	if got := testutil.ToFloat64(w.metrics.Depth); got != 0 {
		t.Errorf("depth gauge %v, want 0 after periodic refresh", got)
	}
	cancel()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("Run returned %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Run did not return after cancel")
	}
	if got := testutil.ToFloat64(w.metrics.Sent); got != 7 {
		t.Errorf("sent counter %v, want 7", got)
	}
	// Three batches of 3, 3 and 1 rows without waiting a poll interval between them.
	if store.claims < 3 {
		t.Errorf("claims = %d, want at least 3", store.claims)
	}
}

func TestClaimErrorIsCountedAndRetriedNextPoll(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	store.add("a", StatusPending, clk.Now(), 0)
	dbDown := errors.New("connection reset")
	store.claimErr = dbDown
	sender := &fakeSender{}
	w := newTestWorker(store, sender, Config{}, clk)
	ctx := context.Background()

	if _, err := w.Poll(ctx); !errors.Is(err, dbDown) {
		t.Fatalf("Poll err = %v, want wrapped %v", err, dbDown)
	}
	w.drain(ctx)
	if got := testutil.ToFloat64(w.metrics.PollErrors); got != 1 {
		t.Errorf("poll_errors counter %v, want 1", got)
	}
	store.claimErr = nil
	if _, err := w.Poll(ctx); err != nil {
		t.Fatal(err)
	}
	if r := store.row(t, "a"); r.status != StatusSent {
		t.Errorf("status %s, want sent once the database is back", r.status)
	}
}

func TestSentButStatusWriteFailedLeavesRowLeased(t *testing.T) {
	clk := newClock()
	store := newFakeStore(clk)
	store.add("a", StatusPending, clk.Now(), 0)
	store.markSentErr = errors.New("connection reset")
	sender := &fakeSender{}
	w := newTestWorker(store, sender, Config{}, clk)

	if _, err := w.Poll(context.Background()); err != nil {
		t.Fatal(err)
	}
	r := store.row(t, "a")
	if r.status != StatusSending {
		t.Errorf("status %s, want sending (lease keeps it invisible until it expires)", r.status)
	}
	if want := clk.Now().Add(DefaultLease); !r.next.Equal(want) {
		t.Errorf("next %v, want lease expiry %v", r.next, want)
	}
	if got := testutil.ToFloat64(w.metrics.Sent); got != 0 {
		t.Errorf("sent counter %v, want 0 (delivery is only counted once recorded)", got)
	}
}

func TestBackoff(t *testing.T) {
	cases := []struct {
		attempts int
		jitter   float64
		want     time.Duration
	}{
		{0, 0, 30 * time.Second},
		{1, 0, 30 * time.Second},
		{2, 0, time.Minute},
		{3, 0, 2 * time.Minute},
		{7, 0, 32 * time.Minute},
		{8, 0, time.Hour},
		{9, 0, time.Hour},
		{50, 0, time.Hour},
		{1, 1, 30 * time.Second}, // out of range jitter is ignored
		{8, 0.5, 52*time.Minute + 30*time.Second},
	}
	for _, c := range cases {
		if got := Backoff(c.attempts, c.jitter); got != c.want {
			t.Errorf("Backoff(%d, %v) = %v, want %v", c.attempts, c.jitter, got, c.want)
		}
	}
	// Full jitter takes off just under a quarter and never goes past the cap.
	for a := 1; a <= 12; a++ {
		got := Backoff(a, 0.999)
		base := Backoff(a, 0)
		if got > base || got < base*3/4 {
			t.Errorf("Backoff(%d, 0.999) = %v, outside [%v, %v]", a, got, base*3/4, base)
		}
	}
	for i := 0; i < 100; i++ {
		if d := DefaultBackoff(20); d > MaxBackoff || d < MaxBackoff*3/4 {
			t.Fatalf("DefaultBackoff(20) = %v", d)
		}
	}
}

func TestPermanentHelpers(t *testing.T) {
	if Permanent(nil) != nil {
		t.Error("Permanent(nil) should be nil")
	}
	base := errors.New("boom")
	wrapped := Permanent(base)
	if !IsPermanent(wrapped) {
		t.Error("IsPermanent(Permanent(err)) = false")
	}
	if !errors.Is(wrapped, base) {
		t.Error("Permanent should unwrap to the original error")
	}
	if IsPermanent(base) {
		t.Error("plain error reported as permanent")
	}
}
