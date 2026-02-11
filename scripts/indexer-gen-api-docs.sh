#
# //  Copyright 2026 The Tari Project
# //  SPDX-License-Identifier: BSD-3-Clause
#

#!/usr/bin/env bash

set -e

OPENAPI_PATH=./docs/template-lib/public/indexer/openapi.json
OUT_PATH=./docs/template-lib/public/indexer/indexer-api.html

mkdir -p "$(dirname $OPENAPI_PATH)"
# Generate API docs for the indexer
cargo run --bin indexer-gen-openapi -- $OPENAPI_PATH

npx @redocly/cli@latest build-docs $OPENAPI_PATH -o $OUT_PATH
