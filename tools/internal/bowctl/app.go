package bowctl

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/rhs2/bowline/tools/internal/apiclient"
	"github.com/rhs2/bowline/tools/internal/creds"
	"github.com/rhs2/bowline/tools/internal/outbox"
)

// App wires the commands to their inputs and outputs. Every dependency is a
// field so tests can drive it without a terminal or a database.
type App struct {
	Stdin   io.Reader
	Stdout  io.Writer
	Stderr  io.Writer
	Getenv  func(string) string
	Version string
	// OutboxStats counts the notifications table. Nil means a direct
	// connection with pgx; tests inject a fake.
	OutboxStats func(ctx context.Context, dsn string) (outbox.Stats, error)
}

// Exit codes returned by Run.
const (
	ExitOK    = 0
	ExitError = 1
	ExitUsage = 2
)

// Run parses args and executes the command, returning the process exit code.
func (a *App) Run(ctx context.Context, args []string) int {
	a.defaults()
	cmd, err := Parse(args, a.Getenv)
	if errors.Is(err, flag.ErrHelp) {
		fmt.Fprint(a.Stdout, Usage)
		return ExitOK
	}
	if err != nil {
		return a.fail(err)
	}
	if err := a.dispatch(ctx, cmd); err != nil {
		return a.fail(err)
	}
	return ExitOK
}

func (a *App) defaults() {
	if a.Stdin == nil {
		a.Stdin = strings.NewReader("")
	}
	if a.Stdout == nil {
		a.Stdout = io.Discard
	}
	if a.Stderr == nil {
		a.Stderr = io.Discard
	}
	if a.Getenv == nil {
		a.Getenv = os.Getenv
	}
	if a.Version == "" {
		a.Version = "dev"
	}
	if a.OutboxStats == nil {
		a.OutboxStats = pgOutboxStats
	}
}

func (a *App) fail(err error) int {
	if IsUsageError(err) {
		fmt.Fprintf(a.Stderr, "bowctl: %v\nRun 'bowctl help' for usage.\n", err)
		return ExitUsage
	}
	fmt.Fprintf(a.Stderr, "bowctl: %v\n", err)
	return ExitError
}

func (a *App) dispatch(ctx context.Context, cmd *Command) error {
	switch cmd.Name {
	case "version":
		fmt.Fprintf(a.Stdout, "bowctl %s\n", a.Version)
		return nil
	case "health":
		return a.health(ctx, cmd)
	case "login":
		return a.login(ctx, cmd)
	case "logout":
		return a.logout(ctx, cmd)
	case "whoami":
		return a.whoami(ctx, cmd)
	case "broadcast":
		return a.broadcast(ctx, cmd)
	case "ticket":
		return a.ticket(ctx, cmd)
	case "employees":
		return a.employees(ctx, cmd)
	case "outbox":
		return a.outboxDepth(ctx, cmd)
	}
	return usageErr("unknown command %q", cmd.Name)
}

// credentials resolves the session file for this invocation.
func (a *App) credentials() (creds.Store, error) {
	path, err := creds.DefaultPath(a.Getenv)
	if err != nil {
		return creds.Store{}, err
	}
	return creds.Store{Path: path}, nil
}

// client builds an API client; authed attaches the stored session.
func (a *App) client(cmd *Command, authed bool) (*apiclient.Client, creds.Store, error) {
	store, err := a.credentials()
	if err != nil {
		return nil, store, err
	}
	opts := []apiclient.Option{
		apiclient.WithHTTPClient(&http.Client{Timeout: cmd.Timeout}),
		apiclient.WithUserAgent("bowctl/" + a.Version),
	}
	if authed {
		opts = append(opts, apiclient.WithTokenSource(&tokenFile{store: store, api: cmd.API}))
	}
	c, err := apiclient.New(cmd.API, opts...)
	if err != nil {
		return nil, store, usageErr("%v", err)
	}
	return c, store, nil
}

// tokenFile adapts the credentials file to apiclient.TokenSource. Tokens are
// only handed out for the API they were issued by.
type tokenFile struct {
	store creds.Store
	api   string
}

func (t *tokenFile) Tokens(context.Context) (apiclient.Tokens, error) {
	c, err := t.store.Load()
	if errors.Is(err, creds.ErrNotFound) {
		return apiclient.Tokens{}, apiclient.ErrNotLoggedIn
	}
	if err != nil {
		return apiclient.Tokens{}, err
	}
	if c.APIURL != "" && !sameAPI(c.APIURL, t.api) {
		return apiclient.Tokens{}, fmt.Errorf("stored session is for %s, not %s; run: bowctl --api %s login", c.APIURL, t.api, t.api)
	}
	return apiclient.Tokens{Access: c.AccessToken, Refresh: c.RefreshToken, ExpiresAt: c.ExpiresAt}, nil
}

func (t *tokenFile) Store(_ context.Context, tok apiclient.Tokens) error {
	c, err := t.store.Load()
	if err != nil && !errors.Is(err, creds.ErrNotFound) {
		return err
	}
	if c.APIURL == "" {
		c.APIURL = t.api
	}
	c.AccessToken, c.RefreshToken, c.ExpiresAt = tok.Access, tok.Refresh, tok.ExpiresAt
	return t.store.Save(c)
}

func sameAPI(a, b string) bool {
	norm := func(s string) string { return strings.ToLower(strings.TrimRight(strings.TrimSpace(s), "/")) }
	return norm(a) == norm(b)
}

// pgOutboxStats opens a short-lived pool with the notify role and counts rows.
func pgOutboxStats(ctx context.Context, dsn string) (outbox.Stats, error) {
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return outbox.Stats{}, fmt.Errorf("DATABASE_URL_NOTIFY: %w", err)
	}
	cfg.MaxConns = 1
	cfg.ConnConfig.RuntimeParams["application_name"] = "bowctl"
	if cfg.ConnConfig.ConnectTimeout == 0 {
		cfg.ConnConfig.ConnectTimeout = 10 * time.Second
	}
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return outbox.Stats{}, fmt.Errorf("connect: %w", err)
	}
	defer pool.Close()
	return outbox.NewPGStore(pool).Stats(ctx)
}
