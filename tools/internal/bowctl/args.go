// Package bowctl implements the operator command line: argument parsing, the
// commands, and their output. cmd/bowctl is a thin wrapper around App.
package bowctl

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/rhs2/bowline/tools/internal/apiclient"
)

// Usage is printed by `bowctl help` and on usage errors.
const Usage = `bowctl is the Bowline operator command line.

Usage:
  bowctl [--api URL] [--timeout DURATION] <command> [flags]

Commands:
  health                      Probe /healthz and /readyz
  login                       Sign in and store the session
                                --email EMAIL (--password PASSWORD | --password-stdin)
  logout                      Revoke the session and delete the stored tokens
  whoami                      Show the signed-in user, roles and chain of command
  broadcast                   Post an announcement
                                --scope company|department|subtree [--ref ID]
                                --subject TEXT (--body TEXT | --body-file PATH)
  ticket                      Open a support ticket
                                --category NAME --priority LEVEL
                                --subject TEXT (--body TEXT | --body-file PATH)
  employees                   List employees visible to you
                                [--q TEXT] [--department ID] [--limit N]
  outbox depth                Count notifications waiting for delivery
                                (reads the database via DATABASE_URL_NOTIFY)
  version                     Print the build version
  help                        Print this text

Global flags:
  --api URL          API base URL (default: $API_PUBLIC_URL or http://localhost:8080)
  --timeout DURATION Request timeout, for example 30s or 2m (default 30s)

Environment:
  API_PUBLIC_URL       Default for --api
  BOWCTL_CREDENTIALS   Session file (default $HOME/.config/bowline/credentials.json)
  DATABASE_URL_NOTIFY  Connection string used by "outbox depth"

Exit codes: 0 success, 1 the command failed, 2 usage error.
`

// Command is a parsed invocation.
type Command struct {
	Name    string
	API     string
	Timeout time.Duration

	Login     LoginArgs
	Broadcast BroadcastArgs
	Ticket    TicketArgs
	Employees EmployeesArgs
	Outbox    OutboxArgs
}

// LoginArgs are the flags of `bowctl login`.
type LoginArgs struct {
	Email         string
	Password      string
	PasswordStdin bool
}

// BroadcastArgs are the flags of `bowctl broadcast`.
type BroadcastArgs struct {
	Scope    string
	Ref      string
	Subject  string
	Body     string
	BodyFile string
}

// TicketArgs are the flags of `bowctl ticket`.
type TicketArgs struct {
	Category string
	Priority string
	Subject  string
	Body     string
	BodyFile string
}

// EmployeesArgs are the flags of `bowctl employees`.
type EmployeesArgs struct {
	Query      string
	Department string
	Limit      int
}

// OutboxArgs hold the `bowctl outbox` subcommand.
type OutboxArgs struct {
	Sub string
}

// UsageError is a problem with the command line; the process exits with 2.
type UsageError struct {
	Msg string
}

func (e *UsageError) Error() string { return e.Msg }

func usageErr(format string, args ...any) error {
	return &UsageError{Msg: fmt.Sprintf(format, args...)}
}

// IsUsageError reports whether err is a UsageError.
func IsUsageError(err error) bool {
	var ue *UsageError
	return errors.As(err, &ue)
}

var scopes = map[string]bool{"company": true, "department": true, "subtree": true}

const defaultTimeout = 30 * time.Second

// Parse turns the arguments after the program name into a Command. It returns
// flag.ErrHelp when help was requested and a *UsageError for anything wrong.
func Parse(args []string, getenv func(string) string) (*Command, error) {
	cmd := &Command{API: apiclient.DefaultBaseURL, Timeout: defaultTimeout}
	if v := getenv("API_PUBLIC_URL"); v != "" {
		cmd.API = v
	}

	global := newFlagSet("bowctl")
	addGlobal(global, cmd)
	if err := global.Parse(args); err != nil {
		return nil, flagErr(err)
	}
	rest := global.Args()
	if len(rest) == 0 {
		return nil, usageErr("missing command")
	}
	cmd.Name, rest = rest[0], rest[1:]

	fs := newFlagSet(cmd.Name)
	addGlobal(fs, cmd)
	switch cmd.Name {
	case "help":
		return nil, flag.ErrHelp
	case "health", "logout", "whoami", "version":
		// no flags of their own
	case "login":
		fs.StringVar(&cmd.Login.Email, "email", "", "")
		fs.StringVar(&cmd.Login.Password, "password", "", "")
		fs.BoolVar(&cmd.Login.PasswordStdin, "password-stdin", false, "")
	case "broadcast":
		fs.StringVar(&cmd.Broadcast.Scope, "scope", "", "")
		fs.StringVar(&cmd.Broadcast.Ref, "ref", "", "")
		fs.StringVar(&cmd.Broadcast.Subject, "subject", "", "")
		fs.StringVar(&cmd.Broadcast.Body, "body", "", "")
		fs.StringVar(&cmd.Broadcast.BodyFile, "body-file", "", "")
	case "ticket":
		fs.StringVar(&cmd.Ticket.Category, "category", "", "")
		fs.StringVar(&cmd.Ticket.Priority, "priority", "", "")
		fs.StringVar(&cmd.Ticket.Subject, "subject", "", "")
		fs.StringVar(&cmd.Ticket.Body, "body", "", "")
		fs.StringVar(&cmd.Ticket.BodyFile, "body-file", "", "")
	case "employees":
		fs.StringVar(&cmd.Employees.Query, "q", "", "")
		fs.StringVar(&cmd.Employees.Department, "department", "", "")
		fs.IntVar(&cmd.Employees.Limit, "limit", 100, "")
	case "outbox":
		if len(rest) == 0 {
			return nil, usageErr("outbox needs a subcommand: bowctl outbox depth")
		}
		cmd.Outbox.Sub, rest = rest[0], rest[1:]
		if cmd.Outbox.Sub != "depth" {
			return nil, usageErr("unknown outbox subcommand %q (try: depth)", cmd.Outbox.Sub)
		}
	default:
		return nil, usageErr("unknown command %q", cmd.Name)
	}

	if err := fs.Parse(rest); err != nil {
		return nil, flagErr(err)
	}
	if extra := fs.Args(); len(extra) > 0 {
		return nil, usageErr("%s: unexpected argument %q", cmd.Name, extra[0])
	}
	if err := cmd.validate(); err != nil {
		return nil, err
	}
	return cmd, nil
}

func (cmd *Command) validate() error {
	if strings.TrimSpace(cmd.API) == "" {
		return usageErr("--api must not be empty")
	}
	if cmd.Timeout <= 0 {
		return usageErr("--timeout must be positive")
	}
	switch cmd.Name {
	case "login":
		a := cmd.Login
		if a.Email == "" {
			return usageErr("login: --email is required")
		}
		if a.Password == "" && !a.PasswordStdin {
			return usageErr("login: provide --password or --password-stdin")
		}
		if a.Password != "" && a.PasswordStdin {
			return usageErr("login: --password and --password-stdin are mutually exclusive")
		}
	case "broadcast":
		a := cmd.Broadcast
		if !scopes[a.Scope] {
			return usageErr("broadcast: --scope must be company, department or subtree (got %q)", a.Scope)
		}
		if a.Scope == "company" && a.Ref != "" {
			return usageErr("broadcast: --ref is only used with --scope department or subtree")
		}
		if err := checkSubjectBody("broadcast", a.Subject, a.Body, a.BodyFile); err != nil {
			return err
		}
	case "ticket":
		a := cmd.Ticket
		if a.Category == "" {
			return usageErr("ticket: --category is required")
		}
		if a.Priority == "" {
			return usageErr("ticket: --priority is required")
		}
		if err := checkSubjectBody("ticket", a.Subject, a.Body, a.BodyFile); err != nil {
			return err
		}
	case "employees":
		if cmd.Employees.Limit <= 0 {
			return usageErr("employees: --limit must be positive")
		}
	}
	return nil
}

func checkSubjectBody(name, subject, body, bodyFile string) error {
	if strings.TrimSpace(subject) == "" {
		return usageErr("%s: --subject is required", name)
	}
	if body == "" && bodyFile == "" {
		return usageErr("%s: provide --body or --body-file (use --body-file - for stdin)", name)
	}
	if body != "" && bodyFile != "" {
		return usageErr("%s: --body and --body-file are mutually exclusive", name)
	}
	return nil
}

func newFlagSet(name string) *flag.FlagSet {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	fs.Usage = func() {}
	return fs
}

// addGlobal registers the global flags on a FlagSet so they are accepted both
// before and after the command name.
func addGlobal(fs *flag.FlagSet, cmd *Command) {
	fs.StringVar(&cmd.API, "api", cmd.API, "")
	fs.DurationVar(&cmd.Timeout, "timeout", cmd.Timeout, "")
}

func flagErr(err error) error {
	if errors.Is(err, flag.ErrHelp) {
		return flag.ErrHelp
	}
	return &UsageError{Msg: err.Error()}
}
