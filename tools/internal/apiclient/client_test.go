package apiclient

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

// memTokens is an in-memory TokenSource.
type memTokens struct {
	mu     sync.Mutex
	t      Tokens
	stores int
	err    error
}

func (m *memTokens) Tokens(context.Context) (Tokens, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.t, m.err
}

func (m *memTokens) Store(_ context.Context, t Tokens) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.t = t
	m.stores++
	return nil
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeProblem(w http.ResponseWriter, status int, code, detail string) {
	w.Header().Set("Content-Type", "application/problem+json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"type": "about:blank", "title": http.StatusText(status), "status": status,
		"detail": detail, "code": code, "request_id": "req-1",
	})
}

func newClient(t *testing.T, srv *httptest.Server, ts TokenSource) *Client {
	t.Helper()
	opts := []Option{WithHTTPClient(srv.Client())}
	if ts != nil {
		opts = append(opts, WithTokenSource(ts))
	}
	c, err := New(srv.URL, opts...)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestLoginAndMe(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /api/v1/auth/login", func(w http.ResponseWriter, r *http.Request) {
		var in map[string]string
		_ = json.NewDecoder(r.Body).Decode(&in)
		if r.Header.Get("Content-Type") != "application/json" || in["email"] != "ceo@bowline.example" || in["password"] != "pw" {
			writeProblem(w, 401, "unauthorized", "bad credentials")
			return
		}
		writeJSON(w, 200, Session{AccessToken: "acc", RefreshToken: "ref", ExpiresIn: 900})
	})
	mux.HandleFunc("GET /api/v1/auth/me", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer acc" {
			writeProblem(w, 401, "unauthorized", "missing token")
			return
		}
		_, _ = w.Write([]byte(`{"user":{"id":"u1","email":"ceo@bowline.example"},
			"employee":{"id":"e1","employee_no":"EMP-000001","first_name":"Ada","last_name":"Chief","title":"Chief Executive Officer","department":{"id":"d1","name":"Executive Office"},"position":"CEO"},
			"roles":["ceo"],"permissions":["org:read","messages:broadcast:company"],
			"chain":[{"id":"e1","name":"Ada Chief","title":"Chief Executive Officer","level":1}]}`))
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()

	ts := &memTokens{}
	c := newClient(t, srv, ts)
	ctx := context.Background()

	sess, err := c.Login(ctx, "ceo@bowline.example", "pw")
	if err != nil {
		t.Fatalf("Login: %v", err)
	}
	if sess.AccessToken != "acc" || sess.RefreshToken != "ref" || sess.ExpiresIn != 900 {
		t.Fatalf("session = %+v", sess)
	}
	if _, err := c.Login(ctx, "ceo@bowline.example", "nope"); err == nil {
		t.Fatal("bad password should fail")
	}

	if _, err := c.Me(ctx); !errors.Is(err, ErrNotLoggedIn) {
		t.Fatalf("Me without tokens: err = %v, want ErrNotLoggedIn", err)
	}
	_ = ts.Store(ctx, sess.Tokens(time.Now()))
	me, err := c.Me(ctx)
	if err != nil {
		t.Fatalf("Me: %v", err)
	}
	if me.User.Email != "ceo@bowline.example" || me.Employee.FullName() != "Ada Chief" {
		t.Errorf("me = %+v", me)
	}
	if me.Employee.Department.Name != "Executive Office" || me.Employee.Department.ID != "d1" {
		t.Errorf("department ref = %+v", me.Employee.Department)
	}
	if me.Employee.Position.Name != "CEO" {
		t.Errorf("position ref from a bare string = %+v", me.Employee.Position)
	}
	if me.Employee.JobTitle() != "Chief Executive Officer" || len(me.Chain) != 1 || me.Chain[0].Level != 1 {
		t.Errorf("me = %+v", me)
	}
}

func TestRefreshOn401ThenRetry(t *testing.T) {
	var refreshes, meCalls int
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/auth/me", func(w http.ResponseWriter, r *http.Request) {
		meCalls++
		if r.Header.Get("Authorization") != "Bearer new-acc" {
			writeProblem(w, 401, "unauthorized", "token expired")
			return
		}
		writeJSON(w, 200, map[string]any{"user": map[string]string{"email": "x@y"}})
	})
	mux.HandleFunc("POST /api/v1/auth/refresh", func(w http.ResponseWriter, r *http.Request) {
		refreshes++
		var in map[string]string
		_ = json.NewDecoder(r.Body).Decode(&in)
		if in["refresh_token"] != "old-ref" {
			writeProblem(w, 401, "unauthorized", "refresh token reuse")
			return
		}
		writeJSON(w, 200, Session{AccessToken: "new-acc", RefreshToken: "new-ref", ExpiresIn: 900})
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()

	ts := &memTokens{t: Tokens{Access: "old-acc", Refresh: "old-ref"}}
	c := newClient(t, srv, ts)

	me, err := c.Me(context.Background())
	if err != nil {
		t.Fatalf("Me: %v", err)
	}
	if me.User.Email != "x@y" {
		t.Errorf("me = %+v", me)
	}
	if refreshes != 1 || meCalls != 2 {
		t.Errorf("refreshes = %d, me calls = %d; want 1 and 2", refreshes, meCalls)
	}
	if ts.t.Access != "new-acc" || ts.t.Refresh != "new-ref" || ts.stores != 1 {
		t.Errorf("rotated tokens were not stored: %+v (stores=%d)", ts.t, ts.stores)
	}
	if ts.t.ExpiresAt.Before(time.Now().Add(14 * time.Minute)) {
		t.Errorf("expiry %v not derived from expires_in", ts.t.ExpiresAt)
	}

	// Second call goes straight through with the new token.
	if _, err := c.Me(context.Background()); err != nil {
		t.Fatal(err)
	}
	if refreshes != 1 || meCalls != 3 {
		t.Errorf("after second call: refreshes = %d, me calls = %d", refreshes, meCalls)
	}
}

func TestRefreshFailureIsSessionExpired(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/auth/me", func(w http.ResponseWriter, r *http.Request) {
		writeProblem(w, 401, "unauthorized", "token expired")
	})
	mux.HandleFunc("POST /api/v1/auth/refresh", func(w http.ResponseWriter, r *http.Request) {
		writeProblem(w, 401, "unauthorized", "refresh token revoked")
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()

	c := newClient(t, srv, &memTokens{t: Tokens{Access: "a", Refresh: "r"}})
	_, err := c.Me(context.Background())
	if !errors.Is(err, ErrSessionExpired) {
		t.Fatalf("err = %v, want ErrSessionExpired", err)
	}

	// No refresh token at all: also a session problem, and no network call.
	c = newClient(t, srv, &memTokens{t: Tokens{Access: "a"}})
	if _, err := c.Me(context.Background()); !errors.Is(err, ErrSessionExpired) {
		t.Fatalf("err = %v, want ErrSessionExpired", err)
	}
}

func TestProblemDocumentsBecomeErrors(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /api/v1/support/tickets", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/problem+json")
		w.WriteHeader(422)
		_, _ = w.Write([]byte(`{"type":"about:blank","title":"Unprocessable Entity","status":422,"detail":"validation failed","code":"validation_failed","request_id":"req-9","errors":[{"field":"priority","message":"must be one of low, normal, high, urgent"}]}`))
	})
	mux.HandleFunc("POST /api/v1/comms/announcements", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(502)
		_, _ = w.Write([]byte("<html>bad gateway</html>"))
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()
	c := newClient(t, srv, &memTokens{t: Tokens{Access: "a", Refresh: "r"}})

	_, err := c.CreateTicket(context.Background(), TicketRequest{Category: "it", Priority: "asap", Subject: "s", Body: "b"})
	var apiErr *Error
	if !errors.As(err, &apiErr) {
		t.Fatalf("err = %T %v, want *Error", err, err)
	}
	if apiErr.Status != 422 || apiErr.Code != "validation_failed" || apiErr.RequestID != "req-9" || len(apiErr.Errors) != 1 {
		t.Errorf("parsed problem = %+v", apiErr)
	}
	for _, want := range []string{"422", "validation_failed", "priority: must be one of", "req-9"} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("error text %q lacks %q", err.Error(), want)
		}
	}

	_, err = c.Announce(context.Background(), Announcement{Scope: "company", Subject: "s", Body: "b"})
	if !errors.As(err, &apiErr) || apiErr.Status != 502 || apiErr.Title != "Bad Gateway" || !strings.Contains(apiErr.Detail, "bad gateway") {
		t.Errorf("non-JSON error = %+v (%v)", apiErr, err)
	}
}

func TestAllEmployeesPaginates(t *testing.T) {
	var pages []string
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/employees", func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		pages = append(pages, q.Get("page"))
		if q.Get("q") != "ada" || q.Get("department_id") != "d1" || q.Get("per_page") != "2" {
			t.Errorf("unexpected query %s", r.URL.RawQuery)
		}
		switch q.Get("page") {
		case "1":
			writeJSON(w, 200, Page[Employee]{Items: []Employee{{ID: "1", FirstName: "A"}, {ID: "2", FirstName: "B"}}, Page: 1, PerPage: 2, Total: 3})
		case "2":
			writeJSON(w, 200, Page[Employee]{Items: []Employee{{ID: "3", FirstName: "C"}}, Page: 2, PerPage: 2, Total: 3})
		default:
			writeJSON(w, 200, Page[Employee]{Page: 3, PerPage: 2, Total: 3})
		}
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()
	c := newClient(t, srv, &memTokens{t: Tokens{Access: "a", Refresh: "r"}})

	rows, total, err := c.AllEmployees(context.Background(), EmployeeFilter{Query: "ada", DepartmentID: "d1"}, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 2 || total != 3 || len(pages) != 1 {
		t.Errorf("limit 2: rows=%d total=%d pages=%v", len(rows), total, pages)
	}

	// A page size chosen by the caller is honoured, and the walker keeps going
	// until the server's total is collected.
	pages = nil
	c2 := newClient(t, srv, &memTokens{t: Tokens{Access: "a", Refresh: "r"}})
	rows, total, err = c2.AllEmployees(context.Background(), EmployeeFilter{Query: "ada", DepartmentID: "d1", PerPage: 2}, 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 3 || total != 3 || len(pages) != 2 {
		t.Errorf("page size 2: rows=%d total=%d pages=%v", len(rows), total, pages)
	}
}

func TestAllEmployeesWalksEveryPage(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/employees", func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Query().Get("page") {
		case "1":
			items := make([]Employee, 100)
			for i := range items {
				items[i].ID = "p1"
			}
			writeJSON(w, 200, Page[Employee]{Items: items, Page: 1, PerPage: 100, Total: 150})
		case "2":
			items := make([]Employee, 50)
			for i := range items {
				items[i].ID = "p2"
			}
			writeJSON(w, 200, Page[Employee]{Items: items, Page: 2, PerPage: 100, Total: 150})
		default:
			t.Errorf("page %s should not be requested", r.URL.Query().Get("page"))
			writeJSON(w, 200, Page[Employee]{Total: 150})
		}
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()
	c := newClient(t, srv, &memTokens{t: Tokens{Access: "a", Refresh: "r"}})

	rows, total, err := c.AllEmployees(context.Background(), EmployeeFilter{}, 1000)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 150 || total != 150 {
		t.Errorf("rows=%d total=%d", len(rows), total)
	}
}

func TestHealthProbesWithPrefixFallback(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/healthz", func(w http.ResponseWriter, r *http.Request) { _, _ = w.Write([]byte("ok")) })
	mux.HandleFunc("GET /api/v1/readyz", func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "database unreachable", http.StatusServiceUnavailable)
	})
	srv := httptest.NewServer(mux)
	defer srv.Close()
	c := newClient(t, srv, nil)

	probes, err := c.Health(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(probes) != 2 {
		t.Fatalf("probes = %+v", probes)
	}
	if !probes[0].OK || probes[0].Status != 200 || !strings.HasSuffix(probes[0].URL, "/api/v1/healthz") {
		t.Errorf("healthz probe = %+v", probes[0])
	}
	if probes[1].OK || probes[1].Status != 503 || !strings.Contains(probes[1].Body, "database") {
		t.Errorf("readyz probe = %+v", probes[1])
	}
}

func TestHealthReportsUnreachableServer(t *testing.T) {
	srv := httptest.NewServer(http.NotFoundHandler())
	srv.Close()
	c, err := New(srv.URL, WithHTTPClient(&http.Client{Timeout: time.Second}))
	if err != nil {
		t.Fatal(err)
	}
	probes, err := c.Health(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range probes {
		if p.OK || p.Status != 0 || p.Body == "" {
			t.Errorf("probe against a closed server = %+v", p)
		}
	}
}

func TestNewValidatesURL(t *testing.T) {
	for _, bad := range []string{"localhost:8080", "://x", "http://", "not a url"} {
		if _, err := New(bad); err == nil {
			t.Errorf("New(%q) should fail", bad)
		}
	}
	c, err := New("https://api.bowline.example/base/")
	if err != nil {
		t.Fatal(err)
	}
	if c.BaseURL() != "https://api.bowline.example/base" {
		t.Errorf("BaseURL = %s", c.BaseURL())
	}
	if got := c.endpoint("/employees", nil); got != "https://api.bowline.example/base/api/v1/employees" {
		t.Errorf("endpoint = %s", got)
	}
	d, err := New("")
	if err != nil || d.BaseURL() != DefaultBaseURL {
		t.Errorf("empty URL should use the default: %v %v", d, err)
	}
}

func TestRefUnmarshal(t *testing.T) {
	var r struct {
		A Ref `json:"a"`
		B Ref `json:"b"`
		C Ref `json:"c"`
		D Ref `json:"d"`
	}
	err := json.Unmarshal([]byte(`{"a":"Ops","b":{"id":"x","name":"Ops"},"c":null,"d":{"id":"y","first_name":"Bo","last_name":"Ops"}}`), &r)
	if err != nil {
		t.Fatal(err)
	}
	if r.A.Name != "Ops" || r.B.ID != "x" || r.B.Name != "Ops" || r.C != (Ref{}) || r.D.String() != "Bo Ops" {
		t.Errorf("refs = %+v", r)
	}
	if (Ref{ID: "z"}).String() != "z" {
		t.Error("String should fall back to the id")
	}
}
