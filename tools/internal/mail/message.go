// Package mail delivers outbox notifications over SMTP.
package mail

import (
	"bytes"
	"fmt"
	"mime"
	"mime/quotedprintable"
	netmail "net/mail"
	"strings"
	"time"
)

// Message is the plain-text email built for one notification.
type Message struct {
	From    *netmail.Address
	To      *netmail.Address
	Subject string
	Body    string
	Date    time.Time
	// MessageID is the Message-ID value without the angle brackets.
	MessageID      string
	NotificationID string
}

// Bytes renders the RFC 5322 message with CRLF line endings and a
// quoted-printable body, ready for the SMTP DATA command. Header values that
// come from the database are stripped of line breaks so they cannot inject
// extra headers.
func (m Message) Bytes() ([]byte, error) {
	if m.From == nil || m.To == nil {
		return nil, fmt.Errorf("message needs both From and To")
	}
	var b bytes.Buffer
	header := func(name, value string) {
		b.WriteString(name)
		b.WriteString(": ")
		b.WriteString(value)
		b.WriteString("\r\n")
	}
	header("From", m.From.String())
	header("To", m.To.String())
	header("Subject", mime.QEncoding.Encode("utf-8", headerSafe(m.Subject)))
	header("Date", m.Date.UTC().Format(time.RFC1123Z))
	header("Message-ID", "<"+headerSafe(m.MessageID)+">")
	header("X-Bowline-Notification-Id", headerSafe(m.NotificationID))
	header("Auto-Submitted", "auto-generated")
	header("MIME-Version", "1.0")
	header("Content-Type", "text/plain; charset=utf-8")
	header("Content-Transfer-Encoding", "quoted-printable")
	b.WriteString("\r\n")

	qp := quotedprintable.NewWriter(&b)
	if _, err := qp.Write([]byte(m.Body)); err != nil {
		return nil, fmt.Errorf("encode body: %w", err)
	}
	if err := qp.Close(); err != nil {
		return nil, fmt.Errorf("encode body: %w", err)
	}
	return b.Bytes(), nil
}

func headerSafe(s string) string {
	return strings.Map(func(r rune) rune {
		if r == '\r' || r == '\n' {
			return ' '
		}
		return r
	}, s)
}
