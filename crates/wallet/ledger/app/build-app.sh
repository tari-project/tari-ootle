#!/usr/bin/env bash

#
# //  Copyright 2026 The Tari Project
# //  SPDX-License-Identifier: BSD-3-Clause
#

set -e

# 🎯 Ledger App Builder
# Builds the Ledger app using Docker for a given target device.

SUPPORTED_TARGETS=("nanosplus" "nanox" "nanos" "stax" "flex")

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

docker run --rm -it \
  -v "$SCRIPT_DIR:/app" \
  -w /app/app \
  ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder \
  cargo ledger setup && cargo ledger build "$TARGET"

echo ""
echo "✅ Build complete for target: $TARGET"
echo ""

