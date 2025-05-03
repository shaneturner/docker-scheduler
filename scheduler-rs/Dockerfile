# ---- Builder Stage ----
# Use a specific Alpine-based Rust image for smaller builds
FROM rust:1.86-alpine AS builder

# Install build dependencies if needed (e.g., for static linking, openssl)
RUN apk add --no-cache musl-dev openssl-dev build-base

WORKDIR /app

# Create dummy project to cache dependencies
# RUN cargo init --bin scheduler-rs --name scheduler-rs # Use correct package name if different
COPY Cargo.* ./
# Build dependencies only (speeds up subsequent builds if dependencies don't change)
# Create dummy main.rs to allow building deps
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
# Clean up dummy file/dir after deps are built
RUN rm -rf src target/release/deps/scheduler_rs* # Adjust name if needed

# Copy actual source code
COPY src ./src

# Build the application release binary
# Ensure network access is available if crates need to download things during build
RUN cargo build --release

# ---- Final Stage ----
# Use a minimal base image that includes necessary tools
FROM alpine:latest

# Install runtime dependencies: Docker CLI and Docker Compose
# This is crucial because the Rust app EXECUTES these commands.
RUN apk update && apk add --no-cache \
    docker-cli \
    docker-compose \
    bash \
    # ca-certificates might be needed for HTTPS calls if the app made them directly
    ca-certificates \
    && rm -rf /var/cache/apk/*

WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/scheduler-rs /usr/local/bin/scheduler-rs

# Optional: Copy .env.example if needed within the container (unlikely)
# COPY .env.example /app/.env.example

# Set the entrypoint to the compiled binary
ENTRYPOINT ["/usr/local/bin/scheduler-rs"]

# Optional: Add healthcheck
# HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
#   CMD # Add a command here if your app has a health endpoint/check mechanism
