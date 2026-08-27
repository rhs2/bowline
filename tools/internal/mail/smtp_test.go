package mail

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"io"
	"mime"
	"mime/quotedprintable"
	"net"
	netmail "net/mail"
	"net/textproto"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rhs2/bowline/tools/internal/outbox"
)

type captured struct {
	From string
	To   []string
	Data []byte
}

// fakeSMTP speaks just enough ESMTP for net/smtp: EHLO, optional AUTH PLAIN,
// MAIL, RCPT, DATA and QUIT.
type fakeSMTP struct {
	ln         net.Listener
	rejectRcpt string
	hang       bool
	wantAuth   string // "user\x00user\x00pass" when AUTH PLAIN is required

	mu   sync.Mutex
	msgs []captured
}

func startFakeSMTP(t *testing.T) *fakeSMTP {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	s := &fakeSMTP{ln: ln}
	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go s.handle(conn)
		}
	}()
	t.Cleanup(func() { _ = ln.Close() })
	return s
}

func (s *fakeSMTP) hostPort(t *testing.T) (string, int) {
	t.Helper()
	addr := s.ln.Addr().(*net.TCPAddr)
	return addr.IP.String(), addr.Port
}

func (s *fakeSMTP) messages() []captured {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]captured(nil), s.msgs...)
}

func angle(line string) string {
	i, j := strings.Index(line, "<"), strings.LastIndex(line, ">")
	if i < 0 || j < i {
		return ""
	}
	return line[i+1 : j]
}

// readDotCRLF reads a dot-terminated DATA block, keeping the CRLF line endings
// exactly as they were on the wire and removing the dot-stuffing. (textproto's
// ReadDotBytes would normalise the line endings to LF.)
func readDotCRLF(r *bufio.Reader) ([]byte, error) {
	var out []byte
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			return nil, err
		}
		if line == ".\r\n" {
			return out, nil
		}
		if strings.HasPrefix(line, "..") {
			line = line[1:]
		}
		out = append(out, line...)
	}
}

func (s *fakeSMTP) handle(conn net.Conn) {
	defer conn.Close()
	tc := textproto.NewConn(conn)
	_ = tc.PrintfLine("220 fake ESMTP")
	if s.hang {
		_, _ = io.Copy(io.Discard, conn)
		return
	}
	var msg captured
	authed := s.wantAuth == ""
	for {
		line, err := tc.ReadLine()
		if err != nil {
			return
		}
		upper := strings.ToUpper(line)
		switch {
		case strings.HasPrefix(upper, "EHLO"):
			_ = tc.PrintfLine("250-fake")
			_ = tc.PrintfLine("250-8BITMIME")
			_ = tc.PrintfLine("250 AUTH PLAIN")
		case strings.HasPrefix(upper, "HELO"):
			_ = tc.PrintfLine("250 fake")
		case strings.HasPrefix(upper, "AUTH PLAIN"):
			raw, _ := base64.StdEncoding.DecodeString(strings.TrimSpace(line[len("AUTH PLAIN"):]))
			if string(raw) == s.wantAuth {
				authed = true
				_ = tc.PrintfLine("235 2.7.0 ok")
			} else {
				_ = tc.PrintfLine("535 5.7.8 bad credentials")
			}
		case strings.HasPrefix(upper, "MAIL FROM:"):
			if !authed {
				_ = tc.PrintfLine("530 5.7.0 authentication required")
				continue
			}
			msg.From = angle(line)
			_ = tc.PrintfLine("250 ok")
		case strings.HasPrefix(upper, "RCPT TO:"):
			addr := angle(line)
			if addr == s.rejectRcpt {
				_ = tc.PrintfLine("550 5.1.1 no such user")
				continue
			}
			msg.To = append(msg.To, addr)
			_ = tc.PrintfLine("250 ok")
		case upper == "DATA":
			_ = tc.PrintfLine("354 go ahead")
			data, err := readDotCRLF(tc.R)
			if err != nil {
				return
			}
			msg.Data = data
			s.mu.Lock()
			s.msgs = append(s.msgs, msg)
			s.mu.Unlock()
			msg = captured{}
			_ = tc.PrintfLine("250 queued")
		case upper == "RSET", upper == "NOOP":
			_ = tc.PrintfLine("250 ok")
		case upper == "QUIT":
			_ = tc.PrintfLine("221 bye")
			return
		default:
			_ = tc.PrintfLine("502 not implemented")
		}
	}
}

func newSender(t *testing.T, s *fakeSMTP, mutate func(*Config)) *SMTPSender {
	t.Helper()
	host, port := s.hostPort(t)
	cfg := Config{Host: host, Port: port, From: "Bowline <no-reply@bowline.example>", HelloName: "test", Timeout: 5 * time.Second}
	if mutate != nil {
		mutate(&cfg)
	}
	sender, err := NewSMTPSender(cfg)
	if err != nil {
		t.Fatal(err)
	}
	sender.now = func() time.Time { return time.Date(2026, 8, 27, 9, 30, 0, 0, time.UTC) }
	return sender
}

var sample = outbox.Notification{
	ID:          "0f1e2d3c-0000-4000-8000-000000000001",
	RecipientID: "emp-1",
	Channel:     "email",
	ToAddress:   "ada.chief@bowline.example",
	Subject:     "Leave request approved: café week",
	BodyText:    "Hello Ada,\n\nYour leave was approved.\r\n.Leading dot line\nRegards,\nBowline\n",
	Attempts:    2,
}

func TestSendDeliversMessage(t *testing.T) {
	srv := startFakeSMTP(t)
	sender := newSender(t, srv, nil)

	if err := sender.Send(context.Background(), sample); err != nil {
		t.Fatalf("Send: %v", err)
	}
	msgs := srv.messages()
	if len(msgs) != 1 {
		t.Fatalf("server captured %d messages, want 1", len(msgs))
	}
	got := msgs[0]
	if got.From != "no-reply@bowline.example" {
		t.Errorf("MAIL FROM = %q", got.From)
	}
	if len(got.To) != 1 || got.To[0] != sample.ToAddress {
		t.Errorf("RCPT TO = %v", got.To)
	}

	parsed, err := netmail.ReadMessage(bytes.NewReader(got.Data))
	if err != nil {
		t.Fatalf("parse captured message: %v", err)
	}
	h := parsed.Header
	checks := map[string]string{
		"From":                      "\"Bowline\" <no-reply@bowline.example>",
		"To":                        "<ada.chief@bowline.example>",
		"Message-Id":                "<0f1e2d3c-0000-4000-8000-000000000001.3@bowline.example>",
		"X-Bowline-Notification-Id": sample.ID,
		"Date":                      "Thu, 27 Aug 2026 09:30:00 +0000",
		"Content-Type":              "text/plain; charset=utf-8",
		"Content-Transfer-Encoding": "quoted-printable",
		"Auto-Submitted":            "auto-generated",
	}
	for name, want := range checks {
		if v := h.Get(name); v != want {
			t.Errorf("header %s = %q, want %q", name, v, want)
		}
	}
	subject, err := new(mime.WordDecoder).DecodeHeader(h.Get("Subject"))
	if err != nil || subject != sample.Subject {
		t.Errorf("Subject decoded to %q (%v), want %q", subject, err, sample.Subject)
	}
	if !strings.Contains(h.Get("Subject"), "=?utf-8?q?") {
		t.Errorf("non-ASCII subject was not Q-encoded: %q", h.Get("Subject"))
	}
	if !bytes.Contains(got.Data, []byte("\r\n\r\n")) || bytes.Contains(bytes.ReplaceAll(got.Data, []byte("\r\n"), nil), []byte("\n")) {
		t.Error("message on the wire must use CRLF line endings only")
	}
	body, err := io.ReadAll(quotedprintable.NewReader(parsed.Body))
	if err != nil {
		t.Fatalf("decode body: %v", err)
	}
	want := "Hello Ada,\r\n\r\nYour leave was approved.\r\n.Leading dot line\r\nRegards,\r\nBowline\r\n"
	if string(body) != want {
		t.Errorf("body = %q\nwant  %q", body, want)
	}
}

func TestSendUsesAuthWhenConfigured(t *testing.T) {
	srv := startFakeSMTP(t)
	// Named so it is obvious at a glance, and to a secret scanner, that these are
	// fixtures and not a real SES credential.
	srv.wantAuth = "\x00fake-smtp-user\x00fake-smtp-password"
	sender := newSender(t, srv, func(c *Config) {
		c.Username = "fake-smtp-user"
		c.Password = "fake-smtp-password"
	})

	if err := sender.Send(context.Background(), sample); err != nil {
		t.Fatalf("Send with auth: %v", err)
	}
	if len(srv.messages()) != 1 {
		t.Fatal("message was not delivered")
	}

	bad := newSender(t, srv, func(c *Config) { c.Username = "ses-user"; c.Password = "wrong" })
	err := bad.Send(context.Background(), sample)
	if err == nil || !strings.Contains(err.Error(), "AUTH") {
		t.Fatalf("bad credentials: err = %v, want AUTH failure", err)
	}
	if outbox.IsPermanent(err) {
		t.Error("authentication failure must be retried, not parked")
	}
}

func TestSendRejectedRecipientIsPermanent(t *testing.T) {
	srv := startFakeSMTP(t)
	srv.rejectRcpt = sample.ToAddress
	sender := newSender(t, srv, nil)

	err := sender.Send(context.Background(), sample)
	if err == nil {
		t.Fatal("expected an error")
	}
	if !outbox.IsPermanent(err) {
		t.Errorf("550 on RCPT TO should be permanent, got %v", err)
	}
	if !strings.Contains(err.Error(), "550") {
		t.Errorf("error should carry the server reply: %v", err)
	}
}

func TestSendInvalidRecipientIsPermanent(t *testing.T) {
	srv := startFakeSMTP(t)
	sender := newSender(t, srv, nil)
	n := sample
	n.ToAddress = "not an address"
	err := sender.Send(context.Background(), n)
	if !outbox.IsPermanent(err) {
		t.Errorf("unparseable address should be permanent, got %v", err)
	}
	if len(srv.messages()) != 0 {
		t.Error("nothing should have reached the server")
	}
}

func TestSendRefusesWithoutStartTLS(t *testing.T) {
	srv := startFakeSMTP(t)
	sender := newSender(t, srv, func(c *Config) { c.StartTLS = true })
	err := sender.Send(context.Background(), sample)
	if err == nil || !strings.Contains(err.Error(), "STARTTLS") {
		t.Fatalf("err = %v, want STARTTLS refusal", err)
	}
	if outbox.IsPermanent(err) {
		t.Error("a server without STARTTLS is a transient condition")
	}
}

func TestSendConnectionRefusedIsTransient(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	_ = ln.Close()

	sender, err := NewSMTPSender(Config{Host: "127.0.0.1", Port: port, From: "no-reply@bowline.example", HelloName: "test"})
	if err != nil {
		t.Fatal(err)
	}
	err = sender.Send(context.Background(), sample)
	if err == nil || !strings.Contains(err.Error(), "dial") {
		t.Fatalf("err = %v, want dial failure", err)
	}
	if outbox.IsPermanent(err) {
		t.Error("connection refused must be retried")
	}
}

func TestSendHonoursContextDeadline(t *testing.T) {
	srv := startFakeSMTP(t)
	srv.hang = true
	sender := newSender(t, srv, nil)

	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	defer cancel()
	start := time.Now()
	err := sender.Send(ctx, sample)
	if err == nil {
		t.Fatal("expected an error from a server that never answers")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("err = %v, want context.DeadlineExceeded in the chain", err)
	}
	if time.Since(start) > 2*time.Second {
		t.Errorf("Send took %v; the deadline was not enforced", time.Since(start))
	}
}

func TestNewSMTPSenderValidatesConfig(t *testing.T) {
	cases := []Config{
		{Port: 25, From: "a@b"},
		{Host: "h", Port: 0, From: "a@b"},
		{Host: "h", Port: 70000, From: "a@b"},
		{Host: "h", Port: 25, From: "\"Bowline <no-reply@bowline.example>\""},
		{Host: "h", Port: 25, From: ""},
	}
	for _, c := range cases {
		if _, err := NewSMTPSender(c); err == nil {
			t.Errorf("config %+v should be rejected", c)
		}
	}
	s, err := NewSMTPSender(Config{Host: "smtp.example", Port: 587, From: "Bowline <no-reply@bowline.example>"})
	if err != nil {
		t.Fatal(err)
	}
	if s.domain != "bowline.example" || s.addr != "smtp.example:587" || s.cfg.HelloName == "" || s.cfg.Timeout != DefaultTimeout {
		t.Errorf("unexpected defaults: %+v", s.cfg)
	}
}

func TestMessageStripsHeaderInjection(t *testing.T) {
	from, _ := netmail.ParseAddress("Bowline <no-reply@bowline.example>")
	to, _ := netmail.ParseAddress("x@y.example")
	raw, err := Message{
		From: from, To: to,
		Subject:        "Hi\r\nBcc: evil@example",
		Body:           "body",
		Date:           time.Unix(0, 0),
		MessageID:      "id@bowline.example",
		NotificationID: "n\r\nX-Injected: 1",
	}.Bytes()
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(raw, []byte("\r\nBcc:")) || bytes.Contains(raw, []byte("\r\nX-Injected:")) {
		t.Errorf("line breaks from data reached the headers:\n%s", raw)
	}
	if !bytes.HasSuffix(raw[:bytes.Index(raw, []byte("\r\n\r\n"))+2], []byte("\r\n")) {
		t.Error("headers must end with CRLF")
	}
	if _, err := (Message{From: from}).Bytes(); err == nil {
		t.Error("a message without To must be rejected")
	}
}
