#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WASM_CRATE="$SCRIPT_DIR/wasm"
OUT_DIR="$SCRIPT_DIR/pkg"

TARGET="${1:-bundler}"
PROFILE="${2:-release}"

case "$TARGET" in
    bundler|nodejs|web) ;;
    *)
        echo "Usage: $0 [bundler|nodejs|web] [release|dev]"
        echo "  target:  bundler (default), nodejs, web"
        echo "  profile: release (default), dev"
        exit 1
        ;;
esac


WASM_PACK_ARGS=(build "$WASM_CRATE" --target "$TARGET" --out-dir "$OUT_DIR" --scope tari-project)

if [ "$PROFILE" = "release" ]; then
    WASM_PACK_ARGS+=(--release)
elif [ "$PROFILE" = "dev" ]; then
    WASM_PACK_ARGS+=(--dev -- --features debug)
else
    echo "Unknown profile: $PROFILE (expected release or dev)"
    exit 1
fi

echo "Building ootle-wasm (target=$TARGET, profile=$PROFILE)..."
wasm-pack "${WASM_PACK_ARGS[@]}"

# wasm-pack omits README.md from the "files" list in package.json, so npm won't publish it.
# Patch it in so the readme appears on the npm registry.
PKG_JSON="$OUT_DIR/package.json"
if [ -f "$PKG_JSON" ] && command -v jq &>/dev/null; then
    jq '.files += ["README.md"] | .publishConfig = {"access": "public"}' "$PKG_JSON" > "$PKG_JSON.tmp" && mv "$PKG_JSON.tmp" "$PKG_JSON"
fi

# Optimise size in release mode if wasm-opt is available
if [ "$PROFILE" = "release" ] && command -v wasm-opt &>/dev/null; then
    WASM_FILE="$OUT_DIR/ootle_wasm_bg.wasm"
    if [ -f "$WASM_FILE" ]; then
        echo "Optimising WASM binary with wasm-opt..."
        wasm-opt -Oz --strip-debug --strip-producers "$WASM_FILE" -o "$WASM_FILE"
    fi
elif [ "$PROFILE" = "release" ]; then
    echo "Note: wasm-opt not found, skipping size optimisation."
    echo "  Install with: brew install binaryen (or cargo install wasm-opt)"
fi

# Report size
WASM_FILE="$OUT_DIR/ootle_wasm_bg.wasm"
if [ -f "$WASM_FILE" ]; then
    RAW_SIZE=$(wc -c < "$WASM_FILE" | tr -d ' ')
    RAW_KB=$(echo "scale=1; $RAW_SIZE / 1024" | bc)
    echo ""
    echo "Output: $OUT_DIR/"
    echo "  WASM size: ${RAW_KB} KB"
    if command -v gzip &>/dev/null; then
        GZ_SIZE=$(gzip -c "$WASM_FILE" | wc -c | tr -d ' ')
        GZ_KB=$(echo "scale=1; $GZ_SIZE / 1024" | bc)
        echo "  Gzipped:   ${GZ_KB} KB"
    fi
fi
