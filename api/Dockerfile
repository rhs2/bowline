# syntax=docker/dockerfile:1.7
#
# The build context is the REPOSITORY ROOT, not this directory: the migration set is
# embedded into the binary at compile time from db/migrations (see api/src/db.rs),
# which lives outside api/.
#
#   docker build -f api/Dockerfile -t bowline/api:dev .
#
# The image carries all three binaries: the service, the migration runner and the
# demo data seeder, so one image can run a task as well as the API.

# ---- build ------------------------------------------------------------------
FROM rust:1.98-slim AS build
WORKDIR /src/api
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0

# Dependency layer: manifests plus stand-in sources, so every crates.io dependency
# compiles into a layer that editing the service sources does not invalidate.
COPY api/Cargo.toml api/Cargo.lock ./
# One stub per [[bin]] target in Cargo.toml. Adding a binary without adding it
# here breaks this layer with "can't find bin", which is what happened when
# backfill-documents was introduced.
RUN mkdir -p src/bin \
 && printf 'fn main() {}\n' > src/main.rs \
 && printf 'fn main() {}\n' > src/bin/migrate.rs \
 && printf 'fn main() {}\n' > src/bin/seed.rs \
 && printf 'fn main() {}\n' > src/bin/backfill_documents.rs \
 && : > src/lib.rs \
 && cargo build --release --locked --bins \
 && rm -rf src

# Real sources. The migrations keep their repository path because sqlx::migrate!
# resolves "../db/migrations" relative to api/.
COPY db/migrations /src/db/migrations
COPY api/src ./src
# COPY preserves the mtimes from the build context, which can be older than the
# stand-in artifacts above; cargo compares mtimes, so the sources are touched and
# the stand-in binaries removed before the real build.
RUN find src -name '*.rs' -exec touch {} + \
 && rm -f target/release/bowline-api target/release/migrate target/release/seed \
 && cargo build --release --locked --bins \
 && mkdir -p /out \
 && cp target/release/bowline-api /out/bowline-api \
 && cp target/release/migrate /out/migrate \
 && cp target/release/seed /out/seed

# ---- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
# Apply the distribution's security updates. A published base image lags the
# archive, so it ships with patched packages still outstanding; without this the
# image scan fails on fixable CVEs that have nothing to do with this code.
RUN apt-get update \
 && apt-get upgrade -y \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 bowline \
 && useradd --system --uid 10001 --gid bowline --home-dir /app --shell /usr/sbin/nologin bowline

WORKDIR /app
COPY --from=build /out/bowline-api /usr/local/bin/bowline-api
COPY --from=build /out/migrate /usr/local/bin/migrate
COPY --from=build /out/seed /usr/local/bin/seed
USER bowline:bowline

# Everything else (DATABASE_URL, JWT_SECRET, S3, ...) is injected by the platform.
ENV API_BIND=0.0.0.0:8080 \
    LOG_FORMAT=json \
    RUST_LOG=info,bowline_api=info,sqlx=warn
EXPOSE 8080

HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/bowline-api"]
