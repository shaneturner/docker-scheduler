# ---- Builder Stage ----
# Use a specific Alpine-based Rust image for smaller builds
FROM rust:1.86-alpine AS builder

# Install build dependencies for static MUSL build
# build-base is included in rust:alpine, musl-dev needed for target
RUN apk add --no-cache musl-dev openssl-dev # Keep openssl-dev if your app uses SSL/TLS features

WORKDIR /app

# Create dummy project to cache dependencies
COPY scheduler-rs/Cargo.* ./
RUN mkdir src && echo "fn main() {println!(\"dummy build\");}" > src/main.rs
# Build dependencies only for the target architecture
# Use --target to ensure MUSL build even for dependencies
RUN cargo build --release --target x86_64-unknown-linux-musl
# Clean up dummy file/dir after deps are built
RUN rm -rf src target/x86_64-unknown-linux-musl/release/deps/scheduler_rs*

# Copy actual source code
COPY scheduler-rs/src ./src

# Ensure Cargo.toml profile.release settings are applied (see below)
# Build the application release binary, targeting MUSL for static linking
RUN cargo build --release --target x86_64-unknown-linux-musl

# Strip debug symbols from the binary to reduce size
RUN strip target/x86_64-unknown-linux-musl/release/scheduler-rs

# ---- Final Stage ----
# Use the same minimal base image
FROM alpine:latest

# Install runtime dependencies: Docker CLI and Docker Compose
# Remove bash, ca-certificates (add back ca-certs if your Rust code makes HTTPS calls)
RUN apk update && apk add --no-cache \
    docker-cli \
    docker-compose \
    # ca-certificates \ # Uncomment if needed by Rust app's direct HTTPS calls
    && rm -rf /var/cache/apk/*

# Set working directory - important if docker-compose commands rely on finding compose.yaml here
WORKDIR /app

# Copy the compiled and stripped binary from the builder stage
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/scheduler-rs /usr/local/bin/scheduler-rs

# Copy the compose file - NECESSARY if running docker-compose commands inside container
COPY compose.yaml /app/compose.yaml

# Set the entrypoint to the compiled binary
ENTRYPOINT ["/usr/local/bin/scheduler-rs"]

# Optional: Add healthcheck
# HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
#   CMD # Add a command here if your app has a health endpoint/check mechanism
