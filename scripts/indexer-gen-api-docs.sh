#!/usr/bin/env bash
# //  Copyright 2026 The Tari Project
# //  SPDX-License-Identifier: BSD-3-Clause

set -e

OPENAPI_PATH=./docs/developer-docs/public/indexer/openapi.json
OUT_PATH=./docs/developer-docs/public/indexer/indexer-api.html

mkdir -p "$(dirname $OPENAPI_PATH)"
# Generate API docs for the indexer
cargo run --bin indexer-gen-openapi --release --no-default-features -- $OPENAPI_PATH

npx @redocly/cli@2.20.0 build-docs $OPENAPI_PATH -o $OUT_PATH
