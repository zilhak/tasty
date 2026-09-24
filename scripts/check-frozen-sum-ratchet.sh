#!/usr/bin/env bash
# 예외 목록 파일들의 test 전용이 아닌 code SLOC 합을 예산과 비교한다.
# 파일별 증감이 상쇄되면 총합으로는 발견하지 못한다. 사본 조건·검색 범위·허용 폭은 check-file-size.sh에서 읽는다.
# doc 주석에 따른 tokei 측정 차이와 최소 재현 결과를 먼저 확인한 뒤 합을 비교한다.
# 정책·보정 근거: docs/dev-guide/complexity-gate.md#계측용-사본과-측정값-보정

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ALLOWLIST="$ROOT/.complexity-file-allowlist"
SIZE_GATE="$ROOT/scripts/check-file-size.sh"

die() { echo "$1"; echo "(측정이 안 됐으므로 게이트를 통과로 읽지 않는다)"; exit 2; }

# 추가 허용 폭은 파일 크기 검사의 THRESHOLD를 사용한다. 남은 여유와는 다르다.
SLACK="$(sed -n 's/^THRESHOLD=\([0-9][0-9]*\)$/\1/p' "$SIZE_GATE")"
SLACK="${SLACK%%$'\n'*}"
[ -n "$SLACK" ] || die "check-file-size.sh 에서 THRESHOLD 를 못 읽었다 — 허용 폭을 정할 수 없다."

# 사본 생성과 측정이 파일 크기 검사와 같은 디렉터리를 보도록 설정을 읽는다.
SCAN_DIRS_RAW="$(sed -n 's/^SCAN_DIRS=(\(.*\))$/\1/p' "$SIZE_GATE")"
SCAN_DIRS_RAW="${SCAN_DIRS_RAW%%$'\n'*}"
[ -n "$SCAN_DIRS_RAW" ] || die "check-file-size.sh 에서 SCAN_DIRS 를 못 읽었다 — 무엇을 훑을지 정할 수 없다."
read -r -a SCAN_DIRS <<<"$SCAN_DIRS_RAW"
[ "${#SCAN_DIRS[@]}" -gt 0 ] || die "check-file-size.sh 의 SCAN_DIRS 가 비었다 — 훑을 트리가 없다."

# 같은 파일이 두 검사에서 다르게 측정되지 않도록 사본 생성 플래그도 공유한다.
JUDGE_FLAGS_RAW="$(sed -n 's/^SHIPPING_JUDGE_FLAGS=(\(.*\))$/\1/p' "$SIZE_GATE")"
JUDGE_FLAGS_RAW="${JUDGE_FLAGS_RAW%%$'\n'*}"
[ -n "$JUDGE_FLAGS_RAW" ] || die "check-file-size.sh 에서 SHIPPING_JUDGE_FLAGS 를 못 읽었다 — 어떻게 판정할지 정할 수 없다."
read -r -a JUDGE_FLAGS <<<"$JUDGE_FLAGS_RAW"
[ "${#JUDGE_FLAGS[@]}" -gt 0 ] || die "check-file-size.sh 의 SHIPPING_JUDGE_FLAGS 가 비었다 — 판정 방식이 없다."

BUDGET="$(sed -n 's/^#[[:space:]]*frozen-sum-budget:[[:space:]]*\([0-9][0-9]*\)[[:space:]]*$/\1/p' "$ALLOWLIST")"
BUDGET="${BUDGET%%$'\n'*}"
[ -n "$BUDGET" ] || die ".complexity-file-allowlist 에 '# frozen-sum-budget: <수>' 줄이 없다."

# doc 주석 제거 전후의 tokei 측정 차이를 고정값과 비교한다.
BIAS_PINNED="$(sed -n 's/^#[[:space:]]*doc-comment-bias:[[:space:]]*\([0-9][0-9]*\)[[:space:]]*$/\1/p' "$ALLOWLIST")"
BIAS_PINNED="${BIAS_PINNED%%$'\n'*}"
[ -n "$BIAS_PINNED" ] || die ".complexity-file-allowlist 에 '# doc-comment-bias: <수>' 줄이 없다."

command -v tokei >/dev/null 2>&1 || die "tokei 미설치: cargo install tokei"

PY=""
for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1 && [ "$("$cand" -c 'print(1)' 2>/dev/null)" = "1" ]; then
        PY="$cand"; break
    fi
done
[ -n "$PY" ] || die "python 미설치: tokei JSON 파싱에 python3 필요"

# 중첩 Cargo 잠금을 피하려고 이미 빌드한 도구만 찾고 최신성을 확인한다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT"
STRIP_BIN="$JUDGE_BIN"
[ -n "$STRIP_BIN" ] || die "strip-cfg-test 를 못 쓴다(없거나 낡았다) — cargo build -p tasty-doc-guards --bin strip-cfg-test"

STRIPPED="$(mktemp -d)"
trap 'rm -rf "$STRIPPED"' EXIT

"$STRIP_BIN" "${JUDGE_FLAGS[@]}" "$STRIPPED" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null \
    || die "출하 줄 판정 실패."

# 예외 목록 파일에 한해 doc 주석 줄을 뺀 추가 사본을 만든다. tokei는 두 사본을 같은 호출로 측정한다.
while IFS= read -r p; do
    case "$p" in ''|'#'*) continue ;; esac
    [ -f "$STRIPPED/$p" ] || continue
    mkdir -p "$STRIPPED/__nodoc/$(dirname "$p")"
    grep -vE '^[[:space:]]*(///|//!)' "$STRIPPED/$p" >"$STRIPPED/__nodoc/$p" || true
done <"$ALLOWLIST"

# 이 입력의 code를 1로 세는 tokei 동작을 감시한다. 결과가 바뀌면 기존 측정 보정을 다시 검토한다.
mkdir -p "$STRIPPED/__probe"
printf 'const A: &str = "x";\n/// doc\nconst B: u32 = 1;\n' >"$STRIPPED/__probe/probe.rs"

TOKEI_JSON="$(cd "$STRIPPED" && tokei --output json "${SCAN_DIRS[@]}" __nodoc __probe)" \
    || die "tokei 실행 실패."

# 목록의 파일이 실제로 있는데 측정 보고에서 빠졌으면 오류다. 삭제된 파일은 합에서 제외한다.
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
probe = sizes.get("__probe/probe.rs")
if probe is None:
    print("형태 프로브가 보고에 없다 — 측정 실패로 읽는다", file=sys.stderr); sys.exit(3)
root = os.environ["ROOT"]
entries, missing, unmeasured, total, bias = [], [], [], 0, 0
for line in open(os.environ["ALLOWLIST"], encoding="utf-8"):
    p = line.strip()
    if not p or p.startswith("#"):
        continue
    if p in sizes:
        nodoc = sizes.get("__nodoc/" + p)
        if nodoc is None:
            unmeasured.append(p); continue
        b = nodoc - sizes[p]
        entries.append((sizes[p], b, p)); total += sizes[p]; bias += b
    elif os.path.exists(os.path.join(root, p)):
        missing.append(p)
if missing:
    print("목록의 파일이 디스크에 있는데 보고에 없다 — 측정 실패로 읽는다: "
          + ", ".join(sorted(missing)[:5]), file=sys.stderr)
    sys.exit(3)
if unmeasured:
    print("doc 줄을 뺀 사본이 보고에 없다 — 편향을 못 잰다: "
          + ", ".join(sorted(unmeasured)[:5]), file=sys.stderr)
    sys.exit(3)
entries.sort(reverse=True)
print(total)
print(bias)
print(probe)
for c, b, p in entries:
    print(f"{c}\t{b}\t{p}")
')" || die "동결 합계 측정 실패."

# head로 파이프를 조기 종료하지 않고 받은 보고 문자열을 나눈다.
REST="$REPORT"
SUM="${REST%%$'\n'*}"; REST="${REST#*$'\n'}"
BIAS="${REST%%$'\n'*}"; REST="${REST#*$'\n'}"
PROBE_CODE="${REST%%$'\n'*}"
BREAKDOWN="${REST#*$'\n'}"
CEILING=$((BUDGET + SLACK))

# 최소 재현의 측정 방식이 달라졌으면 기존 예산과 바로 비교하지 않는다.
if [ "$PROBE_CODE" != "1" ]; then
    echo "동결 총합 래칫: 최소 재현의 측정 결과가 기존 형태와 달라졌다."
    echo "  최소 재현(3 줄)에서 tokei 가 센 code = $PROBE_CODE (기존 기대값 1)"
    echo
    echo "  그 세 줄은 이렇다 — code 줄은 둘이고, doc 주석 바로 앞줄이 문자열 리터럴이다:"
    echo "      const A: &str = \"x\";"
    echo "      /// doc"
    echo "      const B: u32 = 1;"
    echo
    echo "  2가 나왔다면 이 최소 재현의 오독이 사라졌다. 전체 파일의 편향도 다시 확인하고"
    echo "  측정 방식에 따른 차이를 코드 증가와 구분해라. 편향과 예산은 같은"
    echo "  폭으로 함께 옮기고, 아래 가이드에 따라 보정 방식을 다시 검토한다."
    echo "  1도 2도 아니면 다른 형태의 측정 차이가 생겼다. 편향을"
    echo "  갱신하기 전에 새 오독을 최소 예제로 재현하고 보정 방식을 다시 검토한다."
    echo "  근거: docs/dev-guide/complexity-gate.md#계측용-사본과-측정값-보정"
    exit 1
fi

# 편향 변경은 코드 증감과 구분한다. 예산도 같은 폭으로 보정해야 한다.
if [ "$BIAS" -ne "$BIAS_PINNED" ]; then
    DELTA=$((BIAS - BIAS_PINNED))
    echo "동결 총합 래칫: 계측기의 doc 주석 편향이 움직였다."
    echo "  편향 $BIAS  (고정값 $BIAS_PINNED · 차 $DELTA)"
    echo
    echo "이 편향 차이 자체는 코드 증가를 뜻하지 않는다. 합과 예산은 tokei로 측정하며,"
    echo "  편향이 $DELTA만큼 바뀌면 합은 그만큼"
    echo "  반대로 움직이는데 실제 코드는 한 줄도 안 변했을 수 있다."
    echo
    echo "  편향과 예산을 같은 폭으로 보정해라 (.complexity-file-allowlist):"
    echo "      # frozen-sum-budget: $((BUDGET - DELTA))"
    echo "      # doc-comment-bias: $BIAS"
    echo "  예산을 합($SUM)으로 맞추지 마라 — 그러면 같은 커밋에 섞인 진짜 성장까지"
    echo "  함께 허용하게 된다. 보정 폭은 $DELTA이며, 이후 합을 다시 검사해라."
    echo
    echo "  파일별 편향 (0 이 아닌 것만 — 이 파일들에서 doc 주석 제거 전후 측정값이 다르다):"
    while IFS=$'\t' read -r c b q; do
        [ "$b" = "0" ] || printf '    %6s  %s\n' "$b" "$q"
    done <<<"$BREAKDOWN"
    echo
    echo "  보정 방식: docs/dev-guide/complexity-gate.md#계측용-사본과-측정값-보정"
    exit 1
fi

if [ "$SUM" -gt "$CEILING" ]; then
    echo "동결 총합 래칫 위반: 예외 목록 파일들의 test 전용이 아닌 SLOC 합이 예산을 넘었다."
    echo "  합 $SUM  >  예산 $BUDGET + 띠 $SLACK = 천장 $CEILING"
    echo
    echo "  큰 것부터:"
    shown=0
    while IFS=$'\t' read -r c b p; do
        shown=$((shown + 1))
        [ "$shown" -gt 8 ] && break
        printf '    %6s  %6s  %s\n' "$c" "$b" "$p"
    done <<<"$BREAKDOWN"
    echo
    echo "상한 초과만으로 현재 커밋이 복잡도 증가의 원인이라고 단정할 수 없다."
    echo "  파일별 합계이므로 분할로 일부 파일의 개수가 줄어도 전체 합은 늘 수 있다."
    echo "  파일별 변경 이력은 다음 명령으로 확인해라:"
    echo "    git log --format='%h %s' --numstat -- \$(grep -v '^#' .complexity-file-allowlist)"
    echo
    echo "  할 일은 둘 중 하나다."
    echo "  - 위 파일 중 하나를 분해해 합을 $CEILING 이하로 내린다 ."
    echo "  - 이 커밋이 .complexity-file-allowlist 에 **항목을 새로 추가**하는 커밋이라면,"
    echo "    추가가 정당한지 검토한 뒤 예산 줄도 갱신한다:"
    echo "      # frozen-sum-budget: $SUM"
    exit 1
fi

# 합 감소만으로 개선이라고 결론내리지 않는다. 목록·수집·계측 변화도 확인해야 한다.
if [ "$SUM" -lt "$BUDGET" ]; then
    echo "동결 총합 래칫: 합이 예산 아래로 내려갔다."
    echo "  합 $SUM  <  예산 $BUDGET"
    echo
    echo "합 감소는 원인을 말하지 않는다. 다음 네 경우를 구분해라."
    echo "  ㄱ 실제 코드를 줄이거나 파일을 삭제했다면 예산을 내린다."
    echo "  ㄴ 목록에서 항목을 뺐다면 check-file-size.sh도 확인해라. 여전히 임계를 넘으면"
    echo "     파일 크기 검사에 실패하므로 먼저 그 원인을 해결해야 한다. 그렇지 않으면"
    echo "     예산만 영구히 내려간 채 저쪽 위반이 그대로 남는다."
    echo "  ㄷ 수집·측정 오류로 적게 센 경우에는 예산을 내리지 마라. 잘못된 값으로"
    echo "     예산을 조정하기 전에 오류를 고치고 다시 측정해라."
    echo "  ㄹ 기존 과다 집계를 고쳤다면 예산도 보정할 수 있다."
    echo "     기존 예산 역시 과다 집계된 값으로 정했기 때문이다."
    echo "     측정 오류와 올바른 보정을 구분하려면 아래 두 근거가 필요하다."
    echo "     (1) 오독 형태를 보여 주는 최소 재현을 시험으로 남겼는가 —"
    echo "         기대값을 직접 셀 수 있는 작은 입력으로 확인해라."
    echo "     (2) 감소한 파일 모두에 해당 형태가 있는가 — 아래 내역을 고치기"
    echo "         전 값과 비교해라. 다른 파일도 감소했다면 별도 원인을 조사해라."
    echo "     일반 코드 +N·test 전용 +0 변이만으로는 특정 형태의 과소 집계를 배제하지"
    echo "     못한다. 해당 형태의 독립된 최소 재현도 필요하다."
    echo
    echo "  지금 내역 (큰 것부터 전량 — 위 (2) 는 이 표를 고치기 전 값과 견주는 것이다):"
    while IFS=$'\t' read -r c b p; do
        printf '    %6s  %6s  %s\n' "$c" "$b" "$p"
    done <<<"$BREAKDOWN"
    echo
    echo "실제 코드 감소나 올바른 측정 보정임을 확인한 뒤에만: .complexity-file-allowlist 의 예산 줄을 이 값으로 내린다(한 줄):"
    echo "      # frozen-sum-budget: $SUM"
    echo
    echo "  예산을 그대로 두면 줄어든 폭만큼 새 증가를 허용하므로 예산도 맞춰라."
    exit 1
fi

# 고정 허용 폭과 현재 남은 여유를 구분해 출력한다.
echo "동결 총합 래칫 통과 (합 $SUM / 천장 $CEILING = 예산 $BUDGET + 띠 $SLACK — 남은 여유 $((CEILING - SUM)))."
echo "  계측 편향 $BIAS (고정값 $BIAS_PINNED · 여유 0) — 이 보정 범위에서 doc 주석을 뺀 사본의 합과 그만큼 차이 난다."
