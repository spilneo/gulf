# syntax=docker/dockerfile:1.18

ARG RUST_VERSION=1.96.0

# ---------------------------------------------------------------------------
# Stage 1: Chef — install cargo-chef for incremental compilation caching
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-bookworm AS chef
WORKDIR /app
RUN cargo install cargo-chef --locked

# ---------------------------------------------------------------------------
# Stage 2: Planner — compute dependency fingerprint from Cargo.* files
# ---------------------------------------------------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------------
# Stage 3: Builder — cache and compile dependencies, then the full binary
# ---------------------------------------------------------------------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin gulf \
    && strip /app/target/release/gulf

# ---------------------------------------------------------------------------
# Stage 4: Runtime — minimal Debian slim with non-root user
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.source="https://github.com/spilneo/gulf"
LABEL org.opencontainers.image.description="Config-driven bridge and swap routing CLI/API for cross-chain token transfers"
LABEL org.opencontainers.image.licenses="Apache-2.0 OR MIT"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /usr/sbin/nologin gulf

# Create config directory with correct ownership
RUN mkdir -p /etc/gulf && chown gulf:gulf /etc/gulf

WORKDIR /app
COPY --from=builder --chown=gulf:gulf /app/target/release/gulf /usr/local/bin/gulf
COPY --chown=gulf:gulf config/gulf.toml /etc/gulf/gulf.toml

USER gulf
EXPOSE 8080

ENV GULF_CONFIG=/etc/gulf/gulf.toml

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD gulf --config /etc/gulf/gulf.toml check || exit 1

ENTRYPOINT ["gulf"]
CMD ["serve", "--listen", "0.0.0.0:8080"]
