# syntax=docker/dockerfile:1.7
# Build with: docker build -t bowline/billing:dev billing

# ---- build stage: the wrapper fetches Maven 3.9.16, then packages the jar ----
FROM eclipse-temurin:17-jdk AS build

# The wrapper pins the distribution by SHA-256, and that checksum is the one published
# for the .zip. Without unzip on PATH the wrapper silently falls back to the .tar.gz of
# the same release and then checks it against the .zip checksum, which can never match
# ("Failed to validate Maven distribution SHA-256"). Installing unzip keeps the pinned
# checksum meaningful instead of forcing it to be dropped.
RUN apt-get update \
 && apt-get install -y --no-install-recommends unzip \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Dependency layer: only re-resolved when the pom or wrapper changes.
COPY .mvn/ .mvn/
COPY mvnw pom.xml ./
RUN ./mvnw -B -q -ntp dependency:go-offline

COPY src/ src/
RUN ./mvnw -B -q -ntp package -DskipTests

# ---- runtime stage: JRE only, non-root, health checked ----
FROM eclipse-temurin:17-jre
# Apply the distribution's security updates. A published base image lags the
# archive, so it ships with patched packages still outstanding; without this the
# image scan fails on fixable CVEs that have nothing to do with this code.
RUN apt-get update \
 && apt-get upgrade -y \
 && apt-get install -y --no-install-recommends curl \
 && rm -rf /var/lib/apt/lists/* \
 # An unused Go helper that ships in the base image and carries Go stdlib
 # advisories of its own. Nothing in this service runs it.
 && rm -f /usr/bin/pebble \
 && groupadd --system --gid 10001 billing \
 && useradd --system --uid 10001 --gid billing --home-dir /app --shell /usr/sbin/nologin billing

WORKDIR /app
COPY --from=build /src/target/bowline-billing.jar /app/app.jar
RUN mkdir -p /app/out && chown -R billing:billing /app
USER billing

ENV BILLING_BIND_PORT=8081 \
    BILLING_LOCAL_OUTPUT_DIR=/app/out \
    JAVA_TOOL_OPTIONS="-XX:MaxRAMPercentage=75.0 -Djava.awt.headless=true -Djava.security.egd=file:/dev/./urandom"
EXPOSE 8081

HEALTHCHECK --interval=15s --timeout=3s --start-period=40s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${BILLING_BIND_PORT}/healthz" || exit 1

ENTRYPOINT ["java", "-jar", "/app/app.jar"]
