# syntax = docker/dockerfile:1.3

# https://hub.docker.com/_/rust
ARG RUST_VERSION=1.90.0
ARG OS_BASE=trixie

# rust source compile with cross platform build support
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-${OS_BASE} as builder-tari-ootle

# Declare to make available
ARG BUILDPLATFORM
ARG BUILDOS
ARG BUILDARCH
ARG BUILDVARIANT
ARG TARGETPLATFORM
ARG TARGETOS
ARG TARGETARCH
ARG TARGETVARIANT
ARG RUST_TOOLCHAIN
ARG RUST_TARGET
ARG RUST_VERSION
ARG OS_BASE

# Node Version
# ARG NODE_MAJOR
# ENV NODE_MAJOR=$NODE_MAJOR

# https://nodesource.com/products/distributions
# Prep nodejs lts - 20.x
# RUN apt-get update && apt-get install -y \
#       apt-transport-https \
#       ca-certificates \
#       curl \
#       gpg && \
#       mkdir -p /etc/apt/keyrings && \
#       curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg && \
#       echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list

RUN apt-get update && apt-get install -y \
      libreadline-dev \
      libsqlite3-0 \
      openssl \
      cmake \
      protobuf-compiler \
      nodejs \
      npm && \
    npm install -g typescript && \
    corepack enable && \
    corepack prepare pnpm@latest --activate

# https://gcc.gnu.org/onlinedocs/gcc/x86-Options.html
#ARG ARCH=native

#ENV RUSTFLAGS="-C target_cpu=$ARCH"
#ENV ROARING_ARCH=$ARCH
ENV CARGO_HTTP_MULTIPLEXING=false

WORKDIR /base

ADD . .

RUN apt-get update && \
    sh /base/scripts/install_ubuntu_dependencies.sh

RUN if [ "${BUILDARCH}" != "${TARGETARCH}" ] ; then \
      # Run script to help setup cross-compile environment
      . /base/docker/cross_compile_tooling.sh ; \
    fi && \
    if [ -n "${RUST_TOOLCHAIN}" ] ; then \
      # Install a non-standard toolchain if it has been requested.
      # By default we use the toolchain specified in rust-toolchain.toml
      rustup toolchain install "${RUST_TOOLCHAIN}" --force-non-host ; \
    fi && \
    if [ -n "${RUST_TARGET}" ] ; then \
      # Install rust tripple target.
      rustup target add "${RUST_TARGET}" ; \
    fi && \
    set -e && \
    cd /base/bindings && \
    pnpm install && \
    pnpm run build-dev && \
    cd /base/clients/javascript/indexer_client && \
    pnpm install && \
    pnpm run build && \
    cd /base/applications/tari_indexer/web_ui && \
    pnpm install && \
    pnpm run build && \
    cd /base/applications/tari_validator_node/web_ui && \
    pnpm install && \
    pnpm run build && \
    cd /base && \
    rustup target add wasm32-unknown-unknown && \
    rustup target list --installed && \
    rustup toolchain list && \
    rustup show && \
    cargo build \
      $( [ -n "${RUST_TARGET}" ] && echo --target "${RUST_TARGET}" ) \
      --release --locked \
      --bin tari_ootle_walletd \
      --bin tari_indexer \
      --bin tari_validator_node && \
    # Copy executable out of the cache so it is available in the runtime image.
    ls -l /base/target/${RUST_TARGET:+$RUST_TARGET/}release/tari_* && \
    cp -v \
      /base/target/${RUST_TARGET:+$RUST_TARGET/}release/tari_ootle_walletd \
      /base/target/${RUST_TARGET:+$RUST_TARGET/}release/tari_indexer \
      /base/target/${RUST_TARGET:+$RUST_TARGET/}release/tari_validator_node \
        /usr/local/bin/ && \
    echo "Tari Build Done"

# Create runtime base minimal image for the target platform executables
FROM --platform=$TARGETPLATFORM debian:${OS_BASE} as runtime

ARG BUILDPLATFORM
ARG TARGETPLATFORM
ARG TARGETOS
ARG TARGETARCH
ARG TARGETVARIANT
ARG RUST_VERSION
ARG OS_BASE

ARG VERSION

# Disable Prompt During Packages Installation
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get --no-install-recommends install -y \
      dumb-init \
      ca-certificates \
      openssl

RUN groupadd --gid 1000 tari && \
    useradd --create-home --no-log-init --shell /bin/bash \
      --home-dir /home/tari \
      --uid 1000 --gid 1000 tari

ENV dockerfile_target_platform=$TARGETPLATFORM
ENV dockerfile_version=$VERSION
ENV dockerfile_build_platform=$BUILDPLATFORM
ENV rust_version=$RUST_VERSION

# Setup some folder structure
RUN mkdir -p "/home/tari/data" && \
    chown -R tari:tari "/home/tari/"

COPY --chown=tari:tari --from=builder-tari-ootle /usr/local/bin/tari_* /usr/local/bin/

WORKDIR /home/tari
ENV USER=tari
#CMD [ "tail", "-f", "/dev/null" ]
#CMD ["dumb-init", "node", "./bin/www"]
