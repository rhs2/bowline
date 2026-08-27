# syntax=docker/dockerfile:1.7
#
# Two images from one build stage:
#   docker build --target notify -t bowline/notify .
#   docker build --target bowctl -t bowline/bowctl .
#
# Both final images are distroless, static, non-root, and carry CA certificates
# (needed for STARTTLS to Amazon SES and for HTTPS to the API).

FROM golang:1.27-alpine AS build
WORKDIR /src
ARG VERSION=dev
ENV CGO_ENABLED=0 GOFLAGS=-trimpath
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod go mod download
COPY . .
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    go build -ldflags="-s -w -X main.version=${VERSION}" -o /out/notify ./cmd/notify && \
    go build -ldflags="-s -w -X main.version=${VERSION}" -o /out/bowctl ./cmd/bowctl

FROM gcr.io/distroless/static-debian12:nonroot AS notify
COPY --from=build /out/notify /usr/local/bin/notify
USER nonroot:nonroot
EXPOSE 9101
ENTRYPOINT ["/usr/local/bin/notify"]

FROM gcr.io/distroless/static-debian12:nonroot AS bowctl
COPY --from=build /out/bowctl /usr/local/bin/bowctl
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/bowctl"]
