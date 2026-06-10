#!/usr/bin/env bash
#
# Run the cargo-fuzz targets for a fixed time budget each.
#
# Usage:
#   fuzz/run_all.sh                       # run every target
#   fuzz/run_all.sh value_decode          # run one (or more) named target(s)
#   MAX_TOTAL_TIME=300 fuzz/run_all.sh    # 300s per target instead of the default
#
# Environment:
#   MAX_TOTAL_TIME   seconds to fuzz each target (default: 60)
#   FUZZ_TOOLCHAIN   rustup toolchain to use (default: nightly)
#
# Any panic/abort/OOM/hang in a target is a finding: cargo-fuzz writes the input
# to fuzz/artifacts/<target>/ and this script exits non-zero.

set -euo pipefail

MAX_TOTAL_TIME="${MAX_TOTAL_TIME:-60}"
FUZZ_TOOLCHAIN="${FUZZ_TOOLCHAIN:-nightly}"

# Run from the repo root regardless of where the script is invoked from.
cd "$(dirname "$0")/.."

# Build for the real host triple. cargo-fuzz otherwise defaults to the triple it
# was *itself* compiled for — and prebuilt (e.g. binstall'd) binaries are often
# x86_64-unknown-linux-musl, whose std isn't installed and whose static libc is
# incompatible with the sanitizer. Pinning to the host avoids both. Override with
# FUZZ_TARGET if you really want to cross-fuzz.
FUZZ_TARGET="${FUZZ_TARGET:-$(rustc "+${FUZZ_TOOLCHAIN}" -vV | sed -n 's/^host: //p')}"

# Per-target libFuzzer flags. Recursion targets bound their own stack internally;
# the alloc/compile targets need resource limits so the failure surfaces as a
# crash artifact rather than killing the runner.
declare -A TARGET_FLAGS=(
  [value_decode]="-rss_limit_mb=2048"
  [transaction_decode]="-rss_limit_mb=512 -malloc_limit_mb=64"
  [substate_id_from_str]="-rss_limit_mb=2048"
  [parse_manifest]="-rss_limit_mb=2048 -timeout=25"
  [wasm_validate_code]="-rss_limit_mb=2048 -timeout=25"
)

# Default target order (kept explicit so output is deterministic).
ALL_TARGETS=(value_decode transaction_decode substate_id_from_str parse_manifest wasm_validate_code)

if ! cargo "+${FUZZ_TOOLCHAIN}" fuzz --version >/dev/null 2>&1; then
  echo "error: cargo-fuzz is not available on toolchain '${FUZZ_TOOLCHAIN}'." >&2
  echo "       install it with: cargo install cargo-fuzz" >&2
  exit 127
fi

if [[ $# -gt 0 ]]; then
  TARGETS=("$@")
else
  TARGETS=("${ALL_TARGETS[@]}")
fi

status=0
for target in "${TARGETS[@]}"; do
  flags="${TARGET_FLAGS[$target]:-}"
  if [[ -z "${TARGET_FLAGS[$target]+set}" ]]; then
    echo "error: unknown fuzz target '${target}'. Known targets: ${ALL_TARGETS[*]}" >&2
    exit 2
  fi

  # The evolving corpus is gitignored runtime state; new finds are written here.
  # Committed seeds (read-only) are merged in from fuzz/seeds/<target> if present.
  mkdir -p "fuzz/corpus/${target}"
  corpus_args=("fuzz/corpus/${target}")
  if [[ -d "fuzz/seeds/${target}" ]]; then
    corpus_args+=("fuzz/seeds/${target}")
  fi

  echo "==> fuzzing '${target}' for ${MAX_TOTAL_TIME}s on ${FUZZ_TARGET} (flags: ${flags:-none})"
  # shellcheck disable=SC2086 # word-splitting of $flags is intentional
  if ! cargo "+${FUZZ_TOOLCHAIN}" fuzz run --target "${FUZZ_TARGET}" "${target}" "${corpus_args[@]}" -- -max_total_time="${MAX_TOTAL_TIME}" ${flags}; then
    echo "!! target '${target}' produced a finding (see fuzz/artifacts/${target}/)" >&2
    status=1
  fi
done

exit "${status}"
