// Package env reads typed configuration from environment variables and
// collects every problem so a program can report all of them at once instead
// of failing on the first one.
package env

import (
	"errors"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

// Lookup returns the value of a variable and whether it was set.
type Lookup func(key string) (string, bool)

// OS reads the real process environment.
func OS(key string) (string, bool) { return os.LookupEnv(key) }

// Map builds a Lookup from a map. Intended for tests.
func Map(m map[string]string) Lookup {
	return func(key string) (string, bool) {
		v, ok := m[key]
		return v, ok
	}
}

// Reader reads variables through a Lookup and remembers every parse error.
type Reader struct {
	lookup Lookup
	errs   []error
}

// New returns a Reader backed by lookup.
func New(lookup Lookup) *Reader { return &Reader{lookup: lookup} }

func (r *Reader) get(key string) (string, bool) {
	v, ok := r.lookup(key)
	if !ok {
		return "", false
	}
	v = strings.TrimSpace(v)
	if v == "" {
		return "", false
	}
	return v, true
}

func (r *Reader) fail(format string, args ...any) {
	r.errs = append(r.errs, fmt.Errorf(format, args...))
}

// String returns the variable, or def when it is unset or blank.
func (r *Reader) String(key, def string) string {
	if v, ok := r.get(key); ok {
		return v
	}
	return def
}

// Require returns the variable and records an error when it is unset or blank.
func (r *Reader) Require(key string) string {
	v, ok := r.get(key)
	if !ok {
		r.fail("%s is required", key)
	}
	return v
}

// Int returns the variable parsed as an integer, or def when unset.
func (r *Reader) Int(key string, def int) int {
	v, ok := r.get(key)
	if !ok {
		return def
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		r.fail("%s: %q is not an integer", key, v)
		return def
	}
	return n
}

// Bool accepts 1/0, true/false, yes/no and on/off (case insensitive).
func (r *Reader) Bool(key string, def bool) bool {
	v, ok := r.get(key)
	if !ok {
		return def
	}
	switch strings.ToLower(v) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	}
	r.fail("%s: %q is not a boolean (use 1 or 0)", key, v)
	return def
}

// Millis reads a whole number of milliseconds as a Duration, or def when unset.
func (r *Reader) Millis(key string, def time.Duration) time.Duration {
	v, ok := r.get(key)
	if !ok {
		return def
	}
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil || n < 0 {
		r.fail("%s: %q is not a number of milliseconds", key, v)
		return def
	}
	return time.Duration(n) * time.Millisecond
}

// Err returns every recorded problem joined into one error, or nil.
func (r *Reader) Err() error { return errors.Join(r.errs...) }

// Unquote strips one layer of matching surrounding quotes. Values copied from a
// .env file by a naive loader sometimes keep them ("Bowline <x@y>").
func Unquote(s string) string {
	if len(s) >= 2 {
		first, last := s[0], s[len(s)-1]
		if first == last && (first == '"' || first == '\'') {
			return s[1 : len(s)-1]
		}
	}
	return s
}
