package mail

import (
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"net"
	netmail "net/mail"
	"net/smtp"
	"net/textproto"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/rhs2/bowline/tools/internal/outbox"
)

// Config describes the SMTP server and the sender identity.
type Config struct {
	Host     string
	Port     int
	Username string
	Password string
	// StartTLS upgrades the connection before authenticating and refuses to
	// continue when the server does not offer it.
	StartTLS bool
	// From is the RFC 5322 sender, for example "Bowline <no-reply@bowline.example>".
	From string
	// HelloName is announced in EHLO. Defaults to the host name.
	HelloName string
	// Timeout bounds one whole SMTP conversation unless the context ends sooner.
	Timeout time.Duration
}

// DefaultTimeout applies when Config.Timeout is zero.
const DefaultTimeout = 30 * time.Second

// SMTPSender delivers notifications through one SMTP server. It is safe for
// concurrent use; every Send opens its own connection.
type SMTPSender struct {
	cfg    Config
	from   *netmail.Address
	addr   string
	domain string
	dialer net.Dialer
	tls    *tls.Config
	now    func() time.Time
}

// NewSMTPSender validates cfg and returns a sender.
func NewSMTPSender(cfg Config) (*SMTPSender, error) {
	if cfg.Host == "" {
		return nil, errors.New("smtp: host is required")
	}
	if cfg.Port <= 0 || cfg.Port > 65535 {
		return nil, fmt.Errorf("smtp: port %d is out of range", cfg.Port)
	}
	from, err := netmail.ParseAddress(cfg.From)
	if err != nil {
		return nil, fmt.Errorf("smtp: from address %q: %w", cfg.From, err)
	}
	if cfg.HelloName == "" {
		if h, err := os.Hostname(); err == nil && h != "" {
			cfg.HelloName = h
		} else {
			cfg.HelloName = "bowline-notify"
		}
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = DefaultTimeout
	}
	domain := "bowline"
	if at := strings.LastIndex(from.Address, "@"); at >= 0 {
		domain = from.Address[at+1:]
	}
	return &SMTPSender{
		cfg:    cfg,
		from:   from,
		addr:   net.JoinHostPort(cfg.Host, strconv.Itoa(cfg.Port)),
		domain: domain,
		tls:    &tls.Config{ServerName: cfg.Host, MinVersion: tls.VersionTLS12},
		now:    time.Now,
	}, nil
}

// Send implements outbox.Sender. An unparseable recipient and a 5xx reply to
// RCPT TO are permanent; everything else is left to the worker's retry policy.
func (s *SMTPSender) Send(ctx context.Context, n outbox.Notification) error {
	to, err := netmail.ParseAddress(n.ToAddress)
	if err != nil {
		return outbox.Permanent(fmt.Errorf("recipient address %q: %w", n.ToAddress, err))
	}
	msg, err := Message{
		From:           s.from,
		To:             to,
		Subject:        n.Subject,
		Body:           n.BodyText,
		Date:           s.now(),
		MessageID:      fmt.Sprintf("%s.%d@%s", n.ID, n.Attempts+1, s.domain),
		NotificationID: n.ID,
	}.Bytes()
	if err != nil {
		return outbox.Permanent(err)
	}

	ctx, cancel := context.WithTimeout(ctx, s.cfg.Timeout)
	defer cancel()

	conn, err := s.dialer.DialContext(ctx, "tcp", s.addr)
	if err != nil {
		return fmt.Errorf("dial %s: %w", s.addr, err)
	}
	if dl, ok := ctx.Deadline(); ok {
		_ = conn.SetDeadline(dl)
	}
	// net/smtp knows nothing about contexts; closing the socket is how a
	// cancellation reaches a conversation that is waiting on the server.
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-ctx.Done():
			_ = conn.Close()
		case <-done:
		}
	}()

	c, err := smtp.NewClient(conn, s.cfg.Host)
	if err != nil {
		return wrap(ctx, "greeting", err)
	}
	defer c.Close()

	if err := c.Hello(s.cfg.HelloName); err != nil {
		return wrap(ctx, "EHLO", err)
	}
	if s.cfg.StartTLS {
		if ok, _ := c.Extension("STARTTLS"); !ok {
			return fmt.Errorf("server %s does not offer STARTTLS", s.addr)
		}
		if err := c.StartTLS(s.tls); err != nil {
			return wrap(ctx, "STARTTLS", err)
		}
	}
	if s.cfg.Username != "" {
		if ok, _ := c.Extension("AUTH"); !ok {
			return fmt.Errorf("server %s does not offer AUTH", s.addr)
		}
		if err := c.Auth(smtp.PlainAuth("", s.cfg.Username, s.cfg.Password, s.cfg.Host)); err != nil {
			return wrap(ctx, "AUTH", err)
		}
	}
	if err := c.Mail(s.from.Address); err != nil {
		return wrap(ctx, "MAIL FROM", err)
	}
	if err := c.Rcpt(to.Address); err != nil {
		err = wrap(ctx, "RCPT TO", err)
		if isPermanentReply(err) {
			return outbox.Permanent(err)
		}
		return err
	}
	w, err := c.Data()
	if err != nil {
		return wrap(ctx, "DATA", err)
	}
	if _, err := w.Write(msg); err != nil {
		return wrap(ctx, "DATA", err)
	}
	if err := w.Close(); err != nil {
		// The reply to the end of DATA is the server accepting the message.
		return wrap(ctx, "DATA", err)
	}
	// The message is accepted at this point; a failed QUIT changes nothing.
	_ = c.Quit()
	return nil
}

// wrap adds the SMTP step to err and reports a cancelled context in place of
// the "use of closed network connection" it causes.
func wrap(ctx context.Context, step string, err error) error {
	if ctx.Err() != nil {
		return fmt.Errorf("%s: %w", step, ctx.Err())
	}
	// The socket deadline is derived from the context deadline, so a timeout on
	// the connection means the context has run out even when its own timer has
	// not fired yet. Report it as the context error either way.
	if errors.Is(err, os.ErrDeadlineExceeded) {
		return fmt.Errorf("%s: %w", step, context.DeadlineExceeded)
	}
	return fmt.Errorf("%s: %w", step, err)
}

func isPermanentReply(err error) bool {
	var tp *textproto.Error
	return errors.As(err, &tp) && tp.Code >= 500 && tp.Code < 600
}
