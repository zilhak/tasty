#!/usr/bin/env bash
# clipboard-history WASM POC 빌드용 스크립트. 현재 workspace에는 해당 crate가 없다.
# 요구 도구: wasm32-wasip2 Rust 타깃과 wasm-tools. 실행 명령은 보관된 POC 구성을 따른다.

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
