# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Bowline web: Next.js 15 in standalone mode.
#
# Three stages so the runtime image carries neither the toolchain nor the full
# node_modules tree: `deps` installs, `builder` compiles, and `runner` holds only
# the standalone server, its traced dependencies and the static assets.
# ---------------------------------------------------------------------------

FROM node:22-alpine AS deps
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1
# node:22-alpine bundles npm 10, which resolves the wasm32 optional subtree of
# `sharp` differently from the npm 11 that wrote package-lock.json. It would reject a
# lock file that is in fact correct, so pin the matching npm major before installing.
RUN npm install -g npm@11.6.2
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund

# ---------------------------------------------------------------------------

FROM node:22-alpine AS builder
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1
COPY --from=deps /app/node_modules ./node_modules
COPY . .
# `next build` must never call the API. Every data page is client-rendered and the
# app shell is force-dynamic, so this succeeds with no backend running.
RUN mkdir -p public && npm run build

# ---------------------------------------------------------------------------

FROM node:22-alpine AS runner
WORKDIR /app

ENV NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1 \
    PORT=3000 \
    HOSTNAME=0.0.0.0

# The upstream API the BFF proxy forwards to. Override at run time.
ENV API_INTERNAL_URL=http://api:8080

# node:22-alpine ships an unprivileged `node` user (uid 1000); the image runs as
# that user and owns nothing it does not need to write.
COPY --from=builder --chown=node:node /app/public ./public
COPY --from=builder --chown=node:node /app/.next/standalone ./
COPY --from=builder --chown=node:node /app/.next/static ./.next/static

USER node

EXPOSE 3000

# /login is the one page that renders without a session, so a 200 there proves the
# server is up and serving. Uses the runtime's own fetch, no extra packages.
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD node -e "fetch('http://127.0.0.1:'+(process.env.PORT||3000)+'/login').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"

CMD ["node", "server.js"]
