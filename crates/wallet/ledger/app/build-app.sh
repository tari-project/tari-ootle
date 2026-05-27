#!/usr/bin/env bash

#
# //  Copyright 2026 The Tari Project
# //  SPDX-License-Identifier: BSD-3-Clause
#

set -e

# 🎯 Ledger App Builder
# Builds the Ledger app using Docker for a given target device.

SUPPORTED_TARGETS=("nanosplus" "nanox" "stax" "flex")

usage() {
  echo ""
  echo "🦀 Ledger App Builder 🦀"
  echo ""
  echo "Usage: $0 <target>"
  echo ""
  echo "📦 Supported targets:"
  for t in "${SUPPORTED_TARGETS[@]}"; do
    echo "   • $t"
  done
  echo ""
  echo "📖 Examples:"
  echo "   $0 nanosplus"
  echo "   $0 nanox"
  echo ""
  exit 1
}

if [ -z "$1" ]; then
  echo "❌ Error: No target specified."
  usage
fi

TARGET="$1"

# Validate target
VALID=false
for t in "${SUPPORTED_TARGETS[@]}"; do
  if [ "$TARGET" = "$t" ]; then
    VALID=true
    break
  fi
done

if [ "$VALID" = false ]; then
  echo "❌ Error: Unknown target '$TARGET'."
  usage
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo ""
echo "🚀 Building Ledger app for target: $TARGET"
echo "📁 Using source directory: $SCRIPT_DIR"
echo ""

# NBGL targets (Stax/Flex) need the `nbgl` cargo feature, which enables the SDK's
# `io_new` + `nano_nbgl`. BAGL targets (Nano S+/X) build with default features.
EXTRA=""
case "$TARGET" in
  stax | flex) EXTRA="-- --features nbgl" ;;
esac

# Mount the ledger workspace root (parent of this app crate) so the `../common` path
# dependency resolves inside the container; build from the app crate directory.
docker run --rm -it \
  -v "$SCRIPT_DIR/..:/app" \
  -w /app/app \
  ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder \
  bash -lc "cargo ledger build $TARGET $EXTRA"

echo ""
echo "✅ Build complete for target: $TARGET"
echo ""

