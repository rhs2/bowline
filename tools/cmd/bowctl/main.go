// Command bowctl is the Bowline operator command line. See internal/bowctl
// for the commands; this file only wires the process environment to App.
package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"github.com/rhs2/bowline/tools/internal/bowctl"
)

// version is set at build time with -ldflags "-X main.version=...".
var version = "dev"

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	app := &bowctl.App{
		Stdin:   os.Stdin,
		Stdout:  os.Stdout,
		Stderr:  os.Stderr,
		Getenv:  os.Getenv,
		Version: version,
	}
	code := app.Run(ctx, os.Args[1:])
	stop()
	os.Exit(code)
}
