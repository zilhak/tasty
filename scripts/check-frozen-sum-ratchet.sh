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
# **저울은 하나다.** 합만 보므로 상쇄가 안 보인다 — 한 파일이 자라는 동안 다른 파일이
# 그만큼 줄면 이 게이트는 아무 말도 안 한다(실측: 도입 3 일에 항목 11 개가 움직여 이동량
# 224 줄인데 총합 축이 본 값은 -14). 그래도 항목마다 저울을 두지 않는 이유는, 안 보는
# 구간이 **띠 x 저울 수**라 같은 발화율에서 24 배가 되기 때문이다. 표와 재검토 조건:
# docs/adr/0205-the-frozen-sum-stays-one-scale.md
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
# 그 값이어야 하는 이유: 이 폭만큼의 구간은 **아무도 안 보는 구간이 아니라**
# `check-file-size.sh` 가 파일 단위로 보는 구간이다. 둘이 구멍도 겹침도 없이
# 맞물리려면 여유가 그 임계와 같아야 하고, 그래서 적지 않고 읽어 온다.
SLACK="$(sed -n 's/^THRESHOLD=\([0-9][0-9]*\)$/\1/p' "$SIZE_GATE")"
SLACK="${SLACK%%$'\n'*}"
[ -n "$SLACK" ] || die "check-file-size.sh 에서 THRESHOLD 를 못 읽었다 — 띠 너비를 정할 수 없다."

# ── 좌변도 **외우지 않고 읽어 온다** — 지목처가 둘이었다 ──────────────────
# 이 스크립트는 같은 트리를 두 번 지목한다: 출하 사본을 뜰 때와 그 사본을 잴 때.
# 게다가 그 pathspec 은 `check-file-size.sh` 에도 두 벌 있었으니 두 파일에 걸쳐 **네
# 벌**이었다. 그 넷이 갈리면 두 게이트가 서로 다른 모수를 재는데, 이 게이트의 합은
# 정의상 저 게이트가 판정하는 집합의 부분집합(allowlist 항목)이라 **맞물림이 깨진다.**
# 그리고 그 어긋남은 조용하다 — 여유가 파일 하나 몫이라 웬만한 차이를 삼킨다.
#
# 위 `SLACK` 과 같은 규율을 쓴다: 적지 않고 저 게이트에서 읽는다. 그래서 좌변은 레포
# 전체에서 **한 곳**에만 있다. 못 읽으면 판정 불가다 — 여기서 기본값으로 물러나면
# 저쪽이 넓어진 날 이쪽만 좁은 채로 조용히 돈다.
SCAN_DIRS_RAW="$(sed -n 's/^SCAN_DIRS=(\(.*\))$/\1/p' "$SIZE_GATE")"
SCAN_DIRS_RAW="${SCAN_DIRS_RAW%%$'\n'*}"
[ -n "$SCAN_DIRS_RAW" ] || die "check-file-size.sh 에서 SCAN_DIRS 를 못 읽었다 — 무엇을 훑을지 정할 수 없다."
read -r -a SCAN_DIRS <<<"$SCAN_DIRS_RAW"
[ "${#SCAN_DIRS[@]}" -gt 0 ] || die "check-file-size.sh 의 SCAN_DIRS 가 비었다 — 훑을 트리가 없다."

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
#
# 찾기를 여기서 다시 쓰지 않고 공용 `resolve_judge` 를 부른다. 손으로 쓴 `[ -x ]` 판은
# **있기만 하면 쓴다** — 소스가 앞서 나간 낡은 바이너리도 그대로 쓴다. 그러면 옛 규칙으로
# 잰 값이 나오고, 이 게이트는 여유가 넉넉해서(파일 하나 몫) **그 값이 조용히 통과한다.**
# 실측: 판정기를 낡게 만들면 이 게이트는 rc=0 으로 값을 냈고, 같은 트리에서
# `resolve_judge` 를 쓰는 파일 SLOC 게이트는 rc=2(판정 불가)로 떨어졌다.
# `resolve_judge` 는 환경변수로 경로를 **줘도** `--check-fresh` 를 돌려 낡음을 없음으로 다룬다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT"
STRIP_BIN="$JUDGE_BIN"
[ -n "$STRIP_BIN" ] || die "strip-cfg-test 를 못 쓴다(없거나 낡았다) — cargo build -p tasty-doc-guards --bin strip-cfg-test"

STRIPPED="$(mktemp -d)"
trap 'rm -rf "$STRIPPED"' EXIT

"$STRIP_BIN" --neutralize-char-literal-quotes "$STRIPPED" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null \
    || die "출하 줄 판정 실패."
TOKEI_JSON="$(cd "$STRIPPED" && tokei --output json "${SCAN_DIRS[@]}")" || die "tokei 실행 실패."

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
# 죽고 `pipefail` 이 그것을 실패로 읽는다. 가드: crates/tasty-doc-guards/tests/no_early_exit_consumer_in_shell_pipes.rs
SUM="${REPORT%%$'\n'*}"
BREAKDOWN="${REPORT#*$'\n'}"
CEILING=$((BUDGET + SLACK))

if [ "$SUM" -gt "$CEILING" ]; then
    echo "동결 총합 래칫 위반: 동결 파일들의 출하 SLOC 합이 예산을 넘었다."
    echo "  합 $SUM  >  예산 $BUDGET + 띠 $SLACK = 천장 $CEILING"
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

# ── 합이 내려간 이유는 이 게이트가 모른다 ──────────────────────────────────
# 옛 문장은 "래칫을 조여라" 를 단정하고 곧바로 예산을 내리라고 했다. 형제 둘
# (`check-allow-reason.sh` · `check-shared-walk-ratchet.sh`)은 같은 자리에서 갈래를
# 열거하는데 이 게이트만 안 열고 있었다(실측 2026-09-08: 갈래 열거 줄 수 1·1·**0**).
#
# 예산은 되돌아 올라가지 않으므로, 틀린 갈래에서 내린 한 번이 영구히 그만큼을 안 보게
# 만든다. 그래서 원인을 정하지 않고 갈래를 연다.
#
# ★ 갈래 ㄴ 은 이 게이트에만 있다 — 형제 둘의 좌변에는 "목록" 이 없다. 실측
# (2026-09-08): `.complexity-file-allowlist` 에서 항목 하나(2008 줄짜리)를 빼면
# **두 게이트가 함께 rc=1** 이 되고, 이쪽 처방을 먼저 따르면 예산만 그 시점 값(36374)에서
# 34442 로 영구히 내려간 채 저쪽 위반이 그대로 남는다. 그래서 형제의 색을 먼저 보라고 말한다.
#
# ★ 갈래 ㄹ 은 실측에서 나왔다(2026-09-09). 계측기 `tokei` 14.0.0 은 문자 리터럴
# `'"'` 의 따옴표를 문자열의 시작으로 읽어 그 뒤 파일 끝까지를 문자열 안으로 보고,
# 문자열 안의 빈 줄을 code 로 센다 — 즉 `strip-cfg-test` 가 지운 줄이 다시 세어진다.
# `src/core/attach_runtime.rs` 에 그 형태 한 자리가 들어오자 이 게이트가 본 값이
# 1240 → 3046 이 됐는데 같은 구간의 원시 순증은 +364 였다. 사본을 뜰 때
# `--neutralize-char-literal-quotes` 로 그 자리만 중화해 고쳤고, 합이 38597 → 36355 로
# 제자리에 왔다. 오진은 **위쪽으로만** 나므로 고치면 값이 내려가고, 그 내려감은 ㄷ 이
# 아니라 ㄹ 이다.
#
# ★ ㄹ 의 판별법이 한 번 바뀌었다. 처음 적은 것은 "변이로 양방향을 봤는가" 였는데
# 그것은 **ㄷ 을 배제하지 못한다** — 특정 형태만 덜 세는 회귀라면 출하 +N 도 테스트
# 전용 +0 도 그대로 통과한다. 지금 거는 것은 (1) 오독 형태의 **최소 재현을 시험으로**
# 남겼는가와 (2) 내려감이 그 형태를 담은 파일에 **갇혀 있는가**이고, (2) 를 볼 수
# 있게 미달 갈래가 내역을 전량 찍는다. 독립 계측기를 상시 대조로 두는 안은 기각했지만,
# **그 기각 근거가 한 번 틀렸었다.** 처음 적은 것은 "차가 난 `render.rs` 에서는 tokei 가
# 옳았다(doc 코드펜스를 Markdown 으로 가르는 쪽이 맞다)" 였는데, 다시 재니 반대다.
#
# ★ 재측정(2026-09-09): 목록 23 중 **9 파일**이 갈리고 **전부 tokei 가 덜 세는 방향,
# 합 +25** 다. 원인은 코드펜스 분류가 아니라 **doc 주석에 딸린 줄의 누락**이다. 근거 셋:
# 9 파일 전부 doc 주석 줄(`///`·`//!`)만 지우면 두 계측이 **정확히** 일치하고, tokei 가
# 잃는 것은 code 만이 아니라 **같은 수의 blank** 이며(`render.rs` code 1982→1988 ·
# blank 2440→2446), embedded Markdown 의 code 는 **0** 이다. 빈 줄은 Markdown 코드로 갈릴
# 수 없고, 갈라 넘긴 코드가 0 인데 잃은 줄이 6 이면 그것은 분류가 아니라 누락이다.
#
# ★ 그래서 **이 게이트의 값에는 갈래 ㄷ 이 25 줄만큼 상시로 섞여 있다.** 그래도 예산
# 하향 판단은 안 뒤집힌다 — 예산도 같은 tokei 로 정해져 이 편향이 **양변에서 상쇄**되고,
# 신호가 되는 것은 편향의 **변동**(동결 파일에 doc 주석이 드나드는 것)뿐이다. 그 변동을
# 재는 데는 둘째 계측기가 필요 없다:
#     grep -vE '^[[:space:]]*(///|//!)' <사본 경로> >/tmp/nodoc.rs && tokei /tmp/nodoc.rs
# 로 doc 줄을 뺀 code 를 원래 code 와 견주면 그 차가 그 파일의 편향분이다.
#
# 재측정 뒤에도 상시 대조를 안 두는 이유는 남는다: 대조표는 "갈린다" 만 말하고 "누가
# 옳은가" 는 안 말한다 — 이번 판별을 지은 것도 대조표가 아니라 위 doc 제거 실험이다.
# 근거·대안(합 래칫 · 진단 열 포함)·재검토 조건:
# docs/adr/0258-the-measured-copy-is-neutralized-for-the-counter.md
if [ "$SUM" -lt "$BUDGET" ]; then
    echo "동결 총합 래칫: 합이 예산 아래로 내려갔다."
    echo "  합 $SUM  <  예산 $BUDGET"
    echo
    echo "★ 이 줄은 원인을 말하지 않는다 — 합이 내려가는 길이 넷이다."
    echo "  ㄱ 정말로 줄었다(동결 파일을 분해했거나 지웠다). 예산을 내린다."
    echo "  ㄴ 목록에서 항목이 빠졌다. 그 파일이 아직 임계를 넘으면 check-file-size.sh 가"
    echo "     같은 트리에서 함께 빨개진다 — **그쪽을 먼저 봐라.** 이쪽 처방을 먼저 따르면"
    echo "     예산만 영구히 내려간 채 저쪽 위반이 그대로 남는다."
    echo "  ㄷ 세는 술어가 깨졌다(tokei 가 덜 센다). 이때 값은 실제보다 작고, 그 상태에서"
    echo "     예산을 내리면 다음 회차부터 진짜 성장이 그 차이만큼 조용히 통과한다."
    echo "  ㄹ 세는 술어를 **고쳤다**(더 세던 오독을 없앴다). ㄷ 과 방향이 반대라 처방도"
    echo "     반대다 — 예산 자신이 그 오독으로 정해진 값이므로 새 값으로 내리는 것이 맞다."
    echo "     ★ ㄷ 과 가르는 것은 산문이 아니라 아래 둘이다. 둘 다 못 대면 ㄷ 으로 읽어라."
    echo "     (1) 오독의 **형태**를 지목하고 그 형태의 **최소 재현**을 시험으로 남겼는가 —"
    echo "         옳은 값이 눈으로 세어지는 크기여야 한다. 형태를 못 대면 그냥 ㄷ 이다."
    echo "     (2) 내려간 자리가 그 형태를 담은 파일에 **갇혀 있는가** — 아래 내역을 고치기"
    echo "         전 값과 견줘라. 형태가 없는 파일에서도 내려갔으면 ㄷ 이 섞인 것이다."
    echo "     변이 양방향(출하 +N · 테스트 전용 +0)은 온전성 확인일 뿐 ㄷ 을 배제하지"
    echo "     못한다 — 특정 형태만 덜 세는 회귀는 그 둘을 **모두 통과한다.**"
    echo
    echo "  지금 내역 (큰 것부터 전량 — 위 (2) 는 이 표를 고치기 전 값과 견주는 것이다):"
    while IFS=$'\t' read -r c p; do
        printf '    %6s  %s\n' "$c" "$p"
    done <<<"$BREAKDOWN"
    echo
    echo "ㄱ 이나 ㄹ 임을 확인한 뒤에만: .complexity-file-allowlist 의 예산 줄을 이 값으로 내린다(한 줄):"
    echo "      # frozen-sum-budget: $SUM"
    echo
    echo "  남는 여유는 곧 아무도 안 보는 구간이다. 줄인 만큼 예산도 줄여야 다음 성장이 보인다."
    exit 1
fi

# 한 이름이 두 물음에 답하지 않게 한다. **띠**($SLACK)는 상수 — 파일 하나가 새로 동결돼도
# 천장을 안 넘게 하는 폭이고 파일 SLOC 게이트의 THRESHOLD 다. **남은 여유**는 변수이고
# 판단에 쓰는 값이다. 통과줄이 식을 떼고 "여유 $SLACK" 만 남기면 그 상수가 남은 여유로
# 읽힌다 — 실제로 lane 이 924 를 보고하는데 이 줄은 1000 을 찍는 어긋남이 났다.
echo "동결 총합 래칫 통과 (합 $SUM / 천장 $CEILING = 예산 $BUDGET + 띠 $SLACK — 남은 여유 $((CEILING - SUM)))."
