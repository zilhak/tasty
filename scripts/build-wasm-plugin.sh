#!/usr/bin/env bash
# Phase J.C — clipboard-history WASM plugin POC 빌드.
#
# 산출물:
#   target/wasm32-wasip2/release/tasty_plugin_clipboard_history.wasm    (raw module)
#   target/poc/clipboard-history.component.wasm                          (component, embedded WIT)
#
# 요구사항:
#   rustup target add wasm32-wasip2
#   cargo install wasm-tools

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

CRATE="tasty-plugin-clipboard-history"
TARGET_DIR="target/wasm32-wasip2/release"
RAW="$TARGET_DIR/tasty_plugin_clipboard_history.wasm"
OUT_DIR="target/poc"
OUT="$OUT_DIR/clipboard-history.component.wasm"

echo "[1/4] toolchain check"
installed_targets=$(rustup target list --installed)
if ! grep -q wasm32-wasip2 <<<"$installed_targets"; then
    echo "    installing wasm32-wasip2 target..."
    rustup target add wasm32-wasip2
fi
if ! command -v wasm-tools >/dev/null 2>&1; then
    echo "ERROR: wasm-tools not installed. run: cargo install wasm-tools" >&2
    exit 1
fi

echo "[2/4] building wasm component ($CRATE)"
cargo build -p "$CRATE" \
    --no-default-features --features wasm \
    --target wasm32-wasip2 --release

echo "[3/4] validating component"
wasm-tools validate "$RAW"
mkdir -p "$OUT_DIR"
cp "$RAW" "$OUT"

echo "[4/4] component WIT introspection"
component_wit=$(wasm-tools component wit "$OUT")
head -40 <<<"$component_wit"

echo
echo "OK — $OUT ($(du -h "$OUT" | cut -f1))"
