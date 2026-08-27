package creds

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestSaveThenLoadRoundTrip(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "bowline", "credentials.json")
	s := Store{Path: path}

	in := Credentials{
		APIURL: "http://localhost:8080", Email: "ceo@bowline.example",
		AccessToken: "acc", RefreshToken: "ref",
		ExpiresAt: time.Date(2026, 8, 27, 12, 0, 0, 0, time.UTC),
	}
	if err := s.Save(in); err != nil {
		t.Fatalf("Save: %v", err)
	}
	out, err := s.Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if out.APIURL != in.APIURL || out.Email != in.Email || out.AccessToken != in.AccessToken || out.RefreshToken != in.RefreshToken || !out.ExpiresAt.Equal(in.ExpiresAt) {
		t.Errorf("round trip mismatch: %+v", out)
	}
	if out.SavedAt.IsZero() {
		t.Error("SavedAt was not stamped")
	}

	if runtime.GOOS != "windows" {
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if info.Mode().Perm() != 0o600 {
			t.Errorf("file mode %04o, want 0600", info.Mode().Perm())
		}
		dirInfo, err := os.Stat(filepath.Dir(path))
		if err != nil {
			t.Fatal(err)
		}
		if dirInfo.Mode().Perm() != 0o700 {
			t.Errorf("dir mode %04o, want 0700", dirInfo.Mode().Perm())
		}
	}

	// Overwriting keeps the mode and leaves no temp file behind.
	in.AccessToken = "acc2"
	if err := s.Save(in); err != nil {
		t.Fatalf("second Save: %v", err)
	}
	out, err = s.Load()
	if err != nil || out.AccessToken != "acc2" {
		t.Errorf("after overwrite: %+v, %v", out, err)
	}
	entries, _ := os.ReadDir(filepath.Dir(path))
	if len(entries) != 1 {
		t.Errorf("directory has %d entries, want just the credentials file", len(entries))
	}
}

func TestLoadMissingFile(t *testing.T) {
	s := Store{Path: filepath.Join(t.TempDir(), "credentials.json")}
	_, err := s.Load()
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("err = %v, want ErrNotFound", err)
	}
	if err := s.Remove(); err != nil {
		t.Errorf("Remove of a missing file should succeed: %v", err)
	}
}

func TestLoadRefusesWorldReadableFile(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("file modes are not enforced on windows")
	}
	path := filepath.Join(t.TempDir(), "credentials.json")
	if err := os.WriteFile(path, []byte(`{"access_token":"x"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := Store{Path: path}.Load()
	if err == nil || errors.Is(err, ErrNotFound) {
		t.Fatalf("err = %v, want a permissions complaint", err)
	}
}

func TestLoadRejectsGarbage(t *testing.T) {
	path := filepath.Join(t.TempDir(), "credentials.json")
	if err := os.WriteFile(path, []byte("not json"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := (Store{Path: path}).Load(); err == nil {
		t.Error("expected a parse error")
	}
}

func TestRemove(t *testing.T) {
	s := Store{Path: filepath.Join(t.TempDir(), "credentials.json")}
	if err := s.Save(Credentials{AccessToken: "x"}); err != nil {
		t.Fatal(err)
	}
	if err := s.Remove(); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Load(); !errors.Is(err, ErrNotFound) {
		t.Errorf("after Remove: %v", err)
	}
}

func TestDefaultPath(t *testing.T) {
	env := func(m map[string]string) func(string) string {
		return func(k string) string { return m[k] }
	}
	cases := []struct {
		name string
		env  map[string]string
		want string
	}{
		{"explicit", map[string]string{"BOWCTL_CREDENTIALS": "/tmp/x.json", "HOME": "/home/u"}, "/tmp/x.json"},
		{"xdg", map[string]string{"XDG_CONFIG_HOME": "/xdg", "HOME": "/home/u"}, filepath.Join("/xdg", "bowline", "credentials.json")},
		{"home", map[string]string{"HOME": "/home/u"}, filepath.Join("/home/u", ".config", "bowline", "credentials.json")},
	}
	for _, c := range cases {
		got, err := DefaultPath(env(c.env))
		if err != nil {
			t.Fatalf("%s: %v", c.name, err)
		}
		if got != c.want {
			t.Errorf("%s: got %s, want %s", c.name, got, c.want)
		}
	}
}
