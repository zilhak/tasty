#!/usr/bin/env bash
# Phase J.C — WASM vs process plugin 비교 벤치마크.
#
# 본 스크립트는 *POC harness* 의 측정값을 csv 로 export 한다. process baseline
# 측정은 별 작업 — `tasty` 의 full plugin lifecycle 을 거치므로 통합 환경 필요.
# POC 단계는 wasm 측정값만 수집.
#
# 결과: ${BENCH_OUT_DIR:-${TMPDIR:-/tmp}/tasty-bench}/bench-wasm-poc.csv

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="${BENCH_OUT_DIR:-${TMPDIR:-/tmp}/tasty-bench}"
mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/bench-wasm-poc.csv"

COMPONENT="target/poc/clipboard-history.component.wasm"
HOST="crates/tasty-plugin-sdk-wasm/target/release/poc-host"

if [[ ! -f "$COMPONENT" ]]; then
    echo "missing $COMPONENT — run ./scripts/build-wasm-plugin.sh first" >&2
    exit 1
fi
if [[ ! -f "$HOST" ]]; then
    echo "missing $HOST — building..."
    cargo build --release --manifest-path crates/tasty-plugin-sdk-wasm/Cargo.toml --bin poc-host
fi

echo "mode,iter,load_ms,init_ms,open_popup_ms,roundtrip_100x_ms" > "$OUT"
ITERS=10
echo "[wasm] $ITERS iterations..."
for i in $(seq 1 $ITERS); do
    OUTPUT=$("$HOST" "$COMPONENT" 2>&1)
    LOAD=$(echo "$OUTPUT" | grep '^load' | awk '{print $2}')
    INIT=$(echo "$OUTPUT" | grep '^init' | awk '{print $2}')
    OPEN=$(echo "$OUTPUT" | grep '^open_popup' | grep -oE '[0-9]+\.[0-9]+ ms' | awk 'NR==1{print $1}')
    RT=$(echo "$OUTPUT" | grep '^handle_popup_event' | awk -F'total ' '{print $2}' | awk '{print $1}')
    echo "wasm,$i,$LOAD,$INIT,$OPEN,$RT" >> "$OUT"
done

echo
echo "wrote $OUT"
column -s, -t "$OUT"
