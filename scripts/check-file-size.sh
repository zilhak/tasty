#!/usr/bin/env bash
# test 전용 범위를 지운 Rust 사본을 tokei로 측정한다. 임계 초과·예외·제외 기준은 아래 설정을 따른다.
# 주석·문자열을 직접 다시 해석하지 않고 strip-cfg-test를 사용한다. 정책은 docs/dev-guide/complexity-gate.md.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

THRESHOLD=1000

# 같은 디렉터리 목록으로 사본을 만들고 측정한다.
SCAN_DIRS=(src crates)

# 총합 검사도 이 플래그를 읽어 같은 사본을 측정한다.
SHIPPING_JUDGE_FLAGS=(--neutralize-char-literal-quotes --blank-test-only-files)

ALLOWLIST="$ROOT/.complexity-file-allowlist"

# WARN_BAND는 안내만 하며 종료 코드나 상한을 바꾸지 않는다.
WARN_BAND=900

command -v tokei >/dev/null 2>&1 || { echo "tokei 미설치: cargo install tokei"; exit 2; }

# Windows의 Store 실행 별칭을 도구로 오인하지 않도록 실제 Python 실행도 확인한다.
PY=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1 && [ "$("$cand" -c 'print(1)' 2>/dev/null)" = "1" ]; then
        PY="$cand"; break
    fi
done
[ -n "$PY" ] || { echo "python 미설치: tokei JSON 파싱에 python3 필요"; exit 2; }

# 이름으로 고른 test·generated 경로는 별도로 제외한다. 일부 test 파일은 이미 사본에서 지워진다.
skip() {
    case "$1" in
        */tests/*|*/tests.rs|*_test.rs|*_tests.rs) return 0 ;;
        # 대표 경로를 먼저 적되 두 패턴 모두 같은 제외 결과다.
        */design-tokens/generated/*|*generated*)   return 0 ;;
        *) return 1 ;;
    esac
}

# 도구 실행 실패·빈 보고는 위반 없음과 구분해 rc 2로 처리한다.
# cargo test 안에서도 호출하므로 여기서 보조 도구를 빌드하지 않는다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT"
STRIP_BIN="$JUDGE_BIN"
if [ -z "$STRIP_BIN" ]; then
    echo "[file-size] 판정 불가 — test 전용 코드를 제외할 도구가 없다(또는 낡았다)."
    echo "  원문에는 test 전용 코드도 포함되어 측정 조건이 달라진다. 그 값으로 임계를"
    echo "  판정하지 않는다. 먼저 현재 소스의 strip-cfg-test를 준비해라."
    echo
    echo "  .complexity-file-allowlist에 예외를 추가하지 말고 도구를 빌드해라:"
    echo "      cargo build -p tasty-doc-guards --bin strip-cfg-test"
    echo "      target/debug/strip-cfg-test --check-fresh ."
    echo "  --check-fresh의 rc=0으로 현재 소스와 일치하는지 확인해라."
    echo "  rebase 직후라면 이것이 첫 번째로 할 일이다."
    echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"
    exit 2
fi

STRIPPED="$(mktemp -d)"
trap 'rm -rf "$STRIPPED"' EXIT

# 도구가 보고한 사본 수와 디스크의 실제 사본 수를 비교해 누락을 확인한다. 파일별 내용까지 대조하지는 않는다.
STRIP_REPORTED="$("$STRIP_BIN" "${SHIPPING_JUDGE_FLAGS[@]}" "$STRIPPED" "$ROOT" "${SCAN_DIRS[@]}")" || {
    echo "출하 줄 판정 실패 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2; }

COPIED="$(find "$STRIPPED" -type f -name '*.rs' -print | wc -l | tr -d ' ')" || COPIED=""
case "$STRIP_REPORTED" in
    ''|*[!0-9]*)
        echo "판정기가 만든 사본 수를 안 냈다 — 그 수 없이는 사본이 모자란지 알 수 없다."
        echo "  받은 것: '$STRIP_REPORTED'"
        echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"; exit 2 ;;
esac
case "$COPIED" in
    ''|*[!0-9]*)
        echo "사본 수를 셀 수 없다 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2 ;;
esac
if [ "$COPIED" -ne "$STRIP_REPORTED" ]; then
    echo "도구가 보고한 사본 수와 디스크의 사본 수가 달라 판정할 수 없다."
    echo "  판정기 보고: ${STRIP_REPORTED}개 · 디스크: ${COPIED}개"
    echo "  일부 파일이 누락됐을 수 있으므로 이 결과의 '임계 초과 0'을"
    echo "  통과로 처리하지 않는다."
    echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"; exit 2
fi

TOKEI_JSON="$(cd "$STRIPPED" && tokei --output json "${SCAN_DIRS[@]}")" || {
    echo "tokei 실행 실패 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2; }

# Rust 파일 보고를 code 내림차순으로 읽는다. 임베드 언어 children은 포함하지 않는다.
ROWS_RAW="$(printf '%s' "$TOKEI_JSON" | "$PY" -c '
import json, sys
sys.stdout.reconfigure(newline="\n")  # Windows text 모드의 \n→\r\n 변환 방지
try:
    rust = json.load(sys.stdin).get("Rust", {})
except Exception as e:
    print("tokei JSON 파싱 실패: " + str(e), file=sys.stderr)
    sys.exit(3)
reports = rust.get("reports", [])
if not reports:
    print("tokei 가 Rust 파일을 하나도 보고하지 않았다 — 측정 실패로 읽는다", file=sys.stderr)
    sys.exit(3)
rows = sorted(
    ((r["stats"]["code"], r["name"].replace("\\", "/")) for r in reports),
    key=lambda cn: -cn[0],
)
for code, name in rows:
    print(str(code) + "\t" + name)
')" || {
    echo "SLOC 측정 실패 — 게이트를 통과로 읽지 않는다"; exit 2; }

mapfile -t rows <<< "$ROWS_RAW"

violations=()
warnings=()
judged=0
max_code=""   # 게이트가 실제로 판정하는 파일 중 임계 이하 최댓값(내림차순이라 첫 건)
max_path=""
for line in "${rows[@]}"; do
    line="${line%$'\r'}"  # 방어적 CR 제거(플랫폼 무관)
    [ -z "$line" ] && continue
    code="${line%%$'\t'*}"
    path="${line#*$'\t'}"
    skip "$path" && continue
    grep -qxF "$path" "$ALLOWLIST" 2>/dev/null && continue
    judged=$((judged + 1))
    if [ "$code" -gt "$THRESHOLD" ]; then
        violations+=("$line")
    else
        [ -z "$max_code" ] && { max_code="$code"; max_path="$path"; }
        [ "$code" -ge "$WARN_BAND" ] && warnings+=("$line")
    fi
done

if [ "${#violations[@]}" -gt 0 ]; then
    echo "파일 SLOC 게이트 판정 ${judged} 개 · 임계초과 ${#violations[@]} · 경고(${WARN_BAND}↑) ${#warnings[@]}"
    echo "파일 SLOC 게이트 위반: code SLOC > $THRESHOLD 인데 allowlist 에 없는 Rust 파일:"
    printf '  %s\n' "${violations[@]}"
    echo
    echo "모듈로 분할하거나, 정당하면 .complexity-file-allowlist 에 경로(레포 상대, 슬래시)를 추가하세요."
    exit 1
fi

# 통과 때도 최대값과 경고 목록을 출력한다. 경고는 실패 조건이 아니다.
if [ -n "$max_code" ]; then
    echo "파일 SLOC 게이트 판정 ${judged} 개 · 임계초과 0 · 경고(${WARN_BAND}↑) ${#warnings[@]} · 최대 ${max_code} (${max_path}) / 임계 ${THRESHOLD} — 남은 여유 $((THRESHOLD - max_code))"
    if [ "${#warnings[@]}" -gt 0 ]; then
        echo "경고 — 임계까지 $((THRESHOLD - WARN_BAND)) 줄 안에 든 파일 (안내만 하며 실패 조건은 아니다):"
        printf '  %s\n' "${warnings[@]}"
    fi
else
    echo "판정 대상 파일이 0 건이다 — 모든 파일이 skip/allowlist로 제외됐거나 검사 대상이 비었다."
    echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"
    exit 2
fi
