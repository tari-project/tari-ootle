#!/usr/bin/env bash

set -e

if ! command -v node &> /dev/null
then
    echo "❌ Node.js could not be found, please install it first. https://nodejs.org/en/download/"
    exit 1
fi

pnpm env use --global 23
echo "🟢 Node.js version: $(node -v)"

if ! command -v pnpm &> /dev/null
then
    echo "❌ pnpm could not be found, please install it first. https://pnpm.io/installation"
    exit 1
fi

#
# Builds all webuis
#

function usage() {
  echo "Usage: $0 [-h|--help] [-t|--check-typescript] [-k|--skip-bindings]"
  echo "  -h|--help    This help"
  echo "  -t|--check-typescript    Check that typescript compiles without building"
  echo "  -b|--build-bindings     Generating bindings"
  exit 1
}

# Git base dir
base_path=$(git rev-parse --show-toplevel)

# Parse arguments
while [[ $# -gt 0 ]]; do
  key="$1"
  case $key in
    -h|--help)
      usage
      ;;
    -bt|-tb)
      check_typescript=true
      build_bindings=true
      shift
      ;;
    -b|--build-bindings)
      build_bindings=true
      shift
      ;;
    -t|--check-typescript)
      check_typescript=true
      shift
      ;;
    *)
      echo "❌ Unknown option: $key"
      usage
      ;;
  esac
done

echo "📦 Installing dependencies..."
pushd $base_path > /dev/null
pnpm install
popd > /dev/null

pushd $base_path/bindings > /dev/null
if [ ! -z "${build_bindings}" ]; then
  echo "🔧 Building Bindings..."
  pnpm run build
else
  echo "🔧 Building Bindings (Dist only)..."
  pnpm run build-dev # build with the TS definitions included
fi
popd > /dev/null

pushd $base_path/applications/theming > /dev/null
  echo "🎨 Building theme..."
  pnpm build
popd > /dev/null

function build() {
  pushd $base_path/$1 > /dev/null
  if [ -z ${check_typescript+x} ]; then
    pnpm run clean-dist
    pnpm run build
  else
    npx tsc
    echo "  ✅ Typescript compiled successfully"
  fi
  popd > /dev/null
}

set +e
echo "💰 Building Wallet client..."
build clients/javascript/wallet_daemon_client

echo "🔍 Building Indexer client..."
build clients/javascript/indexer_client

# Build webuis
echo "🖥️  Building Wallet Web UI..."
build applications/tari_walletd/web_ui

echo "🖥️  Building Indexer Web UI..."
build applications/tari_indexer/web_ui

echo "🖥️  Building Validator Node Web UI..."
build applications/tari_validator_node/web_ui

echo "🎉 All done!"
