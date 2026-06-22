#!/usr/bin/env bash
#
# build_ffi_lib.sh — build, strip and stage the ootle_sdk_ffi_c native lib for one target.
#
# One cargo build emits both the staticlib and the cdylib (crate-type in Cargo.toml). This stages,
# into --out, the artifacts native consumers link against:
#   libootle_sdk_ffi_c.a                          static lib   (Go cgo, static C/C++)
#   libootle_sdk_ffi_c.{so,dylib} | ootle_sdk_ffi_c.dll (+ .dll.a)  shared lib (C# P/Invoke, dynamic C/C++)
#   ootle_sdk.h                                   shared ABI header (the committed cross-repo contract)
#   provenance.json                               commit / crate version / ABI tag / per-file sha256
#
# Used by .github/workflows/ffi_libs.yml (one native runner per platform) and runnable locally.
#
# Usage:  scripts/build_ffi_lib.sh --platform <label> --target <rust-triple> --out <dir>
# Env:    OOTLE_PROFILE   release (default) | debug   (release is ~6x smaller after stripping)
set -euo pipefail

CRATE=ootle_sdk_ffi_c
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PLATFORM="" TARGET="" OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform) PLATFORM="$2"; shift 2 ;;
    --target)   TARGET="$2";   shift 2 ;;
    --out)      OUT="$2";      shift 2 ;;
    *) echo "error: unknown arg: $1" >&2; exit 2 ;;
  esac
done
[[ -n "$TARGET" ]] || { echo "error: --target required" >&2; exit 2; }
[[ -n "$OUT"    ]] || { echo "error: --out required"    >&2; exit 2; }
PLATFORM="${PLATFORM:-$TARGET}"

OOTLE_PROFILE="${OOTLE_PROFILE:-release}"
CARGO_FLAGS=( --target "$TARGET" )
PROFILE_DIR="debug"
if [[ "$OOTLE_PROFILE" == "release" ]]; then
  CARGO_FLAGS+=( --release ); PROFILE_DIR="release"
elif [[ "$OOTLE_PROFILE" != "debug" ]]; then
  echo "error: OOTLE_PROFILE must be 'release' or 'debug' (got '${OOTLE_PROFILE}')" >&2; exit 2
fi

echo "==> Building ${CRATE} (${OOTLE_PROFILE}) for ${PLATFORM} [${TARGET}]"
# `cargo rustc … -- --print native-static-libs` both builds the crate's lib (all crate-types from
# Cargo.toml) and prints the linker's real native-lib needs — a best-effort hint for consumers' link
# flags (e.g. cgo LDFLAGS), captured here from the primary build so the crate is compiled only once.
BUILD_STDERR="$(mktemp)"
trap 'rm -f "$BUILD_STDERR"' EXIT
# tee to a temp file (then drained synchronously below) and back to this process's stderr for live logs.
( cd "$REPO_DIR" && cargo rustc -p "$CRATE" "${CARGO_FLAGS[@]}" -- --print native-static-libs ) 2>&1 1>/dev/null | tee "$BUILD_STDERR" >&2
NATIVE_LIBS="$(sed -n 's/.*native-static-libs: //p' "$BUILD_STDERR" | head -n1)"

TARGET_DIR="${REPO_DIR}/target/${TARGET}/${PROFILE_DIR}"
HEADER_SRC="${REPO_DIR}/crates/${CRATE}/include/ootle_sdk.h"
[[ -f "$HEADER_SRC" ]] || { echo "error: header not found: ${HEADER_SRC}" >&2; exit 1; }

mkdir -p "$OUT"
echo "==> Staging header -> ${OUT}/ootle_sdk.h"
cp "$HEADER_SRC" "${OUT}/ootle_sdk.h"

# Stage whichever lib flavours cargo produced for this target (static + shared + windows import lib).
LIBS=()
for f in \
  "${TARGET_DIR}/lib${CRATE}.a" \
  "${TARGET_DIR}/lib${CRATE}.so" \
  "${TARGET_DIR}/lib${CRATE}.dylib" \
  "${TARGET_DIR}/${CRATE}.dll" \
  "${TARGET_DIR}/lib${CRATE}.dll.a"; do
  [[ -f "$f" ]] && { cp "$f" "${OUT}/"; LIBS+=( "$(basename "$f")" ); }
done
[[ ${#LIBS[@]} -gt 0 ]] || { echo "error: no libs produced in ${TARGET_DIR}" >&2; exit 1; }

# Strip debug symbols, keeping the global symbols the linker needs. Never --strip-all on the archive.
case "$(uname -s)" in
  Darwin) STRIP_CMD="strip -S" ;;          # works on .a and .dylib
  *)      STRIP_CMD="strip --strip-debug" ;; # linux + windows (mingw binutils)
esac
for l in "${LIBS[@]}"; do
  [[ "$l" == *.dll.a ]] && continue   # GNU import lib (stub archive) — stripping can break its symbols
  ${STRIP_CMD} "${OUT}/${l}" || echo "warn: strip failed on ${l} (continuing)"
done
echo "==> Staged + stripped (${STRIP_CMD}): ${LIBS[*]}"

# Escape for embedding as a JSON string (mingw output can contain backslashes / quotes).
NATIVE_LIBS="${NATIVE_LIBS//\\/\\\\}"
NATIVE_LIBS="${NATIVE_LIBS//\"/\\\"}"

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD 2>/dev/null || echo unknown)"
CRATE_VERSION="$(sed -nE '/^\[workspace\.package\]/,/^\[/ s/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "${REPO_DIR}/Cargo.toml" | head -n1)"
[[ -n "$CRATE_VERSION" ]] || { echo "error: could not extract crate version from Cargo.toml" >&2; exit 1; }
ABI="$(sed -nE 's/.*b"(ootle-sdk-ffi-c\/[0-9]+)\\0".*/\1/p' "${REPO_DIR}/crates/${CRATE}/src/c_abi.rs" | head -n1)"
[[ -n "$ABI" ]] || { echo "error: could not extract ABI version from c_abi.rs" >&2; exit 1; }
BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}';
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}

{
  echo "{"
  echo "  \"platform\": \"${PLATFORM}\","
  echo "  \"target\": \"${TARGET}\","
  echo "  \"commit\": \"${COMMIT}\","
  echo "  \"crate_version\": \"${CRATE_VERSION}\","
  echo "  \"abi\": \"${ABI}\","
  echo "  \"profile\": \"${OOTLE_PROFILE}\","
  echo "  \"native_static_libs\": \"${NATIVE_LIBS}\","
  echo "  \"built_at\": \"${BUILT_AT}\","
  echo "  \"libs\": {"
  first=1
  for l in "${LIBS[@]}"; do
    [[ $first -eq 1 ]] || echo ","
    first=0
    printf '    "%s": { "size_bytes": %s, "sha256": "%s" }' \
      "$l" "$(wc -c < "${OUT}/${l}" | tr -d ' ')" "$(sha256 "${OUT}/${l}")"
  done
  echo ""
  echo "  }"
  echo "}"
} > "${OUT}/provenance.json"

echo "==> Wrote ${OUT}/provenance.json (abi=${ABI}, ${CRATE_VERSION}@${COMMIT:0:9})"
echo "==> Done. Staged ${PLATFORM} into ${OUT}"
