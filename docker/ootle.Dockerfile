# syntax=docker/dockerfile:1.7
#
# Tari Ootle Docker image.
#
# See docs/docker-build-pipeline.md for an architecture overview.
#
# Stages:
#   chef    - base toolchain: rust + system deps + node + pnpm + cargo-chef
#   planner - emits cargo-chef recipe (dependency graph snapshot)
#   builder - cooks Rust deps, installs JS deps, builds binaries
#   runtime - minimal Debian 13 with tini and the compiled binaries
#
# Build:
#   DOCKER_BUILDKIT=1 docker build -f docker/ootle.Dockerfile -t ootle:local .
#
# Run:
#   docker run --rm ootle:local tari_validator_node --help

ARG RUST_VERSION=1.95
ARG DEBIAN_VERSION=trixie
ARG NODE_MAJOR=24
ARG PNPM_VERSION=9


# ---------------------------------------------------------------------------
# chef: shared base for planner and builder
# ---------------------------------------------------------------------------
# Note: Docker Hub uses `<rust>-slim-<distro>` tag ordering (slim before distro).
FROM rust:${RUST_VERSION}-slim-${DEBIAN_VERSION} AS chef

ARG NODE_MAJOR
ARG PNPM_VERSION

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HTTP_MULTIPLEXING=false

# System build dependencies. Trimmed subset of scripts/install_ubuntu_dependencies.sh
# - omits things only useful on dev workstations (zip, less, dh-autoreconf, ncurses).
# Includes:
#   - C/C++ toolchain (make, cmake, clang, g++, libc++) for native crates
#   - libssl, libsqlite3, libreadline (linked by Rust crates at build time)
#   - libudev, libhidapi, libdbus (for ledger HW wallet support in wallet_cli)
#   - protobuf-compiler (for prost-build)
#   - git (for cargo to fetch git deps)
#   - curl, ca-certificates, gnupg (bootstrap for NodeSource)
#
# If you add a binary that needs more system deps, add them here. The repo's
# scripts/install_ubuntu_dependencies.sh is the source of truth for dev setup;
# keep this list aligned with it.
#
# apt cache and lists are mounted so they persist across builds.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    rm -f /etc/apt/apt.conf.d/docker-clean && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
      ca-certificates \
      clang \
      cmake \
      curl \
      dh-autoreconf \
      g++ \
      git \
      gnupg \
      libc++-dev \
      libc++abi-dev \
      libdbus-1-dev \
      libhidapi-dev \
      libncurses-dev \
      libprotobuf-dev \
      libreadline-dev \
      libsqlite3-dev \
      libssl-dev \
      libudev-dev \
      make \
      openssl \
      pkg-config \
      protobuf-compiler && \
    curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash - && \
    apt-get install -y --no-install-recommends nodejs && \
    corepack enable && \
    corepack prepare "pnpm@${PNPM_VERSION}" --activate

# The repo's rust-toolchain.toml pins channel = "stable", which rustup
# treats as a distinct toolchain from the pre-installed `1.95.0` shipped
# in the rust:* base image. Switch the default to `stable` first so any
# `rustup target add` (and subsequent cargo invocations) target the
# toolchain that the workspace will actually use.
#
# Wasm32 target is required: tari_template_builtin compiles its WASM
# templates via build.rs (`cargo build --target wasm32-unknown-unknown`).
RUN rustup default stable && \
    rustup target add wasm32-unknown-unknown

# cargo-chef for Rust dependency prewarming.
RUN cargo install cargo-chef --locked

WORKDIR /base


# ---------------------------------------------------------------------------
# planner: extract dependency recipe from the workspace
# ---------------------------------------------------------------------------
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path /recipe.json


# ---------------------------------------------------------------------------
# builder: cook deps, install JS deps, build binaries
# ---------------------------------------------------------------------------
FROM chef AS builder

# Phase 3a: cook Rust dependencies only. Workspace members are stubbed,
# so no application build.rs runs and pnpm is not needed yet. This layer
# is reused as long as Cargo.lock and feature selections are unchanged.
#
# /base/target is intentionally NOT a cache mount: cargo-chef relies on
# the compiled deps written to target/ to persist as a regular image
# layer so the next cargo build sees them. Cache mounts disappear after
# the RUN ends. The cargo registry/git mounts are still caches because
# they only hold downloaded crate sources, not build outputs.
COPY --from=planner /recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

# Phase 3b: copy full source and warm JS dependencies.
COPY . .

# pnpm workspace (covers walletd, indexer, validator_node web UIs plus
# shared packages: bindings, clients/javascript/*, applications/theming).
RUN --mount=type=cache,target=/root/.local/share/pnpm/store,sharing=locked \
    pnpm install --frozen-lockfile

# Pre-build shared workspace packages so leaf web UIs can import them.
# The walletd build.rs builds these itself, but indexer and validator_node
# build.rs scripts assume the shared dists already exist. Doing it once
# here is deterministic and faster than relying on cargo build ordering.
RUN pnpm --filter "@tari-project/ootle-ts-bindings" run build-dev \
 && pnpm --filter "@tari-project/ootle-web-ui-theming" run build \
 && pnpm --filter "@tari-project/wallet_jrpc_client" run build \
 && pnpm --filter "@tari-project/indexer-client" run build

# tari_swarm_daemon/webui is standalone (not in pnpm workspace, uses npm).
RUN --mount=type=cache,target=/root/.npm,sharing=locked \
    cd applications/tari_swarm_daemon/webui && npm ci

# Phase 3c: build the binaries. Each daemon's build.rs invokes pnpm/npm
# to compile its embedded web UI; node_modules and package manager
# caches from Phase 3b are reused. /base/target inherits the cooked
# dependencies as a layer from Phase 3a - cargo's incremental build
# only compiles the workspace members themselves.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/root/.local/share/pnpm/store,sharing=locked \
    --mount=type=cache,target=/root/.npm,sharing=locked \
    cargo build --release --locked \
      --bin tari_ootle_walletd \
      --bin tari_indexer \
      --bin tari_validator_node \
      --bin tari_swarm_daemon \
      --bin tari_ootle_wallet_cli \
      --bin tari_watcher \
      --bin tari_validator_rollback && \
    mkdir -p /out && \
    cp \
      target/release/tari_ootle_walletd \
      target/release/tari_indexer \
      target/release/tari_validator_node \
      target/release/tari_swarm_daemon \
      target/release/tari_ootle_wallet_cli \
      target/release/tari_watcher \
      target/release/tari_validator_rollback \
      /out/


# ---------------------------------------------------------------------------
# runtime: minimal Debian 13 with tini and the binaries
# ---------------------------------------------------------------------------
FROM debian:${DEBIAN_VERSION}-slim AS runtime

ARG VERSION=unknown
ARG RUST_VERSION
ARG DEBIAN_VERSION

ENV DEBIAN_FRONTEND=noninteractive

# Runtime libraries only - no -dev packages, no toolchain.
# - libdbus-1-3 is needed by tari_ootle_walletd (linked via secret-service crate)
# - libsqlite3-0, libreadline8 linked by daemons
# - openssl + ca-certificates for TLS
# - tini as PID 1 init
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      tini \
      ca-certificates \
      openssl \
      libsqlite3-0 \
      libreadline8 \
      libdbus-1-3 && \
    rm -rf /var/lib/apt/lists/*

# Non-root user.
RUN groupadd --gid 1000 tari && \
    useradd --create-home --no-log-init --shell /bin/bash \
      --home-dir /home/tari \
      --uid 1000 --gid 1000 tari && \
    mkdir -p /home/tari/data && \
    chown -R tari:tari /home/tari

COPY --from=builder /out/ /usr/local/bin/

# Metadata for runtime introspection (`docker inspect`, `env` inside container).
ENV OOTLE_VERSION=$VERSION \
    OOTLE_RUST_VERSION=$RUST_VERSION \
    OOTLE_DEBIAN_VERSION=$DEBIAN_VERSION \
    USER=tari

USER tari
WORKDIR /home/tari

# tini as PID 1 for proper signal handling and zombie reaping.
# No CMD: caller must specify which binary to run, e.g.
#   docker run --rm ghcr.io/tari-project/ootle tari_validator_node --help
ENTRYPOINT ["/usr/bin/tini", "--"]
