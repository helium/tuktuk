# Build stage
FROM rust:1.85-slim-bookworm AS builder

# Install system dependencies and clean up in the same layer
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    libudev-dev \
    protobuf-compiler && \
    rm -rf /var/lib/apt/lists/* && \
    rm -rf /tmp/*

WORKDIR /app
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY tuktuk-crank-turner tuktuk-crank-turner
COPY tuktuk-program tuktuk-program
COPY tuktuk-sdk tuktuk-sdk
COPY tuktuk-cli tuktuk-cli
COPY solana-transaction-utils solana-transaction-utils

# Build with cleanup
RUN cargo build --release --package tuktuk-crank-turner && \
    rm -rf /app/target/release/deps && \
    rm -rf /app/target/release/build && \
    rm -rf /usr/local/cargo/registry

# Runtime stage
#
# Bookworm, matching the builder: the binary links against the builder's OpenSSL, so the two
# stages have to carry the same major version. Debian 11 is no longer an option -- its security
# pool no longer serves libssl 1.1, so `apt-get install libssl-dev` 404s on any rebuild.
FROM debian:bookworm-slim

# Install runtime dependencies with minimal installation. libssl3 rather than libssl-dev: the
# runtime needs the shared library, not the headers and static archives.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 && \
    rm -rf /var/lib/apt/lists/* && \
    rm -rf /tmp/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/tuktuk-crank-turner /usr/local/bin/

# Create directory for key file
RUN mkdir -p /app/keys

EXPOSE 8080

ENTRYPOINT ["tuktuk-crank-turner"]
