# Stage 1: Get uv binary
FROM ghcr.io/astral-sh/uv:latest AS uv

# Stage 2: Build the application
FROM python:3.13-alpine AS builder

# Install uv from the first stage
COPY --from=uv /uv /usr/local/bin/uv

WORKDIR /app

# Setup the environment
COPY ./scheduler-py/pyproject.toml ./scheduler-py/uv.lock* ./

RUN uv venv && \
    uv sync --frozen

# Copy the application code
COPY ./scheduler-py/scheduler.py .

# FINAL STAGE: Use the same Python base image for consistency
FROM python:3.13-alpine

# Install Docker CLI
RUN apk update && apk add --no-cache \
    docker-cli \
    docker-compose \
    bash \
    curl \
    jq \
    && rm -rf /var/cache/apk/*

COPY compose.yaml /app/

WORKDIR /app

# Copy the application code
COPY --from=builder /app/scheduler.py .

# Create a new venv in the final image instead of copying
COPY --from=uv /uv /usr/local/bin/uv
COPY --from=builder /app/pyproject.toml /app/uv.lock* ./
RUN uv venv && \
    uv sync --frozen

# Use the venv Python interpreter
ENTRYPOINT ["/app/.venv/bin/python", "scheduler.py"]
