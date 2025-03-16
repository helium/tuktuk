# Build stage
FROM rust:1.82-slim-bullseye as builder

# Install system dependencies and clean up in the same layer
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    pkg-config \
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
FROM debian:bullseye-slim

# Install runtime dependencies with minimal installation
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev && \
    rm -rf /var/lib/apt/lists/* && \
    rm -rf /tmp/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/tuktuk-crank-turner /usr/local/bin/

# Create directory for key file
RUN mkdir -p /app/keys

EXPOSE 8080

ENTRYPOINT ["tuktuk-crank-turner"]
