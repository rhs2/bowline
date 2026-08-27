package bowctl

import (
	"errors"
	"flag"
	"strings"
	"testing"
	"time"
)

func noEnv(string) string { return "" }

func TestParseCommands(t *testing.T) {
	cases := []struct {
		name  string
		args  []string
		env   map[string]string
		check func(t *testing.T, c *Command)
	}{
		{
			name: "health with defaults",
			args: []string{"health"},
			check: func(t *testing.T, c *Command) {
				if c.Name != "health" || c.API != "http://localhost:8080" || c.Timeout != 30*time.Second {
					t.Errorf("%+v", c)
				}
			},
		},
		{
			name: "api from environment",
			args: []string{"whoami"},
			env:  map[string]string{"API_PUBLIC_URL": "https://api.bowline.example"},
			check: func(t *testing.T, c *Command) {
				if c.API != "https://api.bowline.example" {
					t.Errorf("API = %s", c.API)
				}
			},
		},
		{
			name: "global flags before the command",
			args: []string{"--api", "http://x:1", "--timeout", "5s", "health"},
			check: func(t *testing.T, c *Command) {
				if c.API != "http://x:1" || c.Timeout != 5*time.Second {
					t.Errorf("%+v", c)
				}
			},
		},
		{
			name: "global flags after the command",
			args: []string{"login", "--email", "a@b", "--password", "pw", "--api", "http://y:2"},
			check: func(t *testing.T, c *Command) {
				if c.API != "http://y:2" || c.Login.Email != "a@b" || c.Login.Password != "pw" {
					t.Errorf("%+v", c)
				}
			},
		},
		{
			name: "login with password from stdin",
			args: []string{"login", "-email=a@b", "-password-stdin"},
			check: func(t *testing.T, c *Command) {
				if !c.Login.PasswordStdin || c.Login.Email != "a@b" {
					t.Errorf("%+v", c.Login)
				}
			},
		},
		{
			name: "broadcast to a department",
			args: []string{"broadcast", "--scope", "department", "--ref", "d1", "--subject", "Hi", "--body", "Text"},
			check: func(t *testing.T, c *Command) {
				b := c.Broadcast
				if b.Scope != "department" || b.Ref != "d1" || b.Subject != "Hi" || b.Body != "Text" {
					t.Errorf("%+v", b)
				}
			},
		},
		{
			name: "ticket with body file",
			args: []string{"ticket", "--category", "it", "--priority", "high", "--subject", "VPN", "--body-file", "-"},
			check: func(t *testing.T, c *Command) {
				tk := c.Ticket
				if tk.Category != "it" || tk.Priority != "high" || tk.BodyFile != "-" || tk.Body != "" {
					t.Errorf("%+v", tk)
				}
			},
		},
		{
			name: "employees filters",
			args: []string{"employees", "--q", "ada", "--department", "d1", "--limit", "5"},
			check: func(t *testing.T, c *Command) {
				e := c.Employees
				if e.Query != "ada" || e.Department != "d1" || e.Limit != 5 {
					t.Errorf("%+v", e)
				}
			},
		},
		{
			name: "employees default limit",
			args: []string{"employees"},
			check: func(t *testing.T, c *Command) {
				if c.Employees.Limit != 100 {
					t.Errorf("limit = %d", c.Employees.Limit)
				}
			},
		},
		{
			name: "outbox depth",
			args: []string{"outbox", "depth"},
			check: func(t *testing.T, c *Command) {
				if c.Name != "outbox" || c.Outbox.Sub != "depth" {
					t.Errorf("%+v", c)
				}
			},
		},
		{
			name: "version",
			args: []string{"version"},
			check: func(t *testing.T, c *Command) {
				if c.Name != "version" {
					t.Errorf("%+v", c)
				}
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			getenv := noEnv
			if tc.env != nil {
				getenv = func(k string) string { return tc.env[k] }
			}
			c, err := Parse(tc.args, getenv)
			if err != nil {
				t.Fatalf("Parse(%v): %v", tc.args, err)
			}
			tc.check(t, c)
		})
	}
}

func TestParseUsageErrors(t *testing.T) {
	cases := []struct {
		args []string
		want string
	}{
		{nil, "missing command"},
		{[]string{"frobnicate"}, `unknown command "frobnicate"`},
		{[]string{"health", "extra"}, "unexpected argument"},
		{[]string{"login"}, "--email is required"},
		{[]string{"login", "--email", "a@b"}, "--password or --password-stdin"},
		{[]string{"login", "--email", "a@b", "--password", "x", "--password-stdin"}, "mutually exclusive"},
		{[]string{"broadcast", "--scope", "planet", "--subject", "s", "--body", "b"}, "--scope must be"},
		{[]string{"broadcast", "--scope", "company", "--ref", "x", "--subject", "s", "--body", "b"}, "--ref is only used"},
		{[]string{"broadcast", "--scope", "company", "--body", "b"}, "--subject is required"},
		{[]string{"broadcast", "--scope", "company", "--subject", "s"}, "--body or --body-file"},
		{[]string{"broadcast", "--scope", "company", "--subject", "s", "--body", "b", "--body-file", "f"}, "mutually exclusive"},
		{[]string{"ticket", "--priority", "high", "--subject", "s", "--body", "b"}, "--category is required"},
		{[]string{"ticket", "--category", "it", "--subject", "s", "--body", "b"}, "--priority is required"},
		{[]string{"employees", "--limit", "0"}, "--limit must be positive"},
		{[]string{"employees", "--limit", "many"}, "invalid value"},
		{[]string{"outbox"}, "needs a subcommand"},
		{[]string{"outbox", "purge"}, "unknown outbox subcommand"},
		{[]string{"--timeout", "0s", "health"}, "--timeout must be positive"},
		{[]string{"--bogus", "health"}, "flag provided but not defined"},
	}
	for _, tc := range cases {
		_, err := Parse(tc.args, noEnv)
		if err == nil {
			t.Errorf("Parse(%v) succeeded, want error containing %q", tc.args, tc.want)
			continue
		}
		if !IsUsageError(err) {
			t.Errorf("Parse(%v): %v is not a UsageError", tc.args, err)
		}
		if !strings.Contains(err.Error(), tc.want) {
			t.Errorf("Parse(%v) = %q, want it to contain %q", tc.args, err, tc.want)
		}
	}
}

func TestParseHelp(t *testing.T) {
	for _, args := range [][]string{{"help"}, {"-h"}, {"--help"}, {"login", "--help"}} {
		_, err := Parse(args, noEnv)
		if !errors.Is(err, flag.ErrHelp) {
			t.Errorf("Parse(%v) = %v, want flag.ErrHelp", args, err)
		}
	}
}
