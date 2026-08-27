# Bowline documentation

Five documents, each answering one question. Read them in this order if you are
new to the system; jump straight to the one you need if you are not.

| Document | The question it answers | Length |
|---|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | How do the six services fit together, and why is it split that way? | 232 lines |
| [DOMAIN.md](DOMAIN.md) | What is the company, who reports to whom, and what are the rules? | 180 lines |
| [API.md](API.md) | What is the HTTP contract? | 172 lines |
| [SECURITY.md](SECURITY.md) | How is it secured, and what is deliberately out of scope? | 753 lines |
| [RUNBOOK.md](RUNBOOK.md) | How do I deploy it, roll it back, and fix it at 3am? | 822 lines |

The project page in this directory (`index.html`, published at
[rhs2.github.io/bowline](https://rhs2.github.io/bowline/)) is the short version of
all five, written for someone deciding whether to read further.

## Where the real contract lives

These documents describe the system, but they are not the system. When a document
and the code disagree, the code wins and the document is wrong:

- **The schema is the contract for data.** `db/migrations/` defines every table and,
  more importantly, every rule the database enforces on its own: the reporting path
  triggers, the deferred balance check on journal entries, the closed period lock,
  the leave overlap constraint, the append-only audit log. `db/tests/integrity.sql`
  proves all eleven of them and runs in CI.
- **The OpenAPI document is the contract for HTTP.** The API serves it at
  `/api-docs/openapi.json` with a browsable reference at `/docs`, generated from the
  handlers themselves. `API.md` is the readable summary; the generated document is
  authoritative and cannot drift.

That distinction is not pedantry. It is the reason a wrong sentence in `API.md`
costs an afternoon and a wrong line in a migration costs a weekend.

## Service level documentation

Each service has its own README covering how to run it, its configuration and its
tests: [`api/`](../api/README.md), [`web/`](../web/README.md),
[`billing/`](../billing/README.md), [`analytics/`](../analytics/README.md),
[`tools/`](../tools/README.md), [`db/`](../db/README.md), [`infra/`](../infra/README.md)
and [`scripts/`](../scripts/README.md). Every Terraform module carries one too.

## What is not here

Planning material, estimates and anything naming real infrastructure lives in
`internal/`, which is excluded from the repository. These documents are about the
system; that material is about the project that built it, and the two have
different audiences.
