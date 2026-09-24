#!/usr/bin/env bash
# WASM POC 실행 시간만 CSV로 수집한다. 별도 프로세스 plugin과의 비교 측정은 하지 않는다.

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
