#! /usr/bin/env bash
#
# //  Copyright 2025 The Tari Project
# //  SPDX-License-Identifier: BSD-3-Clause


# This script is used to build and bundle Tari binaries into a single tarball for distribution.

set -e

TARI_BINS=(
    "tari_validator_node"
    "tari_dan_wallet_daemon"
    "tari_indexer"
    "tari_swarm_daemon"
)

# Temp dir for binaries
TMP_DIR=$(mktemp -d)

# Build the binaries
for bin in "${TARI_BINS[@]}"; do
    echo "Building $bin"
    cargo build --release --bin $bin
    cp target/release/$bin $TMP_DIR
done

# Create the tarball
rm -f tari-binaries.tar.gz
tar -czf tari-binaries.tar.gz -C $TMP_DIR .

rm -fr $TMP_DIR

echo "Binaries bundled into tari-binaries.tar.gz"