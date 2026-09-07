#!/usr/bin/env bash
# 파일 SLOC 게이트 (복잡도 게이트 파트 B) — Rust 파일의 **출하** code SLOC 상한을 강제.
# code SLOC(주석·공백 제외) > 1000 인 Rust 파일 중 allowlist 에 없고 skip 대상도
# 아닌 것이 있으면 목록을 출력하고 exit 1. 기존 대형 파일은 .complexity-file-allowlist
# 로 동결(grandfather)하고, 새로 임계를 넘는 것만 차단한다.
#
# **재는 것은 원본이 아니라 인라인 `#[cfg(test)]` 를 지운 사본이다.** 판정(무엇이
# 출하되는가)은 strip-cfg-test 가 하고 계측(몇 줄인가)은 tokei 가 그대로 한다 —
# 계측기를 둘로 늘리지 않는다. 근거: docs/adr/ 의 "출하 SLOC" ADR.
#
# **이것은 신규 파일 필터가 아니라 성장 래칫이다.** 실측(2026-07-06 → 09-05): 새로 생긴
# .rs 353 개 중 게이트가 보는 임계 초과는 0 건이었고, 임계를 넘은 10 건은 전부 이미 있던
# 파일이 자란 것이었다. 재는 사건은 "큰 파일이 생겼다" 가 아니라 "파일이 자라 임계를
# 넘었다" 다. 임계 1000 의 유도와 그 근거: docs/adr/0168-the-file-sloc-threshold-is-not-derived-and-the-freeze-ratchets-one-way.md
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

# 경고 띠 — **판정이 아니다. rc 에 안 들어간다.**
#
# 임계만 있으면 이 게이트는 아무 말도 안 하다가 갑자기 막는 물건이다. 그 해악은
# 높이가 아니라 타이밍이다: 벽은 조립 시점에, 마지막 한 줄을 얹은 lane 에게 터지고
# 그 lane 은 원인이 아니다. 실측(2026-09-07): 임계 이하 최댓값이 999 인데 그 뒤로
# 997·997·995 가 붙어 있었고 900 대가 아홉이었다. 아무도 그것을 몰랐다 — 통과할 때
# 게이트가 수를 안 찍었기 때문이다.
#
# **이 값을 래칫으로 만들지 마라.** 경고 개수에 상한을 박는 순간 그것이 또 하나의
# 벽이 되고, 그러면 사람들이 파일을 줄이는 대신 그 파일을 피해 다닌다(그 형태를
# 이미 한 번 밟았다). 경고는 **값만** 나른다.
WARN_BAND=900

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
# 회귀는 tests/file_sloc_gate_fails_loudly.rs 가 스텁 tokei · 스텁 판정기로 이 경우들을
# 고정한다(수를 여기 적지 않는다 — 시험이 늘면 그 수만 낡는다. 실제로 낡아 있었다).
# 판정기: 인라인 `#[cfg(test)]` 를 빈 줄로 바꾼 사본을 만든다. 줄 번호가 보존되므로
# tokei 의 보고를 원본 좌표로 그대로 읽는다.
#
# 바이너리를 여기서 `cargo build` 로 만들지 않는다 — 이 스크립트는 `cargo test` 안에서도
# 불리고(tests/file_sloc_gate_fails_loudly.rs), 그때 중첩 cargo 는 빌드 디렉토리 잠금에서
# 서로를 기다린다. 호출자가 경로를 주거나(TASTY_STRIP_CFG_TEST_BIN), 이미 빌드된 것을 쓴다.
# 찾기와 **신선도 판정**은 공용이다. 낡은 판정기는 없는 판정기보다 나쁘다 — 여기서는
# 둘 다 판정 불가로 다룬다(위 "측정이 안 됐으면 통과가 아니다" 와 같은 근거).
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
STRIP_BIN="$(resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT")"
if [ -z "$STRIP_BIN" ]; then
    echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"
    exit 2
fi

STRIPPED="$(mktemp -d)"
# 실패 경로에서도 지운다. 종료코드는 건드리지 않는다.
trap 'rm -rf "$STRIPPED"' EXIT

"$STRIP_BIN" "$STRIPPED" "$ROOT" src crates >/dev/null || {
    echo "출하 줄 판정 실패 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2; }

TOKEI_JSON="$(cd "$STRIPPED" && tokei --output json src crates)" || {
    echo "tokei 실행 실패 — 측정이 안 됐으므로 게이트를 통과로 읽지 않는다"; exit 2; }

# tokei JSON → "code<TAB>path" (Rust 파일 **전부**, code 내림차순). 파일 report 는
# 최상위 "Rust".reports 에 평면으로 담긴다(children 은 임베드 언어 집계라 무시).
#
# **임계 비교를 파이썬에서 셸로 옮겼다 — 판정은 그대로다.** 위반은 여전히
# "code > THRESHOLD 이면서 skip 도 allowlist 도 아닌 것" 이다. 옮긴 이유는 계측이다:
# 파이썬이 임계 초과만 내보내면 **임계까지 얼마나 남았는지를 아무도 모른다**. 초록일 때
# 값을 못 찍는 게이트는 "여유 0" 과 "여유 900" 이 같은 얼굴이라, 어느 쪽인지 모른 채
# 커밋하게 된다. 내림차순이므로 skip/allowlist 를 걷어낸 첫 임계 이하 파일이 곧 최댓값이고,
# 그래서 allowlist 조회는 목록 앞부분에서 멈춘다(전수 조회가 아니다).
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
judged=0      # 이 게이트가 실제로 판정한 파일 수. **여기서는 정확한 수다** —
              # 등급(상한/하한)은 수가 아니라 술어에 붙고, 게이트 안의 술어는 skip 과
              # allowlist 를 그대로 쓰므로 근사가 아니다. 밖에서 skip 을 다시 구현해
              # 재현한 값은 하한이 될 수 있다(실측으로 그렇게 어긋났다).
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
        # 경고 띠는 임계 이하만 담는다 — 위반은 위 갈래가 이미 이름으로 찍는다.
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

# 초록일 때도 값을 찍는다 — 여유가 0 인지 900 인지가 통과/실패에 안 나타난다.
# 판정에는 안 들어간다: 상한이 아니라 보고다.
if [ -n "$max_code" ]; then
    echo "파일 SLOC 게이트 판정 ${judged} 개 · 임계초과 0 · 경고(${WARN_BAND}↑) ${#warnings[@]} · 최대 ${max_code} (${max_path}) / 임계 ${THRESHOLD} — 남은 여유 $((THRESHOLD - max_code))"
    if [ "${#warnings[@]}" -gt 0 ]; then
        echo "경고 — 임계까지 $((THRESHOLD - WARN_BAND)) 줄 안에 든 파일 (rc 에 안 들어간다 · 래칫 아니다 · 값만 나른다):"
        printf '  %s\n' "${warnings[@]}"
    fi
else
    # **판정 대상이 0 건인 것은 통과가 아니다.** 이 파일은 다른 모든 "측정이 안 됐다" 를
    # exit 2 로 내는데(위 주석 "측정 실패를 위반 없음으로 읽지 않는다"), 여기만 초록으로
    # 나가면 **아무것도 안 재고도 게이트가 통과한 것과 같은 줄**이 찍힌다. skip 과
    # allowlist 가 전부를 삼켰거나 판정기가 빈 트리를 넘긴 경우이고, 건강한 트리에서는
    # 안 온다 — 그러니 이 갈래가 초록이면 그건 게이트가 눈을 감은 것이다.
    echo "판정 대상 파일이 0 건이다 — skip/allowlist 가 전부를 삼켰거나 훑을 트리가 비었다."
    echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"
    exit 2
fi
