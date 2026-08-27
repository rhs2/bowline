# Scripts

## smoke.sh

An end-to-end scenario against a running stack. It signs in as seven different
people and checks that the platform's rules actually hold, not just that the
endpoints answer.

```bash
make up && make migrate && make seed
make api          # in another terminal
./scripts/smoke.sh
```

Configuration: `API_URL` (default `http://localhost:8080`), `SEED_PASSWORD`
(default `Bowline!2026`), and optionally `DATABASE_URL` so the last check can
count the notification outbox.

What it walks, in order:

1. The API answers `/healthz`, and all seven seeded roles can sign in while a
   wrong password is refused.
2. The CEO's permission set is large, the dock worker's is small.
3. The CEO announces to the whole company; the dock worker is refused when they
   try the same thing, and receives the CEO's announcement in their inbox.
4. The dock worker cannot open a direct thread with the CFO (the messaging rules
   in `docs/DOMAIN.md`), but can message their own manager.
5. The dock worker opens a support ticket, it gets an SLA deadline from its
   priority, an agent takes it, replies and resolves it.
6. A dispatcher books a shipment. A jump straight to `delivered` is refused by
   the state machine; `draft` to `booked` to `picked_up` is accepted.
7. A work order is assigned to a driver. Another employee cannot update it; the
   assigned driver can start and complete it.
8. An accountant drafts, submits and issues an invoice, which posts a balanced
   journal entry. Overpayment is refused, the correct payment is recorded, and
   the trial balance still sums to zero.
9. An unbalanced manual journal entry is refused, and a dock worker cannot read
   the ledger at all.
10. The CFO closes last month, and a post into the closed period is refused.
11. The CEO can read the audit trail for the invoice; the dock worker cannot.
12. The messages sent along the way left rows in the notification outbox.

Each step prints PASS or FAIL and the script exits non-zero if anything failed,
so it can run in a pipeline.

## leak_scan.sh

Refuses to publish anything that should not leave the machine. It scans the files
git would publish rather than the working tree, because a secret sitting in an
ignored file is not a leak and the same secret in a tracked file is.

```bash
./scripts/leak_scan.sh              # what git would track
./scripts/leak_scan.sh --staged     # only what is staged, used by the hook
./scripts/leak_scan.sh --all        # the whole tree, ignored files included
```

Exit code 0 is clean, 1 means findings. What it looks for:

| Check | Catches |
|---|---|
| Private key material | any `BEGIN ... PRIVATE KEY` block |
| Cloud and service tokens | AWS access keys, GitHub, Slack, Stripe, Google, OpenAI, npm, PyPI |
| JSON Web Tokens | a signed token pasted into a file |
| Real AWS identity | account ids and ARNs that are not the documented placeholders |
| Infrastructure endpoints | RDS and ElastiCache hostnames |
| Personal identity | terms from `private_patterns.txt`, which is never published |
| Addresses | any email outside the reserved test domains (RFC 2606 and 6761) |
| Credential literals | a password or key assigned a literal value rather than read from the environment |
| Files | `.env`, state files, keys, `credentials.json`, anything under `internal/` |
| Size | anything over 1 MB, as a warning |

Two design decisions worth knowing:

**The private terms are not in the script.** Writing a real name or employer into
a published scanner would leak exactly what the check exists to prevent. They live
in `scripts/private_patterns.txt`, which `.gitignore` excludes. Without that file
the check is skipped and says so, rather than passing silently.

**Filters match content, never the path.** An early version excluded any line
containing the word "test", which meant a real secret in a file named
`something_test.rs` was waved through. The filter now runs on the matched text
only, after the `path:line:` prefix is stripped.

Run it before every push. To make that automatic:

```bash
git config core.hooksPath .githooks
```

CI runs it too, alongside a full-history scan that catches anything committed once
and deleted later, which a working-tree scan can never see.

## dev-up.sh and dev-down.sh

Start or stop the whole stack locally.

```bash
./scripts/dev-up.sh      # containers, migrations, seed if needed, all five services
./scripts/dev-down.sh    # stop the services, leave the containers running
```

`dev-up.sh` is safe to re-run: it skips seeding when the demo company already
exists, waits for each service to answer its health endpoint, and prints the
sign-in list at the end. Logs go to `.dev-logs/`, one file per service, so
`tail -f .dev-logs/api.log` follows the API.

One caveat: the web app runs in Next.js development mode, so never run
`npm run build` while it is up. The production build overwrites `.next` and the
development server dies with `MODULE_NOT_FOUND`. Stop it, build, remove `.next`,
start it again.
