package env

import (
	"strings"
	"testing"
	"time"
)

func TestReaderParsesTypedValues(t *testing.T) {
	r := New(Map(map[string]string{
		"HOST":  " mail.example ",
		"PORT":  "587",
		"TLS":   "yes",
		"POLL":  "2500",
		"BLANK": "   ",
	}))

	if got := r.String("HOST", "x"); got != "mail.example" {
		t.Errorf("String = %q", got)
	}
	if got := r.String("BLANK", "fallback"); got != "fallback" {
		t.Errorf("blank String = %q, want fallback", got)
	}
	if got := r.Int("PORT", 25); got != 587 {
		t.Errorf("Int = %d", got)
	}
	if got := r.Int("MISSING", 25); got != 25 {
		t.Errorf("missing Int = %d", got)
	}
	if !r.Bool("TLS", false) {
		t.Error("Bool(yes) = false")
	}
	if got := r.Millis("POLL", time.Second); got != 2500*time.Millisecond {
		t.Errorf("Millis = %v", got)
	}
	if err := r.Err(); err != nil {
		t.Fatalf("unexpected errors: %v", err)
	}
}

func TestReaderCollectsEveryError(t *testing.T) {
	r := New(Map(map[string]string{
		"PORT": "abc",
		"TLS":  "maybe",
		"POLL": "-5",
	}))
	r.Int("PORT", 1)
	r.Bool("TLS", false)
	r.Millis("POLL", time.Second)
	r.Require("DATABASE_URL")

	err := r.Err()
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, want := range []string{"PORT", "TLS", "POLL", "DATABASE_URL is required"} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("error %q does not mention %s", err, want)
		}
	}
}

func TestUnquote(t *testing.T) {
	cases := map[string]string{
		`"Bowline <no-reply@bowline.example>"`: "Bowline <no-reply@bowline.example>",
		`'single'`:                             "single",
		`plain`:                                "plain",
		`"mismatched'`:                         `"mismatched'`,
		`"`:                                    `"`,
	}
	for in, want := range cases {
		if got := Unquote(in); got != want {
			t.Errorf("Unquote(%q) = %q, want %q", in, got, want)
		}
	}
}
