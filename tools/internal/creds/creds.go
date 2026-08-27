// Package creds stores the bowctl session (access and refresh tokens) in a
// file only the current user can read.
package creds

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"time"
)

// Credentials is the on-disk session record.
type Credentials struct {
	APIURL       string    `json:"api_url"`
	Email        string    `json:"email"`
	AccessToken  string    `json:"access_token"`
	RefreshToken string    `json:"refresh_token"`
	ExpiresAt    time.Time `json:"expires_at"`
	SavedAt      time.Time `json:"saved_at"`
}

// ErrNotFound is returned by Load when no session has been stored yet.
var ErrNotFound = errors.New("no stored credentials; run bowctl login")

// Store reads and writes one credentials file.
type Store struct {
	Path string
}

// DefaultPath resolves the credentials file: $BOWCTL_CREDENTIALS when set,
// otherwise $XDG_CONFIG_HOME/bowline/credentials.json, otherwise
// $HOME/.config/bowline/credentials.json.
func DefaultPath(getenv func(string) string) (string, error) {
	if p := getenv("BOWCTL_CREDENTIALS"); p != "" {
		return p, nil
	}
	if x := getenv("XDG_CONFIG_HOME"); x != "" {
		return filepath.Join(x, "bowline", "credentials.json"), nil
	}
	home := getenv("HOME")
	if home == "" {
		h, err := os.UserHomeDir()
		if err != nil {
			return "", fmt.Errorf("resolve home directory: %w", err)
		}
		home = h
	}
	return filepath.Join(home, ".config", "bowline", "credentials.json"), nil
}

// Load reads the file. It refuses a file readable by other users.
func (s Store) Load() (Credentials, error) {
	info, err := os.Stat(s.Path)
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return Credentials{}, ErrNotFound
		}
		return Credentials{}, fmt.Errorf("stat %s: %w", s.Path, err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm()&0o077 != 0 {
		return Credentials{}, fmt.Errorf("%s is readable by other users (mode %04o); run: chmod 600 %s", s.Path, info.Mode().Perm(), s.Path)
	}
	raw, err := os.ReadFile(s.Path)
	if err != nil {
		return Credentials{}, fmt.Errorf("read %s: %w", s.Path, err)
	}
	var c Credentials
	if err := json.Unmarshal(raw, &c); err != nil {
		return Credentials{}, fmt.Errorf("parse %s: %w", s.Path, err)
	}
	return c, nil
}

// Save writes the file atomically with mode 0600 inside a 0700 directory.
func (s Store) Save(c Credentials) error {
	dir := filepath.Dir(s.Path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("create %s: %w", dir, err)
	}
	c.SavedAt = time.Now().UTC()
	raw, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		return fmt.Errorf("encode credentials: %w", err)
	}
	tmp, err := os.CreateTemp(dir, ".credentials-*.tmp")
	if err != nil {
		return fmt.Errorf("create temp file in %s: %w", dir, err)
	}
	tmpName := tmp.Name()
	cleanup := func() { _ = os.Remove(tmpName) }
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		cleanup()
		return fmt.Errorf("chmod %s: %w", tmpName, err)
	}
	if _, err := tmp.Write(append(raw, '\n')); err != nil {
		_ = tmp.Close()
		cleanup()
		return fmt.Errorf("write %s: %w", tmpName, err)
	}
	if err := tmp.Close(); err != nil {
		cleanup()
		return fmt.Errorf("close %s: %w", tmpName, err)
	}
	if err := os.Rename(tmpName, s.Path); err != nil {
		cleanup()
		return fmt.Errorf("replace %s: %w", s.Path, err)
	}
	return nil
}

// Remove deletes the file. A missing file is not an error.
func (s Store) Remove() error {
	if err := os.Remove(s.Path); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return fmt.Errorf("remove %s: %w", s.Path, err)
	}
	return nil
}
