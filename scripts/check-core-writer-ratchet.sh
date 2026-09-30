#!/usr/bin/env bash
# Core::apply 밖에서 구조 상태를 바꾸는 writer를 (지표, 파일, 함수) 단위로 세어 기준 파일과 비교한다.
# 지표 합계가 늘거나 기준에 없던 위치가 생기면 실패하고, 줄면 통과하며 기준을 낮추라고 안내한다.
# 수신자 타입을 해석하지 않는 텍스트 검사다. 범위와 한계는 docs/dev-guide/ci-gates.md.
#
# 사용: bash scripts/check-core-writer-ratchet.sh [--write-baseline]
#   --write-baseline: 어느 지표 합계도 늘지 않았을 때만 현재 측정으로 기준 파일을 다시 쓴다.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BASELINE="scripts/core-writer-baseline.txt"
SCANNER="scripts/lib/core_writer_scan.py"
METRICS=(field terminal appstate)

MODE="check"
case "${1:-}" in
    '') ;;
    --write-baseline) MODE="write" ;;
    *) echo "알 수 없는 인자: $1" >&2; exit 2 ;;
esac

# Windows의 Store 실행 별칭을 도구로 오인하지 않도록 실제 Python 실행도 확인한다.
PY=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1 && [ "$("$cand" -c 'print(1)' 2>/dev/null)" = "1" ]; then
        PY="$cand"; break
    fi
done
[ -n "$PY" ] || { echo "python 미설치: 구조 writer 분석에 python3 필요" >&2; exit 2; }

. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT"
STRIP_BIN="$JUDGE_BIN"
resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
MASK_BIN="$JUDGE_BIN"
if [ -z "$STRIP_BIN" ] || [ -z "$MASK_BIN" ]; then
    echo "[core-writer] 판정 불가 — test 전용 코드를 지울 도구나 주석·문자열을 가릴 도구가 없다(또는 낡았다)."
    echo "  원문에는 시험 코드와 주석 속 예시도 포함되어 기준과 비교할 수 없다."
    echo
    echo "  기준 파일을 바꾸지 말고 도구를 빌드해라:"
    echo "      cargo build -p tasty-doc-guards --bin strip-cfg-test --bin mask-source"
    echo "      target/debug/strip-cfg-test --check-fresh ."
    echo "      target/debug/mask-source --check-fresh ."
    echo "  rebase 직후라면 이것이 첫 번째로 할 일이다."
    exit 2
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# test 전용 줄과 test로만 선언된 파일을 비운 뒤 주석·문자열을 가린다. 두 단계 모두 줄 번호를 보존한다.
if ! "$STRIP_BIN" --blank-test-only-files "$WORK/strip" "$ROOT" src >/dev/null; then
    echo "[core-writer] test 전용 코드 제거 실패 — 판정하지 않는다." >&2; exit 2
fi
if ! "$MASK_BIN" "$WORK/mask" "$WORK/strip" src >/dev/null; then
    echo "[core-writer] 마스킹 실패 — 판정하지 않는다." >&2; exit 2
fi

src_count=$(find src -type f -name '*.rs' | wc -l | tr -d ' ')
copy_count=$(find "$WORK/mask/src" -type f -name '*.rs' | wc -l | tr -d ' ')
if [ "$src_count" -eq 0 ] || [ "$src_count" -ne "$copy_count" ]; then
    echo "[core-writer] 사본 수가 원본과 다르다(원본 ${src_count} · 사본 ${copy_count}) — 판정하지 않는다." >&2
    exit 2
fi

if "$PY" "$SCANNER" "$WORK/mask" >"$WORK/rows.tsv"; then
    :
else
    echo "[core-writer] 분석 실패(종료코드 $?) — 판정하지 않는다." >&2
    exit 2
fi

# (지표, 파일, 함수)별 개수. 줄 번호는 키에 넣지 않아 코드가 이동해도 기준이 유지된다.
awk -F'\t' '{ c[$1 "\t" $3 "\t" $4]++ } END { for (k in c) print k "\t" c[k] }' \
    "$WORK/rows.tsv" | LC_ALL=C sort >"$WORK/current.tsv"

total_of() {
    awk -F'\t' -v m="$2" '!/^#/ && $1 == m { s += $4 } END { print s + 0 }' "$1"
}

echo "src .rs ${src_count}개를 훑었다 (Core::apply 구현 파일·제외 목록은 ${SCANNER})."
for m in "${METRICS[@]}"; do
    n=$(total_of "$WORK/current.tsv" "$m")
    case "$m" in
        field) label="구조 필드 직접 쓰기" ;;
        terminal) label="Core::apply 밖 terminals.insert" ;;
        appstate) label="AppState 구조 변경 pub 함수" ;;
    esac
    by_bucket=$(awk -F'\t' -v m="$m" '$1 == m { b[$2]++ } END { for (k in b) printf "%s %d · ", k, b[k] }' \
        "$WORK/rows.tsv")
    echo "  ${m}: ${n}건 — ${label} (${by_bucket% · })"
done
echo

if [ "$MODE" = write ]; then
    if [ -f "$BASELINE" ]; then
        for m in "${METRICS[@]}"; do
            cur=$(total_of "$WORK/current.tsv" "$m")
            base=$(total_of "$BASELINE" "$m")
            if [ "$cur" -gt "$base" ]; then
                echo "[core-writer] ${m} 합계가 기준보다 많다(${cur} > ${base}) — 기준 파일을 올리지 않는다." >&2
                exit 1
            fi
        done
    fi
    {
        echo "# 구조 writer 래칫 기준. scripts/check-core-writer-ratchet.sh --write-baseline 으로만 갱신한다."
        echo "# metric	file	fn	count"
        cat "$WORK/current.tsv"
    } >"$BASELINE"
    echo "기준 파일을 다시 썼다: ${BASELINE}"
    exit 0
fi

if [ ! -f "$BASELINE" ]; then
    echo "[core-writer] 기준 파일이 없다: ${BASELINE} — 판정하지 않는다." >&2
    exit 2
fi

# 기준보다 개수가 많은 (지표, 파일, 함수)가 새 위치다.
awk -F'\t' 'NR == FNR { if ($0 !~ /^#/) base[$1 "\t" $2 "\t" $3] = $4; next }
    { k = $1 "\t" $2 "\t" $3; if ($4 > (k in base ? base[k] : 0)) print k }' \
    "$BASELINE" "$WORK/current.tsv" >"$WORK/grown.tsv"

failed=0
for m in "${METRICS[@]}"; do
    cur=$(total_of "$WORK/current.tsv" "$m")
    base=$(total_of "$BASELINE" "$m")
    if [ "$cur" -gt "$base" ]; then
        echo "${m}: ${cur}건으로 기준 ${base}건보다 늘었다."
        failed=1
    fi
done

if [ -s "$WORK/grown.tsv" ]; then
    echo "기준보다 writer가 많은 위치:"
    awk -F'\t' 'NR == FNR { g[$1 "\t" $2 "\t" $3] = 1; next }
        (($1 "\t" $3 "\t" $4) in g) { printf "  [%s] %s:%s (%s, %s)\n", $1, $3, $5, $4, $6 }' \
        "$WORK/grown.tsv" "$WORK/rows.tsv"
    echo
    failed=1
fi

if [ "$failed" -ne 0 ]; then
    echo "구조 상태는 Core::apply(DomainIntent)로 바꿔라. 새 writer를 기준에 넣어 통과시키지 마라."
    echo "합계는 그대로이고 writer가 다른 함수로 옮겨졌을 뿐이라면 옮긴 것을 확인한 뒤"
    echo "  bash scripts/check-core-writer-ratchet.sh --write-baseline"
    echo "으로 기준 파일을 다시 쓴다. 이 명령은 어느 지표 합계라도 늘었으면 거절한다."
    exit 1
fi

lowered=0
for m in "${METRICS[@]}"; do
    cur=$(total_of "$WORK/current.tsv" "$m")
    base=$(total_of "$BASELINE" "$m")
    if [ "$cur" -lt "$base" ]; then
        echo "${m}: ${cur}건으로 기준 ${base}건보다 줄었다."
        lowered=1
    fi
done

if [ "$lowered" -ne 0 ]; then
    echo
    echo "통과. writer를 Core로 옮겼다면 남은 여유가 새 writer를 허용하지 않도록 기준을 낮춰라:"
    echo "  bash scripts/check-core-writer-ratchet.sh --write-baseline"
    echo "줄어든 원인이 수집 누락(파일 이동으로 입구 목록·제외 목록이 어긋남 등)이 아닌지 먼저 확인해라."
    exit 0
fi

echo "통과. 기준과 같다."
