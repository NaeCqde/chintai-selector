#!/usr/bin/env bash
# Build the WASM SPA into web/dist. Run from the repo root or web/.
#   web/build.sh            debug build
#   web/build.sh --release  optimised build
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE_DIR=debug
CARGO_FLAGS=()
if [[ "${1:-}" == "--release" ]]; then
    PROFILE_DIR=release
    CARGO_FLAGS=(--release)
fi

cargo build -p chintai-web --target wasm32-unknown-unknown "${CARGO_FLAGS[@]}"

WASM="target/wasm32-unknown-unknown/${PROFILE_DIR}/chintai_web.wasm"
rm -rf web/dist
mkdir -p web/dist
wasm-bindgen --target web --no-typescript --out-dir web/dist --out-name chintai "$WASM"
cp web/index.html web/style.css web/dist/

echo "built web/dist ($(du -sh web/dist | cut -f1))"
