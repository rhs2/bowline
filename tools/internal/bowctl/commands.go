package bowctl

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/rhs2/bowline/tools/internal/apiclient"
	"github.com/rhs2/bowline/tools/internal/creds"
)

func (a *App) health(ctx context.Context, cmd *Command) error {
	c, _, err := a.client(cmd, false)
	if err != nil {
		return err
	}
	probes, err := c.Health(ctx)
	if err != nil {
		return err
	}
	tw := a.table()
	fmt.Fprintln(tw, "PROBE\tSTATUS\tLATENCY\tURL\tDETAIL")
	failed := 0
	for _, p := range probes {
		status := "unreachable"
		if p.Status != 0 {
			status = fmt.Sprintf("%d %s", p.Status, http.StatusText(p.Status))
		}
		detail := ""
		if !p.OK {
			failed++
			detail = oneLine(p.Body, 80)
		}
		fmt.Fprintf(tw, "%s\t%s\t%s\t%s\t%s\n", p.Name, status, p.Latency.Round(time.Millisecond), p.URL, detail)
	}
	if err := tw.Flush(); err != nil {
		return err
	}
	if failed > 0 {
		return fmt.Errorf("%d of %d probes failed", failed, len(probes))
	}
	return nil
}

func (a *App) login(ctx context.Context, cmd *Command) error {
	c, store, err := a.client(cmd, false)
	if err != nil {
		return err
	}
	password := cmd.Login.Password
	if cmd.Login.PasswordStdin {
		raw, err := io.ReadAll(io.LimitReader(a.Stdin, 4096))
		if err != nil {
			return fmt.Errorf("read password from stdin: %w", err)
		}
		password = strings.TrimRight(string(raw), "\r\n")
		if password == "" {
			return usageErr("login: no password on stdin")
		}
	}
	sess, err := c.Login(ctx, cmd.Login.Email, password)
	if err != nil {
		return fmt.Errorf("login failed: %w", err)
	}
	tok := sess.Tokens(time.Now())
	err = store.Save(creds.Credentials{
		APIURL:       cmd.API,
		Email:        cmd.Login.Email,
		AccessToken:  tok.Access,
		RefreshToken: tok.Refresh,
		ExpiresAt:    tok.ExpiresAt,
	})
	if err != nil {
		return err
	}
	fmt.Fprintf(a.Stdout, "Logged in to %s as %s. Session saved to %s.\n", c.BaseURL(), cmd.Login.Email, store.Path)
	if sess.MustChangePassword {
		fmt.Fprintln(a.Stdout, "Note: this account must change its password before other calls succeed.")
	}
	return nil
}

func (a *App) logout(ctx context.Context, cmd *Command) error {
	c, store, err := a.client(cmd, false)
	if err != nil {
		return err
	}
	cur, err := store.Load()
	if errors.Is(err, creds.ErrNotFound) {
		fmt.Fprintln(a.Stdout, "No stored session.")
		return nil
	}
	if err != nil {
		return err
	}
	if cur.RefreshToken != "" && sameAPI(cur.APIURL, cmd.API) {
		if err := c.Logout(ctx, cur.RefreshToken); err != nil {
			fmt.Fprintf(a.Stderr, "warning: the API did not revoke the session (%v); removing the local copy anyway\n", err)
		}
	}
	if err := store.Remove(); err != nil {
		return err
	}
	fmt.Fprintf(a.Stdout, "Logged out. Removed %s.\n", store.Path)
	return nil
}

func (a *App) whoami(ctx context.Context, cmd *Command) error {
	c, _, err := a.client(cmd, true)
	if err != nil {
		return err
	}
	me, err := c.Me(ctx)
	if err != nil {
		return err
	}
	tw := a.table()
	fmt.Fprintf(tw, "User:\t%s\t%s\n", me.User.Email, me.User.ID)
	emp := me.Employee
	if emp.ID != "" || emp.FullName() != "" {
		fmt.Fprintf(tw, "Employee:\t%s\t%s\n", emp.FullName(), emp.EmployeeNo)
		fmt.Fprintf(tw, "Title:\t%s\t\n", emp.JobTitle())
		fmt.Fprintf(tw, "Department:\t%s\t\n", emp.Department)
		if emp.Manager.String() != "" {
			fmt.Fprintf(tw, "Manager:\t%s\t\n", emp.Manager)
		}
	}
	fmt.Fprintf(tw, "Roles:\t%s\t\n", strings.Join(me.Roles, ", "))
	fmt.Fprintf(tw, "Permissions:\t%d\t\n", len(me.Permissions))
	if me.User.MustChangePassword {
		fmt.Fprintf(tw, "Password:\tmust be changed\t\n")
	}
	if err := tw.Flush(); err != nil {
		return err
	}
	if len(me.Chain) > 0 {
		fmt.Fprintln(a.Stdout, "Chain of command:")
		for i, link := range me.Chain {
			fmt.Fprintf(a.Stdout, "  %d. %s, %s (level %d)\n", i+1, link.Name, link.Title, link.Level)
		}
	}
	return nil
}

func (a *App) broadcast(ctx context.Context, cmd *Command) error {
	c, _, err := a.client(cmd, true)
	if err != nil {
		return err
	}
	body, err := a.readBody(cmd.Broadcast.Body, cmd.Broadcast.BodyFile)
	if err != nil {
		return err
	}
	res, err := c.Announce(ctx, apiclient.Announcement{
		Scope:   cmd.Broadcast.Scope,
		Ref:     cmd.Broadcast.Ref,
		Subject: cmd.Broadcast.Subject,
		Body:    body,
	})
	if err != nil {
		return err
	}
	target := cmd.Broadcast.Scope
	if cmd.Broadcast.Ref != "" {
		target += " " + cmd.Broadcast.Ref
	}
	fmt.Fprintf(a.Stdout, "Announcement posted to %s%s.\n", target, createdSuffix(res))
	return nil
}

func (a *App) ticket(ctx context.Context, cmd *Command) error {
	c, _, err := a.client(cmd, true)
	if err != nil {
		return err
	}
	body, err := a.readBody(cmd.Ticket.Body, cmd.Ticket.BodyFile)
	if err != nil {
		return err
	}
	res, err := c.CreateTicket(ctx, apiclient.TicketRequest{
		Category: cmd.Ticket.Category,
		Priority: cmd.Ticket.Priority,
		Subject:  cmd.Ticket.Subject,
		Body:     body,
	})
	if err != nil {
		return err
	}
	fmt.Fprintf(a.Stdout, "Ticket opened%s.\n", createdSuffix(res))
	return nil
}

func (a *App) employees(ctx context.Context, cmd *Command) error {
	c, _, err := a.client(cmd, true)
	if err != nil {
		return err
	}
	rows, total, err := c.AllEmployees(ctx, apiclient.EmployeeFilter{
		Query:        cmd.Employees.Query,
		DepartmentID: cmd.Employees.Department,
	}, cmd.Employees.Limit)
	if err != nil {
		return err
	}
	tw := a.table()
	fmt.Fprintln(tw, "EMPLOYEE\tNAME\tTITLE\tDEPARTMENT\tEMAIL\tSTATUS")
	for _, e := range rows {
		fmt.Fprintf(tw, "%s\t%s\t%s\t%s\t%s\t%s\n", e.EmployeeNo, e.FullName(), e.JobTitle(), e.Department, e.Email, e.Status)
	}
	if err := tw.Flush(); err != nil {
		return err
	}
	if total > len(rows) {
		fmt.Fprintf(a.Stdout, "%d of %d employees shown; raise --limit to see more.\n", len(rows), total)
	} else {
		fmt.Fprintf(a.Stdout, "%d employees.\n", len(rows))
	}
	return nil
}

func (a *App) outboxDepth(ctx context.Context, cmd *Command) error {
	dsn := a.Getenv("DATABASE_URL_NOTIFY")
	if dsn == "" {
		return usageErr("outbox depth reads the notifications table directly; set DATABASE_URL_NOTIFY")
	}
	ctx, cancel := context.WithTimeout(ctx, cmd.Timeout)
	defer cancel()
	st, err := a.OutboxStats(ctx, dsn)
	if err != nil {
		return err
	}
	tw := a.table()
	fmt.Fprintf(tw, "depth\t%d\twaiting for delivery (pending %d, sending %d)\n", st.Depth(), st.Pending, st.Sending)
	fmt.Fprintf(tw, "sent\t%d\t\n", st.Sent)
	fmt.Fprintf(tw, "failed\t%d\tparked after exhausting their attempts\n", st.Failed)
	return tw.Flush()
}

// readBody returns the inline body, or the contents of a file ("-" is stdin).
func (a *App) readBody(inline, file string) (string, error) {
	if inline != "" {
		return inline, nil
	}
	var raw []byte
	var err error
	if file == "-" {
		raw, err = io.ReadAll(io.LimitReader(a.Stdin, 1<<20))
	} else {
		raw, err = os.ReadFile(file)
	}
	if err != nil {
		return "", fmt.Errorf("read body: %w", err)
	}
	body := strings.TrimRight(string(raw), "\r\n")
	if strings.TrimSpace(body) == "" {
		return "", usageErr("the body is empty")
	}
	return body, nil
}

func (a *App) table() *tabwriter.Writer {
	return tabwriter.NewWriter(a.Stdout, 0, 4, 2, ' ', 0)
}

func createdSuffix(res apiclient.Created) string {
	var parts []string
	if res.Reference != "" {
		parts = append(parts, res.Reference)
	}
	if res.ID != "" {
		parts = append(parts, "id "+res.ID)
	}
	if res.Status != "" {
		parts = append(parts, "status "+res.Status)
	}
	if len(parts) == 0 {
		return ""
	}
	return " (" + strings.Join(parts, ", ") + ")"
}

func oneLine(s string, max int) string {
	s = strings.Join(strings.Fields(s), " ")
	if len(s) > max {
		return s[:max] + "..."
	}
	return s
}
