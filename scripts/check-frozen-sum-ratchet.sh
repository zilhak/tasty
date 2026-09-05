#!/usr/bin/env bash
# 동결 총합 래칫 (복잡도 게이트 파트 B-2) — `.complexity-file-allowlist` 에 오른 파일들의
# **출하** code SLOC **합** 하나를 예산으로 두고, 양방향으로 고정한다.
#
# 왜 있나. `scripts/check-file-size.sh` 는 파일이 임계를 넘는 순간만 본다. 일단 목록에
# 오르면 그 파일은 얼마나 자라든 아무 신호가 없다 — 목록이 **경로만** 담기 때문이다.
# 실측(2026-07-06 → 09-05): 도입 시 동결 18 중 **15 가 자라 +2406 줄**이고, 그중
# `crates/tasty-plugin-markdown/src/render.rs` 는 1002 → 1997 로 거의 두 배가 됐는데
# 게이트는 한 번도 울지 않았다. 이 스크립트가 그 방향을 본다.
# 근거·측정·대안: docs/adr/0168-the-file-sloc-threshold-is-not-derived-and-the-freeze-ratchets-one-way.md
#
# 판정 셋.
#   합 > 예산 + 여유   → 위반(1). 동결분이 "파일 하나 분량" 만큼 자랐다.
#   합 < 예산          → 위반(1). 래칫을 조여라 — 남는 여유는 곧 안 보는 구간이다.
#   그 사이            → 통과(0).
#
# **여유는 임계 자신이다.** `check-file-size.sh` 의 `THRESHOLD` 를 읽어 쓴다 — 외우지
# 않는다. 그래서 발화 사건이 "동결분이 **허용 파일 하나 분량**만큼 자랐다" 가 되고,
# 여유가 임의의 수가 아니게 된다. 실측 발화율: 여유 0 이면 60 일에 218 회(하루 3.6 회,
# 발화 중앙 증가폭 25 줄)로 못 쓰고, 여유 = 임계면 **60 일에 4 회**다.
#
# 예산은 `.complexity-file-allowlist` 의 `# frozen-sum-budget:` 줄에 있다. 같은 파일에
# 두는 이유: 항목이 드나드는 diff 와 예산이 움직이는 diff 가 **한 화면에 붙어 보인다.**
# 목록에 항목이 **추가**되면 합이 그 파일 크기만큼 뛰는데(추가되는 파일은 정의상 임계
# 초과다), 그 추가는 이미 심사를 거친 사건이므로 그때는 예산을 아래 메시지가 알려주는
# 값으로 **갱신**하는 것이 맞다 — 그 갱신이 정당한지는 같은 커밋의 목록 diff 가 말한다.
#
# 정책 근거: docs/dev-guide/complexity-gate.md
# 선례: scripts/check-allow-reason.sh (늘어도 줄어도 실패하는 상한 래칫)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ALLOWLIST="$ROOT/.complexity-file-allowlist"
SIZE_GATE="$ROOT/scripts/check-file-size.sh"

die() { echo "$1"; echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"; exit 2; }

# 여유는 파일 임계와 같은 값이다 — 외우지 않고 그 게이트에서 읽는다.
SLACK="$(sed -n 's/^THRESHOLD=\([0-9][0-9]*\)$/\1/p' "$SIZE_GATE")"
SLACK="${SLACK%%$'\n'*}"
[ -n "$SLACK" ] || die "check-file-size.sh 에서 THRESHOLD 를 못 읽었다 — 여유를 정할 수 없다."

BUDGET="$(sed -n 's/^#[[:space:]]*frozen-sum-budget:[[:space:]]*\([0-9][0-9]*\)[[:space:]]*$/\1/p' "$ALLOWLIST")"
BUDGET="${BUDGET%%$'\n'*}"
[ -n "$BUDGET" ] || die ".complexity-file-allowlist 에 '# frozen-sum-budget: <수>' 줄이 없다."

command -v tokei >/dev/null 2>&1 || die "tokei 미설치: cargo install tokei"

PY=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1 && [ "$("$cand" -c 'print(1)' 2>/dev/null)" = "1" ]; then
        PY="$cand"; break
    fi
done
[ -n "$PY" ] || die "python 미설치: tokei JSON 파싱에 python3 필요"

# 판정기는 여기서 빌드하지 않는다 — 중첩 cargo 가 빌드 디렉토리 잠금에서 서로를 기다린다.
STRIP_BIN="${TASTY_STRIP_CFG_TEST_BIN:-}"
if [ -z "$STRIP_BIN" ]; then
    for cand in "$ROOT/target/debug/strip-cfg-test" "$ROOT/target/release/strip-cfg-test"; do
        [ -x "$cand" ] && { STRIP_BIN="$cand"; break; }
    done
fi
[ -n "$STRIP_BIN" ] || die "strip-cfg-test 가 없다 — cargo build -p tasty-doc-guards --bin strip-cfg-test"

STRIPPED="$(mktemp -d)"
trap 'rm -rf "$STRIPPED"' EXIT

"$STRIP_BIN" "$STRIPPED" "$ROOT" src crates >/dev/null || die "출하 줄 판정 실패."
TOKEI_JSON="$(cd "$STRIPPED" && tokei --output json src crates)" || die "tokei 실행 실패."

# 합계와 내역. 목록에 있는데 **디스크에 존재하면서** 보고에 없는 경로가 있으면 측정 실패다
# (없어진 파일은 0 으로 세는 것이 맞다 — 삭제는 정당하게 합을 줄인다).
REPORT="$(printf '%s' "$TOKEI_JSON" | ALLOWLIST="$ALLOWLIST" ROOT="$ROOT" "$PY" -c '
import json, os, sys
sys.stdout.reconfigure(newline="\n")
try:
    rust = json.load(sys.stdin).get("Rust", {})
except Exception as e:
    print("tokei JSON 파싱 실패: " + str(e), file=sys.stderr); sys.exit(3)
reports = rust.get("reports", [])
if not reports:
    print("tokei 가 Rust 파일을 하나도 보고하지 않았다 — 측정 실패로 읽는다", file=sys.stderr); sys.exit(3)
sizes = {r["name"].replace("\\", "/").lstrip("./"): r["stats"]["code"] for r in reports}
root = os.environ["ROOT"]
entries, missing, total = [], [], 0
for line in open(os.environ["ALLOWLIST"], encoding="utf-8"):
    p = line.strip()
    if not p or p.startswith("#"):
        continue
    if p in sizes:
        entries.append((sizes[p], p)); total += sizes[p]
    elif os.path.exists(os.path.join(root, p)):
        missing.append(p)
if missing:
    print("목록의 파일이 디스크에 있는데 보고에 없다 — 측정 실패로 읽는다: "
          + ", ".join(sorted(missing)[:5]), file=sys.stderr)
    sys.exit(3)
entries.sort(reverse=True)
print(total)
for c, p in entries:
    print(f"{c}\t{p}")
')" || die "동결 합계 측정 실패."

# 파이프를 안 쓴다 — 조기에 끝나는 소비자(`head`)의 오른쪽에 producer 를 두면 SIGPIPE 로
# 죽고 `pipefail` 이 그것을 실패로 읽는다. 가드: tests/no_early_exit_consumer_in_shell_pipes.rs
SUM="${REPORT%%$'\n'*}"
BREAKDOWN="${REPORT#*$'\n'}"
CEILING=$((BUDGET + SLACK))

if [ "$SUM" -gt "$CEILING" ]; then
    echo "동결 총합 래칫 위반: 동결 파일들의 출하 SLOC 합이 예산을 넘었다."
    echo "  합 $SUM  >  예산 $BUDGET + 여유 $SLACK = $CEILING"
    echo
    echo "  큰 것부터:"
    shown=0
    while IFS=$'\t' read -r c p; do
        shown=$((shown + 1))
        [ "$shown" -gt 8 ] && break
        printf '    %6s  %s\n' "$c" "$p"
    done <<<"$BREAKDOWN"
    echo
    echo "★ **여기서 넘었다고 이 커밋이 원인인 것은 아니다.** 이것은 누적 합이라 마지막"
    echo "  한 줄이 문턱을 밟은 것일 뿐이다 — 실제 이력에서도 복잡도를 *분해한* 리팩터"
    echo "  커밋이 발화 지점이 된 적이 있다. 어느 파일이 언제 자랐는지는 이렇게 본다:"
    echo "    git log --format='%h %s' --numstat -- \$(grep -v '^#' .complexity-file-allowlist)"
    echo
    echo "  할 일은 둘 중 하나다."
    echo "  - 위 파일 중 하나를 분해해 합을 $CEILING 이하로 내린다 (래칫이 원하는 쪽)."
    echo "  - 이 커밋이 .complexity-file-allowlist 에 **항목을 새로 추가**하는 커밋이라면,"
    echo "    그 추가는 이미 심사된 사건이므로 예산 줄을 갱신한다:"
    echo "      # frozen-sum-budget: $SUM"
    exit 1
fi

if [ "$SUM" -lt "$BUDGET" ]; then
    echo "동결 총합 래칫: 합이 예산 아래로 내려갔다 — 래칫을 조여라."
    echo "  합 $SUM  <  예산 $BUDGET"
    echo
    echo "  .complexity-file-allowlist 의 예산 줄을 이 값으로 내린다(한 줄):"
    echo "      # frozen-sum-budget: $SUM"
    echo
    echo "  남는 여유는 곧 아무도 안 보는 구간이다. 줄인 만큼 예산도 줄여야 다음 성장이 보인다."
    exit 1
fi

echo "동결 총합 래칫 통과 (합 $SUM, 예산 $BUDGET + 여유 $SLACK)."
