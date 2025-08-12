# syntax=docker/dockerfile:1.3
# Multi-stage Dockerfile for admission webhook with optimized caching

# Stage 1: Chef - Install cargo-chef for dependency caching
FROM --platform=$BUILDPLATFORM rust:1.87-slim AS chef
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install --locked cargo-chef
WORKDIR /app

# Stage 2: Planner - Generate recipe.json for dependencies
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - Build dependencies and application
FROM --platform=$BUILDPLATFORM rust:1.87-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef and zigbuild for cross-compilation
RUN cargo install --locked cargo-chef && \
    pip3 install --break-system-packages ziglang && \
    cargo install cargo-zigbuild

WORKDIR /app

# Build arguments for target architecture
ARG TARGETPLATFORM
ARG BUILDPLATFORM

# Map Docker platform to Rust target
RUN case "$TARGETPLATFORM" in \
    "linux/amd64") echo "x86_64-unknown-linux-gnu" > /tmp/rust-target ;; \
    "linux/arm64") echo "aarch64-unknown-linux-gnu" > /tmp/rust-target ;; \
    *) echo "aarch64-unknown-linux-gnu" > /tmp/rust-target ;; \
    esac

# Install the target
RUN RUST_TARGET=$(cat /tmp/rust-target) && \
    rustup target add $RUST_TARGET

# Copy recipe and cook dependencies (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-$TARGETPLATFORM \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-$TARGETPLATFORM \
    --mount=type=cache,target=/app/target,id=target-admission-$TARGETPLATFORM \
    RUST_TARGET=$(cat /tmp/rust-target) && \
    cargo chef cook --release --target=$RUST_TARGET --zigbuild --recipe-path recipe.json

# Copy source code (only invalidates if source changes)
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Build the admission webhook (uses cached dependencies)
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-$TARGETPLATFORM \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-$TARGETPLATFORM \
    --mount=type=cache,target=/app/target,id=target-admission-$TARGETPLATFORM \
    RUST_TARGET=$(cat /tmp/rust-target) && \
    cargo zigbuild --release --target=$RUST_TARGET -p admission_webhook && \
    mkdir -p /tmp/bin && \
    cp target/$RUST_TARGET/release/admission_webhook /tmp/bin/

# Stage 4: Runtime - Minimal runtime image
FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder --chown=nonroot:nonroot /tmp/bin/admission_webhook /app/admission_webhook

EXPOSE 8443 8080
ENTRYPOINT ["/app/admission_webhook"]

LABEL org.opencontainers.image.title="Neon Admission Webhook"
LABEL org.opencontainers.image.description="Kubernetes admission webhook for NeonPageserver validation"
LABEL org.opencontainers.image.source="https://github.com/molnett/neon-operator"
LABEL org.opencontainers.image.vendor="Molnett"
