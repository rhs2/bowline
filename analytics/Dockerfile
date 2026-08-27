# Bowline analytics service: FastAPI + scikit-learn, trained at build time.
FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    ANALYTICS_BIND_PORT=8082 \
    ANALYTICS_MODEL_PATH=/app/models/delay_risk.joblib \
    LOG_FORMAT=json

WORKDIR /app

RUN groupadd --system --gid 10001 bowline \
    && useradd --system --uid 10001 --gid bowline --home-dir /app --shell /usr/sbin/nologin bowline

COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

COPY pyproject.toml ./
COPY analytics ./analytics
COPY models/README.md ./models/README.md

# Train the delay-risk model so the image is self-contained; the metadata JSON lands next to it.
RUN python -m analytics.train --out /app/models/delay_risk.joblib \
    && chown -R bowline:bowline /app/models

USER bowline

EXPOSE 8082

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["python", "-m", "analytics.healthcheck"]

CMD ["python", "-m", "analytics.main"]
