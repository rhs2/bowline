package bowctl

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"

	"github.com/rhs2/bowline/tools/internal/creds"
	"github.com/rhs2/bowline/tools/internal/outbox"
)

// fakeAPI is the slice of the Bowline API that bowctl touches.
func fakeAPI(t *testing.T) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) { _, _ = w.Write([]byte("ok")) })
	mux.HandleFunc("GET /readyz", func(w http.ResponseWriter, r *http.Request) { _, _ = w.Write([]byte("ready")) })
	mux.HandleFunc("POST /api/v1/auth/login", func(w http.ResponseWriter, r *http.Request) {
		var in map[string]string
		_ = json.NewDecoder(r.Body).Decode(&in)
		if in["password"] != "Bowline!2026" {
			w.Header().Set("Content-Type", "application/problem+json")
			w.WriteHeader(401)
			_, _ = w.Write([]byte(`{"title":"Unauthorized","status":401,"code":"unauthorized","detail":"invalid credentials"}`))
			return
		}
		_, _ = w.Write([]byte(`{"access_token":"acc-1","refresh_token":"ref-1","expires_in":900,"must_change_password":false}`))
	})
	mux.HandleFunc("POST /api/v1/auth/logout", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(204) })
	mux.HandleFunc("GET /api/v1/auth/me", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer acc-1" {
			w.WriteHeader(401)
			_, _ = w.Write([]byte(`{"title":"Unauthorized","status":401,"code":"unauthorized"}`))
			return
		}
		_, _ = w.Write([]byte(`{"user":{"id":"u1","email":"ceo@bowline.example"},"employee":{"id":"e1","employee_no":"EMP-000001","first_name":"Ada","last_name":"Chief","title":"Chief Executive Officer","department":{"id":"d1","name":"Executive Office"}},"roles":["ceo"],"permissions":["org:read"],"chain":[{"id":"e1","name":"Ada Chief","title":"Chief Executive Officer","level":1}]}`))
	})
	mux.HandleFunc("GET /api/v1/employees", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer acc-1" {
			w.WriteHeader(401)
			return
		}
		_, _ = w.Write([]byte(`{"items":[{"id":"e2","employee_no":"EMP-000002","first_name":"Bo","last_name":"Ops","email":"bo@bowline.example","status":"active","title":"Chief Operating Officer","department":{"name":"Operations"}}],"page":1,"per_page":100,"total":1}`))
	})
	mux.HandleFunc("POST /api/v1/comms/announcements", func(w http.ResponseWriter, r *http.Request) {
		var in map[string]string
		_ = json.NewDecoder(r.Body).Decode(&in)
		if in["scope"] != "company" || in["subject"] != "Hello" || in["body"] != "All hands at 9." {
			t.Errorf("announcement body = %v", in)
		}
		w.WriteHeader(201)
		_, _ = w.Write([]byte(`{"id":"t1","kind":"announcement"}`))
	})
	mux.HandleFunc("POST /api/v1/support/tickets", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(201)
		_, _ = w.Write([]byte(`{"id":"tk1","reference":"TKT-000042","status":"open"}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv
}

type run struct {
	code   int
	stdout string
	stderr string
}

func runApp(t *testing.T, env map[string]string, stdin string, args ...string) run {
	t.Helper()
	var out, errb bytes.Buffer
	app := &App{
		Stdin:   strings.NewReader(stdin),
		Stdout:  &out,
		Stderr:  &errb,
		Getenv:  func(k string) string { return env[k] },
		Version: "test",
		OutboxStats: func(context.Context, string) (outbox.Stats, error) {
			return outbox.Stats{Pending: 3, Sending: 1, Sent: 40, Failed: 2}, nil
		},
	}
	code := app.Run(context.Background(), args)
	return run{code: code, stdout: out.String(), stderr: errb.String()}
}

func TestAppEndToEnd(t *testing.T) {
	srv := fakeAPI(t)
	credPath := filepath.Join(t.TempDir(), "credentials.json")
	env := map[string]string{"API_PUBLIC_URL": srv.URL, "BOWCTL_CREDENTIALS": credPath}

	r := runApp(t, env, "", "health")
	if r.code != 0 || !strings.Contains(r.stdout, "healthz") || !strings.Contains(r.stdout, "200 OK") {
		t.Fatalf("health: %+v", r)
	}

	r = runApp(t, env, "", "whoami")
	if r.code != ExitError || !strings.Contains(r.stderr, "not logged in") {
		t.Fatalf("whoami before login: %+v", r)
	}

	r = runApp(t, env, "", "login", "--email", "ceo@bowline.example", "--password", "wrong")
	if r.code != ExitError || !strings.Contains(r.stderr, "invalid credentials") {
		t.Fatalf("bad login: %+v", r)
	}

	r = runApp(t, env, "Bowline!2026\n", "login", "--email", "ceo@bowline.example", "--password-stdin")
	if r.code != 0 || !strings.Contains(r.stdout, "Logged in") {
		t.Fatalf("login: %+v", r)
	}
	saved, err := (creds.Store{Path: credPath}).Load()
	if err != nil || saved.AccessToken != "acc-1" || saved.RefreshToken != "ref-1" || saved.Email != "ceo@bowline.example" || saved.APIURL != srv.URL {
		t.Fatalf("saved credentials = %+v (%v)", saved, err)
	}

	r = runApp(t, env, "", "whoami")
	if r.code != 0 {
		t.Fatalf("whoami: %+v", r)
	}
	for _, want := range []string{"ceo@bowline.example", "Ada Chief", "EMP-000001", "Executive Office", "Roles:", "ceo", "1. Ada Chief, Chief Executive Officer (level 1)"} {
		if !strings.Contains(r.stdout, want) {
			t.Errorf("whoami output lacks %q:\n%s", want, r.stdout)
		}
	}

	r = runApp(t, env, "", "employees", "--q", "bo")
	if r.code != 0 {
		t.Fatalf("employees: %+v", r)
	}
	for _, want := range []string{"EMPLOYEE", "EMP-000002", "Bo Ops", "Chief Operating Officer", "Operations", "bo@bowline.example", "active", "1 employees."} {
		if !strings.Contains(r.stdout, want) {
			t.Errorf("employees output lacks %q:\n%s", want, r.stdout)
		}
	}

	r = runApp(t, env, "All hands at 9.\n", "broadcast", "--scope", "company", "--subject", "Hello", "--body-file", "-")
	if r.code != 0 || !strings.Contains(r.stdout, "Announcement posted to company (id t1)") {
		t.Fatalf("broadcast: %+v", r)
	}

	r = runApp(t, env, "", "ticket", "--category", "it", "--priority", "high", "--subject", "VPN down", "--body", "Cannot connect.")
	if r.code != 0 || !strings.Contains(r.stdout, "Ticket opened (TKT-000042, id tk1, status open)") {
		t.Fatalf("ticket: %+v", r)
	}

	// A session issued by another API is not sent elsewhere.
	other := map[string]string{"API_PUBLIC_URL": "http://elsewhere.invalid:1", "BOWCTL_CREDENTIALS": credPath}
	r = runApp(t, other, "", "whoami")
	if r.code != ExitError || !strings.Contains(r.stderr, "stored session is for") {
		t.Fatalf("whoami against another API: %+v", r)
	}

	r = runApp(t, env, "", "logout")
	if r.code != 0 || !strings.Contains(r.stdout, "Logged out") {
		t.Fatalf("logout: %+v", r)
	}
	if _, err := (creds.Store{Path: credPath}).Load(); !errors.Is(err, creds.ErrNotFound) {
		t.Errorf("credentials still present after logout: %v", err)
	}
}

func TestAppUsageAndHelp(t *testing.T) {
	r := runApp(t, nil, "", "help")
	if r.code != 0 || !strings.Contains(r.stdout, "Usage:") {
		t.Errorf("help: %+v", r)
	}
	r = runApp(t, nil, "")
	if r.code != ExitUsage || !strings.Contains(r.stderr, "missing command") {
		t.Errorf("no args: %+v", r)
	}
	r = runApp(t, nil, "", "broadcast", "--scope", "galaxy")
	if r.code != ExitUsage {
		t.Errorf("bad scope: %+v", r)
	}
	r = runApp(t, nil, "", "version")
	if r.code != 0 || strings.TrimSpace(r.stdout) != "bowctl test" {
		t.Errorf("version: %+v", r)
	}
	r = runApp(t, nil, "", "--api", "not-a-url", "health")
	if r.code != ExitUsage || !strings.Contains(r.stderr, "invalid API URL") {
		t.Errorf("bad api url: %+v", r)
	}
}

func TestAppOutboxDepth(t *testing.T) {
	r := runApp(t, nil, "", "outbox", "depth")
	if r.code != ExitUsage || !strings.Contains(r.stderr, "DATABASE_URL_NOTIFY") {
		t.Errorf("without DSN: %+v", r)
	}
	r = runApp(t, map[string]string{"DATABASE_URL_NOTIFY": "postgres://x"}, "", "outbox", "depth")
	if r.code != 0 {
		t.Fatalf("with DSN: %+v", r)
	}
	for _, want := range []string{"depth", "4", "pending 3, sending 1", "sent", "40", "failed", "2"} {
		if !strings.Contains(r.stdout, want) {
			t.Errorf("output lacks %q:\n%s", want, r.stdout)
		}
	}
}
