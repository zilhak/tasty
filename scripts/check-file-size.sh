#!/usr/bin/env bash
# 파일 SLOC 게이트 (복잡도 게이트 파트 B) — Rust 파일의 code SLOC 상한을 강제.
# code SLOC(주석·공백 제외) > 1000 인 Rust 파일 중 allowlist 에 없고 skip 대상도
# 아닌 것이 있으면 목록을 출력하고 exit 1. 기존 대형 파일은 .complexity-file-allowlist
# 로 동결(grandfather)하고, 신규 대형 파일만 차단한다.
#
# 예외 등록: 정당하게 큰 파일은 .complexity-file-allowlist 에 레포 상대경로(슬래시)를 추가.
# skip(게이트 미적용): 테스트 모듈·생성/전사 코드는 아래 skip() 에서 제외.
#
# 정책 근거: docs/dev-guide/complexity-gate.md, docs/adr/0037-complexity-gate.md
# 선례: scripts/check-intent-discipline.sh (소스 파싱 게이트 + 위치 단위 예외)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

THRESHOLD=1000
ALLOWLIST="$ROOT/.complexity-file-allowlist"

command -v tokei >/dev/null 2>&1 || { echo "tokei 미설치: cargo install tokei"; exit 2; }

# 실제로 동작하는 python 선택. Windows 는 python3 가 Store 스텁일 수 있어 실행 검증한다.
PY=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1 && [ "$("$cand" -c 'print(1)' 2>/dev/null)" = "1" ]; then
        PY="$cand"; break
    fi
done
[ -n "$PY" ] || { echo "python 미설치: tokei JSON 파싱에 python3 필요"; exit 2; }

# skip: 테스트 모듈/디렉토리 · 생성/전사 코드(*generated*, design-tokens/generated/).
skip() {
    case "$1" in
        */tests/*|*/tests.rs|*_test.rs|*_tests.rs) return 0 ;;
        *generated*|*/design-tokens/generated/*)   return 0 ;;
        *) return 1 ;;
    esac
}

# 측정 실패를 "위반 없음" 으로 읽지 않는다 (필수).
#
# tokei 나 파서가 죽어도 게이트가 초록이 되던 자리다. 원인이 둘이었다:
#   - `mapfile < <(...)` 의 프로세스 치환은 종료코드를 버린다 — `set -o pipefail` 이 안 닿는다.
#   - 결과가 0 줄인 것과 "위반이 0 건인 것" 이 구분되지 않았다.
# 그래서 tokei 는 rc 를 받아 검사하고, 파서는 Rust report 가 하나도 없으면 비영으로 죽는다.
# src·crates 에 Rust 파일이 0 개인 상황은 이 저장소에 없으므로, 0 개는 곧 측정 실패다.
# 측정 실패는 위반(exit 1)과도 구분해 **exit 2**(환경/도구 문제)로 낸다.
#
# 회귀는 tests/file_sloc_gate_fails_loudly.rs 가 스텁 tokei 로 네 경우를 모두 고정한다.
TOKEI_JSON="$(tokei --output json src crates)" || {
    echo "tokei 실행 실패 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2; }

# tokei JSON → "code<TAB>path" (code SLOC > THRESHOLD 인 Rust 파일). 파일 report 는
# 최상위 "Rust".reports 에 평면으로 담긴다(children 은 임베드 언어 집계라 무시).
OVER_RAW="$(printf '%s' "$TOKEI_JSON" | THRESHOLD="$THRESHOLD" "$PY" -c '
import json, os, sys
sys.stdout.reconfigure(newline="\n")  # Windows text 모드의 \n→\r\n 변환 방지
th = int(os.environ["THRESHOLD"])
try:
    rust = json.load(sys.stdin).get("Rust", {})
except Exception as e:
    print("tokei JSON 파싱 실패: " + str(e), file=sys.stderr)
    sys.exit(3)
reports = rust.get("reports", [])
if not reports:
    print("tokei 가 Rust 파일을 하나도 보고하지 않았다 — 측정 실패로 읽는다", file=sys.stderr)
    sys.exit(3)
for r in reports:
    code = r["stats"]["code"]
    if code > th:
        print(str(code) + "\t" + r["name"].replace("\\", "/"))
')" || {
    echo "SLOC 측정 실패 — 게이트를 통과로 읽지 않는다"; exit 2; }

mapfile -t over <<< "$OVER_RAW"

violations=()
for line in "${over[@]}"; do
    line="${line%$'\r'}"  # 방어적 CR 제거(플랫폼 무관)
    [ -z "$line" ] && continue
    path="${line#*$'\t'}"
    skip "$path" && continue
    grep -qxF "$path" "$ALLOWLIST" 2>/dev/null && continue
    violations+=("$line")
done

if [ "${#violations[@]}" -gt 0 ]; then
    echo "파일 SLOC 게이트 위반: code SLOC > $THRESHOLD 인데 allowlist 에 없는 Rust 파일:"
    printf '  %s\n' "${violations[@]}"
    echo
    echo "모듈로 분할하거나, 정당하면 .complexity-file-allowlist 에 경로(레포 상대, 슬래시)를 추가하세요."
    exit 1
fi

echo "파일 SLOC 게이트 통과 (code SLOC ≤ $THRESHOLD 또는 allowlist/skip)."
