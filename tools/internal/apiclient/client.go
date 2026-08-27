// Package apiclient is a small client for the Bowline API, used by bowctl.
// It speaks the contract in docs/API.md: JSON bodies, bearer tokens, RFC 7807
// problem documents, and rotating refresh tokens.
package apiclient

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	// DefaultBaseURL is used when no API URL is configured.
	DefaultBaseURL = "http://localhost:8080"
	apiPrefix      = "/api/v1"
	maxBody        = 8 << 20
	// defaultPageSize is the page size the list walkers request when the caller
	// does not choose one.
	defaultPageSize = 100
)

// Tokens is an access and refresh token pair.
type Tokens struct {
	Access    string
	Refresh   string
	ExpiresAt time.Time
}

// TokenSource supplies the bearer token for authenticated calls and persists
// the rotated pair after a refresh.
type TokenSource interface {
	Tokens(ctx context.Context) (Tokens, error)
	Store(ctx context.Context, t Tokens) error
}

// Sentinel errors callers can test with errors.Is.
var (
	ErrNotLoggedIn    = errors.New("not logged in; run bowctl login")
	ErrSessionExpired = errors.New("session expired; run bowctl login again")
)

// Client calls one Bowline API.
type Client struct {
	base      *url.URL
	http      *http.Client
	tokens    TokenSource
	userAgent string
	refreshMu sync.Mutex
}

// Option customises a Client.
type Option func(*Client)

// WithHTTPClient replaces the default http.Client (30s timeout).
func WithHTTPClient(hc *http.Client) Option { return func(c *Client) { c.http = hc } }

// WithTokenSource enables authenticated calls.
func WithTokenSource(ts TokenSource) Option { return func(c *Client) { c.tokens = ts } }

// WithUserAgent sets the User-Agent header.
func WithUserAgent(ua string) Option { return func(c *Client) { c.userAgent = ua } }

// New validates baseURL (scheme and host required) and returns a Client.
func New(baseURL string, opts ...Option) (*Client, error) {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	u, err := url.Parse(baseURL)
	if err != nil || u.Scheme == "" || u.Host == "" {
		return nil, fmt.Errorf("invalid API URL %q (want e.g. http://localhost:8080)", baseURL)
	}
	u.Path = strings.TrimRight(u.Path, "/")
	u.RawQuery, u.Fragment = "", ""
	c := &Client{
		base:      u,
		http:      &http.Client{Timeout: 30 * time.Second},
		userAgent: "bowctl",
	}
	for _, o := range opts {
		o(c)
	}
	return c, nil
}

// BaseURL returns the normalised base URL.
func (c *Client) BaseURL() string { return c.base.String() }

// Error is an RFC 7807 problem document returned by the API.
type Error struct {
	Status    int          `json:"status"`
	Code      string       `json:"code"`
	Title     string       `json:"title"`
	Detail    string       `json:"detail"`
	RequestID string       `json:"request_id"`
	Errors    []FieldError `json:"errors,omitempty"`
}

// FieldError is one entry of a validation_failed problem.
type FieldError struct {
	Field   string `json:"field"`
	Message string `json:"message"`
}

func (e *Error) Error() string {
	var b strings.Builder
	fmt.Fprintf(&b, "api: %d", e.Status)
	switch {
	case e.Code != "":
		b.WriteString(" " + e.Code)
	case e.Title != "":
		b.WriteString(" " + e.Title)
	}
	if e.Detail != "" {
		b.WriteString(": " + e.Detail)
	}
	for _, fe := range e.Errors {
		fmt.Fprintf(&b, "; %s: %s", fe.Field, fe.Message)
	}
	if e.RequestID != "" {
		fmt.Fprintf(&b, " (request %s)", e.RequestID)
	}
	return b.String()
}

// Session is the response of /auth/login and /auth/refresh.
type Session struct {
	AccessToken        string `json:"access_token"`
	RefreshToken       string `json:"refresh_token"`
	ExpiresIn          int    `json:"expires_in"`
	MustChangePassword bool   `json:"must_change_password"`
}

// Tokens converts the session into a token pair with an absolute expiry.
func (s Session) Tokens(now time.Time) Tokens {
	return Tokens{
		Access:    s.AccessToken,
		Refresh:   s.RefreshToken,
		ExpiresAt: now.Add(time.Duration(s.ExpiresIn) * time.Second),
	}
}

// Ref is a related entity the API may render as a bare string, or as an object
// carrying an id and a name, title or code. Both forms decode into Ref.
type Ref struct {
	ID   string
	Name string
}

// UnmarshalJSON implements json.Unmarshaler.
func (r *Ref) UnmarshalJSON(b []byte) error {
	b = bytes.TrimSpace(b)
	if len(b) == 0 || bytes.Equal(b, []byte("null")) {
		*r = Ref{}
		return nil
	}
	if b[0] == '"' {
		var s string
		if err := json.Unmarshal(b, &s); err != nil {
			return err
		}
		*r = Ref{Name: s}
		return nil
	}
	var obj struct {
		ID        string `json:"id"`
		Name      string `json:"name"`
		Title     string `json:"title"`
		Code      string `json:"code"`
		FirstName string `json:"first_name"`
		LastName  string `json:"last_name"`
	}
	if err := json.Unmarshal(b, &obj); err != nil {
		return err
	}
	name := obj.Name
	if name == "" {
		name = strings.TrimSpace(obj.FirstName + " " + obj.LastName)
	}
	if name == "" {
		name = obj.Title
	}
	if name == "" {
		name = obj.Code
	}
	*r = Ref{ID: obj.ID, Name: name}
	return nil
}

func (r Ref) String() string {
	if r.Name != "" {
		return r.Name
	}
	return r.ID
}

// Employee is a row of GET /employees or the employee block of /auth/me.
type Employee struct {
	ID         string `json:"id"`
	EmployeeNo string `json:"employee_no"`
	FirstName  string `json:"first_name"`
	LastName   string `json:"last_name"`
	Email      string `json:"email"`
	Status     string `json:"status"`
	Title      string `json:"title"`
	Level      int    `json:"level"`
	Site       string `json:"site"`
	Position   Ref    `json:"position"`
	Department Ref    `json:"department"`
	Manager    Ref    `json:"manager"`
}

// FullName joins first and last name.
func (e Employee) FullName() string {
	return strings.TrimSpace(e.FirstName + " " + e.LastName)
}

// JobTitle prefers the flat title field and falls back to the position.
func (e Employee) JobTitle() string {
	if e.Title != "" {
		return e.Title
	}
	return e.Position.Name
}

// Me is the response of GET /auth/me.
type Me struct {
	User struct {
		ID                 string `json:"id"`
		Email              string `json:"email"`
		MustChangePassword bool   `json:"must_change_password"`
	} `json:"user"`
	Employee    Employee    `json:"employee"`
	Roles       []string    `json:"roles"`
	Permissions []string    `json:"permissions"`
	Chain       []ChainLink `json:"chain"`
}

// ChainLink is one step of the chain of command, from the caller up to the CEO.
type ChainLink struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Title string `json:"title"`
	Level int    `json:"level"`
}

// Page is the list envelope.
type Page[T any] struct {
	Items   []T `json:"items"`
	Page    int `json:"page"`
	PerPage int `json:"per_page"`
	Total   int `json:"total"`
}

// Created is the subset of a create response that bowctl reports.
type Created struct {
	ID        string `json:"id"`
	Reference string `json:"reference"`
	ThreadID  string `json:"thread_id"`
	Status    string `json:"status"`
}

// Announcement is the body of POST /comms/announcements.
type Announcement struct {
	Scope   string `json:"scope"`
	Ref     string `json:"ref,omitempty"`
	Subject string `json:"subject"`
	Body    string `json:"body"`
}

// TicketRequest is the body of POST /support/tickets.
type TicketRequest struct {
	Category string `json:"category"`
	Priority string `json:"priority"`
	Subject  string `json:"subject"`
	Body     string `json:"body"`
}

// EmployeeFilter narrows GET /employees.
type EmployeeFilter struct {
	Query        string
	DepartmentID string
	Status       string
	Page         int
	PerPage      int
}

// Probe is the result of one health endpoint.
type Probe struct {
	Name    string
	URL     string
	Status  int
	OK      bool
	Latency time.Duration
	Body    string
}

// Login exchanges credentials for a session. It does not store anything.
func (c *Client) Login(ctx context.Context, email, password string) (Session, error) {
	var s Session
	in := map[string]string{"email": email, "password": password}
	err := c.do(ctx, http.MethodPost, "/auth/login", nil, in, &s, false)
	return s, err
}

// Refresh rotates the refresh token. It does not store anything.
func (c *Client) Refresh(ctx context.Context, refreshToken string) (Session, error) {
	var s Session
	in := map[string]string{"refresh_token": refreshToken}
	err := c.do(ctx, http.MethodPost, "/auth/refresh", nil, in, &s, false)
	return s, err
}

// Logout revokes the refresh token family.
func (c *Client) Logout(ctx context.Context, refreshToken string) error {
	in := map[string]string{"refresh_token": refreshToken}
	return c.do(ctx, http.MethodPost, "/auth/logout", nil, in, nil, false)
}

// Me returns the caller's identity, roles, permissions and chain of command.
func (c *Client) Me(ctx context.Context) (Me, error) {
	var m Me
	err := c.do(ctx, http.MethodGet, "/auth/me", nil, nil, &m, true)
	return m, err
}

// Announce posts an announcement to a company, department or subtree audience.
func (c *Client) Announce(ctx context.Context, a Announcement) (Created, error) {
	var out Created
	err := c.do(ctx, http.MethodPost, "/comms/announcements", nil, a, &out, true)
	return out, err
}

// CreateTicket opens a support ticket.
func (c *Client) CreateTicket(ctx context.Context, t TicketRequest) (Created, error) {
	var out Created
	err := c.do(ctx, http.MethodPost, "/support/tickets", nil, t, &out, true)
	return out, err
}

// Employees returns one page of employees visible to the caller.
func (c *Client) Employees(ctx context.Context, f EmployeeFilter) (Page[Employee], error) {
	q := url.Values{}
	if f.Query != "" {
		q.Set("q", f.Query)
	}
	if f.DepartmentID != "" {
		q.Set("department_id", f.DepartmentID)
	}
	if f.Status != "" {
		q.Set("status", f.Status)
	}
	if f.Page > 0 {
		q.Set("page", strconv.Itoa(f.Page))
	}
	if f.PerPage > 0 {
		q.Set("per_page", strconv.Itoa(f.PerPage))
	}
	var p Page[Employee]
	err := c.do(ctx, http.MethodGet, "/employees", q, nil, &p, true)
	return p, err
}

// AllEmployees walks the pages until limit rows are collected or the list ends.
// It returns the rows and the server's total. A PerPage set on the filter is
// honoured as the page size; otherwise the page size is the default 100, capped
// by the limit so a small request does not ask for a large page.
func (c *Client) AllEmployees(ctx context.Context, f EmployeeFilter, limit int) ([]Employee, int, error) {
	if limit <= 0 {
		limit = 100
	}
	var out []Employee
	f.Page = 1
	if f.PerPage <= 0 {
		f.PerPage = defaultPageSize
		if limit < f.PerPage {
			f.PerPage = limit
		}
	}
	for {
		p, err := c.Employees(ctx, f)
		if err != nil {
			return out, 0, err
		}
		out = append(out, p.Items...)
		if len(out) >= limit || len(p.Items) == 0 || len(out) >= p.Total {
			if len(out) > limit {
				out = out[:limit]
			}
			return out, p.Total, nil
		}
		f.Page++
	}
}

// Health probes /healthz and /readyz. Both live at the server root; if the
// root answers 404 the /api/v1 prefix is tried. Probe failures are reported
// in the result, not as an error; only a malformed URL is an error.
func (c *Client) Health(ctx context.Context) ([]Probe, error) {
	var probes []Probe
	for _, name := range []string{"healthz", "readyz"} {
		p := c.probe(ctx, name, c.base.String()+"/"+name)
		if p.Status == http.StatusNotFound {
			if alt := c.probe(ctx, name, c.base.String()+apiPrefix+"/"+name); alt.Status != http.StatusNotFound {
				p = alt
			}
		}
		probes = append(probes, p)
	}
	return probes, nil
}

func (c *Client) probe(ctx context.Context, name, u string) Probe {
	p := Probe{Name: name, URL: u}
	start := time.Now()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
	if err != nil {
		p.Body = err.Error()
		return p
	}
	req.Header.Set("User-Agent", c.userAgent)
	resp, err := c.http.Do(req)
	p.Latency = time.Since(start)
	if err != nil {
		p.Body = err.Error()
		return p
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
	p.Status = resp.StatusCode
	p.OK = resp.StatusCode >= 200 && resp.StatusCode < 300
	p.Body = strings.TrimSpace(string(raw))
	return p
}

func (c *Client) endpoint(path string, q url.Values) string {
	u := *c.base
	u.Path = c.base.Path + apiPrefix + path
	u.RawQuery = q.Encode()
	return u.String()
}

// do performs one API call. For authenticated calls a 401 triggers exactly one
// refresh of the token pair followed by one retry.
func (c *Client) do(ctx context.Context, method, path string, q url.Values, in, out any, authed bool) error {
	var payload []byte
	if in != nil {
		var err error
		if payload, err = json.Marshal(in); err != nil {
			return fmt.Errorf("encode request: %w", err)
		}
	}
	var access string
	if authed {
		t, err := c.currentTokens(ctx)
		if err != nil {
			return err
		}
		access = t.Access
	}
	target := c.endpoint(path, q)
	resp, err := c.send(ctx, method, target, payload, access)
	if err != nil {
		return err
	}
	if authed && resp.StatusCode == http.StatusUnauthorized {
		drain(resp)
		t, err := c.refresh(ctx)
		if err != nil {
			return err
		}
		if resp, err = c.send(ctx, method, target, payload, t.Access); err != nil {
			return err
		}
	}
	defer drain(resp)
	if resp.StatusCode >= 400 {
		return problem(resp)
	}
	if out == nil || resp.StatusCode == http.StatusNoContent {
		return nil
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, maxBody)).Decode(out); err != nil {
		return fmt.Errorf("decode %s %s response: %w", method, path, err)
	}
	return nil
}

func (c *Client) currentTokens(ctx context.Context) (Tokens, error) {
	if c.tokens == nil {
		return Tokens{}, ErrNotLoggedIn
	}
	t, err := c.tokens.Tokens(ctx)
	if err != nil {
		return Tokens{}, err
	}
	if t.Access == "" && t.Refresh == "" {
		return Tokens{}, ErrNotLoggedIn
	}
	if t.Access == "" {
		return c.refresh(ctx)
	}
	return t, nil
}

func (c *Client) refresh(ctx context.Context) (Tokens, error) {
	c.refreshMu.Lock()
	defer c.refreshMu.Unlock()
	if c.tokens == nil {
		return Tokens{}, ErrNotLoggedIn
	}
	cur, err := c.tokens.Tokens(ctx)
	if err != nil {
		return Tokens{}, err
	}
	if cur.Refresh == "" {
		return Tokens{}, ErrSessionExpired
	}
	sess, err := c.Refresh(ctx, cur.Refresh)
	if err != nil {
		var apiErr *Error
		if errors.As(err, &apiErr) && (apiErr.Status == http.StatusUnauthorized || apiErr.Status == http.StatusForbidden) {
			return Tokens{}, fmt.Errorf("%w (%v)", ErrSessionExpired, err)
		}
		return Tokens{}, fmt.Errorf("refresh access token: %w", err)
	}
	t := sess.Tokens(time.Now())
	if err := c.tokens.Store(ctx, t); err != nil {
		return Tokens{}, fmt.Errorf("store refreshed tokens: %w", err)
	}
	return t, nil
}

func (c *Client) send(ctx context.Context, method, target string, payload []byte, access string) (*http.Response, error) {
	var body io.Reader
	if payload != nil {
		body = bytes.NewReader(payload)
	}
	req, err := http.NewRequestWithContext(ctx, method, target, body)
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Accept", "application/json, application/problem+json")
	req.Header.Set("User-Agent", c.userAgent)
	if payload != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if access != "" {
		req.Header.Set("Authorization", "Bearer "+access)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("%s %s: %w", method, target, err)
	}
	return resp, nil
}

func problem(resp *http.Response) error {
	raw, _ := io.ReadAll(io.LimitReader(resp.Body, 64<<10))
	e := &Error{}
	if err := json.Unmarshal(raw, e); err != nil || (e.Code == "" && e.Title == "" && e.Detail == "") {
		e = &Error{Title: http.StatusText(resp.StatusCode)}
		if s := strings.TrimSpace(string(raw)); s != "" {
			if len(s) > 200 {
				s = s[:200] + "..."
			}
			e.Detail = s
		}
	}
	e.Status = resp.StatusCode
	return e
}

func drain(resp *http.Response) {
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, maxBody))
	_ = resp.Body.Close()
}
