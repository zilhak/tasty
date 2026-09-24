#!/usr/bin/env bash
# 공통 조상과 현재 작업 트리에서 두 개수 검사를 실행하고 차이를 출력한다.
# 비교용 detached worktree를 만들었다가 삭제한다. 제품 테스트를 대신하지 않는다.
# 사용: bash scripts/gate-delta.sh <base-rev> [검사 스크립트...]. 기본 검사는 shared-walk와 allow-reason이다.
# 변경 사이의 상호작용이 있으면 이 차이가 개별 변경의 기여분과 같다고 보장할 수 없다.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ $# -lt 1 ]; then
    echo "사용: scripts/gate-delta.sh <base-rev> [게이트 스크립트...]" >&2
    exit 2
fi
BASE_REV="$1"; shift
if [ $# -gt 0 ]; then
    GATES=("$@")
else
    GATES=(check-shared-walk-ratchet.sh check-allow-reason.sh)
fi

if ! git rev-parse --verify --quiet "${BASE_REV}^{commit}" >/dev/null; then
    echo "[delta] base 를 못 찾는다: $BASE_REV" >&2
    exit 2
fi
# 공유 브랜치에만 추가된 변경을 역방향 차이로 세지 않도록 공통 조상을 사용한다.
BASE_SHA="$(git merge-base HEAD "$BASE_REV" 2>/dev/null || true)"
if [ -z "$BASE_SHA" ]; then
    echo "[delta] HEAD 와 '$BASE_REV' 의 공통 조상을 못 찾는다" >&2
    exit 2
fi
if [ "$BASE_SHA" != "$(git rev-parse "$BASE_REV")" ]; then
    echo "[delta] '$BASE_REV' 가 $(git rev-list --count "${BASE_SHA}..${BASE_REV}") 커밋 앞서 있어" \
         "갈린 지점 $(git rev-parse --short=9 "$BASE_SHA") 을 base 로 쓴다" >&2
fi
HEAD_SHA="$(git rev-parse HEAD)"


WORK=""
cleanup() {
    if [ -n "$WORK" ] && [ -d "$WORK" ]; then
        git worktree remove --force "$WORK" >/dev/null 2>&1 || true
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

WORK="$(mktemp -d)/base"
if ! wt_out=$(git worktree add --detach "$WORK" "$BASE_SHA" 2>&1); then
    echo "[delta] 비교용 worktree를 만들지 못했다: git worktree add --detach $WORK $BASE_SHA" >&2
    printf '%s\n' "$wt_out" >&2
    exit 2
fi

# 비교 트리에 빌드 산출물이 없으므로 보조 도구 경로를 전달한다. 각 검사에서 소스와 최신성을 다시 대조한다.
for _n in mask-source strip-cfg-test; do
    _v="TASTY_$(printf '%s' "$_n" | tr 'a-z-' 'A-Z_')_BIN"
    for _p in "$ROOT/target/debug/$_n" "$ROOT/target/release/$_n"; do
        if [ -x "$_p" ] && [ -z "$(eval "printf '%s' \"\${$_v:-}\"")" ]; then
            export "$_v=$_p"
            break
        fi
    done
done

# 두 검사에서 출력하는 … : <수>건 (상한 <수>) 형식을 읽는다.
read_value() {
    # head로 producer를 조기 종료하지 않도록 전체 문자열을 받아 파싱한다.
    _rv_all=$(sed -n 's/.*: \([0-9]\{1,\}\)건 (상한 \([0-9]\{1,\}\)).*/\1 \2/p' <<<"$1")
    printf '%s' "${_rv_all%%$'\n'*}"
}

show_gate_output() {  # <label> <rc> <output>
    local label="$1" rc="$2" out="$3" keep=30 total
    local -a lines
    mapfile -t lines <<<"$out"
    total=${#lines[@]}
    echo "        --- $label 출력 (rc=$rc, ${total} 줄$([ "$total" -gt "$keep" ] && printf ' 중 마지막 %s' "$keep")) ---" >&2
    local start=$(( total > keep ? total - keep : 0 ))
    printf '        | %s\n' "${lines[@]:start}" >&2
}

status=0
measured=0
refused=0
printf '%-34s %8s %8s %8s %-4s   %s\n' "게이트" "base" "HEAD" "delta" "등급" "상한"
for g in "${GATES[@]}"; do
    [ -f "$ROOT/scripts/$g" ] || { echo "[delta] 없는 게이트: $g" >&2; exit 2; }

    if h_out=$(cd "$ROOT" && bash "scripts/$g" 2>&1); then h_rc=0; else h_rc=$?; fi
    if b_out=$(cd "$WORK" && bash "scripts/$g" 2>&1); then b_rc=0; else b_rc=$?; fi

    # 판정 불가(rc 2)는 개수 차이로 해석하지 않는다.
    if [ "$h_rc" -eq 2 ] || [ "$b_rc" -eq 2 ]; then
        echo "[delta] $g — 게이트가 판정을 거부했다(HEAD rc=$h_rc, base rc=$b_rc)." >&2
        echo "        rc=2는 판정 불가이므로 개수 차이를 계산하지 않는다." >&2
        echo "        흔한 원인은 보조 도구가 없거나 해당 트리의 소스로 빌드되지 않은 것이다." >&2
        echo "        (검사 대상이 비었거나 미추적 타깃이 있어도 같은 rc 로 나온다 — 그 경우" >&2
        echo "         각 검사의 출력에서 원인을 확인해라.)" >&2
        echo "        보조 도구 문제라면 base의 소스로 도구를 빌드해라:" >&2
        echo "          git worktree add --detach <경로> $BASE_SHA" >&2
        echo "          (그 트리에서) cargo build -p tasty-doc-guards --bin mask-source" >&2
        echo "          TASTY_MASK_SOURCE_BIN=<그 경로> scripts/gate-delta.sh $BASE_REV" >&2
        show_gate_output "HEAD" "$h_rc" "$h_out"
        show_gate_output "base" "$b_rc" "$b_out"
        refused=$((refused + 1))
        status=1
        continue
    fi

    h_v=$(read_value "$h_out"); b_v=$(read_value "$b_out")
    if [ -z "$h_v" ] || [ -z "$b_v" ]; then
        echo "[delta] $g — 값을 못 읽었다(HEAD='$h_v' base='$b_v'). 차이를 출력하지 않는다." >&2
        echo "        이 도구가 읽는 것은 \`… : <수>건 (상한 <수>)\` 형식 한 가지다. 판정 불가(rc=2)는" >&2
        echo "        위에서 이미 갈렸으므로, 출력 형식이나 실행 실패를 확인해라. 예를 들면:" >&2
        echo "        ① 그 게이트가 다른 형식으로 값을 찍는다(예: 합/예산/여유) — 이 도구로는 못 잰다." >&2
        echo "        ② 그 게이트가 값을 아예 안 찍는다(순수 통과/실패) — 잴 것이 없다." >&2
        echo "        ③ base 쪽 검사 실행이 실패했다. 보조 도구를 \`scripts/lib/judge-bin.sh\` 의" >&2
        echo "           resolve_judge 로 안 찾는 게이트는 환경변수를 안 읽고 자기 트리의" >&2
        echo "           target/ 만 보므로, 새 worktree에서 도구를 찾지 못할 수 있다." >&2
        show_gate_output "HEAD" "$h_rc" "$h_out"
        show_gate_output "base" "$b_rc" "$b_out"
        refused=$((refused + 1))
        status=1
        continue
    fi
    measured=$((measured + 1))
    hc=${h_v%% *}; cap=${h_v##* }; bc=${b_v%% *}; bcap=${b_v##* }
    d=$((hc - bc))
    note=""
    # 상한이 바뀌면 개수 차이만 보고 변경의 영향을 판단할 수 없다.
    if [ "$cap" != "$bcap" ]; then
        note="$note ★ 상한이 움직였다($bcap → $cap) — rc 만으로는 값이 안 고정된다"
    elif [ "$hc" = "$cap" ] && [ "$bc" = "$bcap" ]; then
        note="$note ☆ 양끝이 상한에 붙어 있고 상한이 그대로다 — 이 게이트는 rc 로 충분했다"
    fi
    printf '%-34s %8s %8s %+8d [확정]   %s%s\n' "$g" "$bc" "$hc" "$d" "$cap" "$note"
done

echo
echo "base $BASE_SHA · HEAD $HEAD_SHA · dirty $(git status --porcelain | wc -l | tr -d ' ')"
echo "게이트 요청 ${#GATES[@]} · 잰 것 ${measured} · 못 잰 것 ${refused}"
if [ "${#GATES[@]}" -eq 0 ]; then
    echo "[delta] 잴 게이트가 하나도 없다 — 목록이 비었다. 통과가 아니라 실패다." >&2
    status=1
fi

if [ "$measured" -ge 1 ]; then
    echo "측정 방법: 두 트리에서 검사 스크립트를 실행해 개수 차이를 계산했다."
    echo "      (diff 텍스트에서 센 값과 측정 방법이 다르므로 구분해서 기록해라.)"
else
    echo "측정값을 얻지 못했다. 이 결과를 delta 0으로 기록하지 마라."
fi
echo "보고할 항목: 종료 코드, 측정값과 차이, 비교한 커밋, 미커밋 변경 여부, 실행한 검사."
exit "$status"
