#!/usr/bin/env bash
#
# 여유 0 래칫의 **기여분(delta)** 을 잰다 — 값이 아니라 delta 를 보고하기 위한 도구.
#
# 왜 값으로는 안 되는가: 래칫 값은 **그 lane 의 base 에서의 절대치**다. 두 lane 이
# 서로 다른 base 에서 각각 하나씩 늘려도 **둘 다 같은 값을 보고한다** — 그리고 병합
# 트리만 넘친다. 어느 lane 도 안 빨갛고 병합만 빨갛다. delta 는 base 와 무관하게
# 더해지므로, 조립하는 쪽이 Σdelta 로 병합 값을 **돌리기 전에 예측**할 수 있고,
# 예측과 실측이 어긋나면 그 어긋남 자체가 신호다(예측 못 한 상호작용).
#
# 왜 diff 를 grep 하지 않는가: 그 방식은 **양방향으로 틀린다**(실측).
#   · 더 찾음 — 사유 주석을 **붙여서** 넣은 `#[allow]` 은 게이트에 0 인데 diff 엔 1 이다.
#     문자열·주석 안의 억제 형태도 diff 엔 보이는데 게이트는 마스킹으로 지운다.
#   · 덜 찾음 — 사유 **주석만** 지우면 게이트는 +1 인데 억제 줄은 안 바뀌어 diff 는 0 이다.
# 즉 diff 의 바늘과 게이트의 바늘이 다른 것을 센다. 그래서 여기서는 **게이트를 양쪽
# 트리에서 실제로 돌리고 값을 뺀다** — 같은 판정기로 물어야 뺄셈이 성립한다.
#
# ★ 그 "같은 판정기" 가 이 스크립트가 지키는 핵심 조건이다. 판정기는 `target/` 에 사는
# 빌드 산출물이라 **새 워크트리에는 없고**, 없으면 게이트는 실패하지 않고 원문 세기로
# **폴백한다**. 폴백은 더 많이 센다. 그때 두 트리의 값 줄은 생김새가 똑같은데 답한 물음이
# 다르고, 뺄셈은 그 차이를 **기여분으로 보고한다**(실측 2026-09-07: 폴백한 base 가
# 57/182 대신 59/184 를 냈고, 그대로 뺐으면 없는 -2 를 보고할 뻔했다). 그래서 양쪽의
# 술어가 같은지를 먼저 묻고, 다르면 **판정 불가로 실패한다** — 통과가 아니다.
#
# ★ 이름이 `check-` 로 시작하지 않는 것은 의도다. `ls scripts/check-*.sh` 가 이 저장소의
# **게이트 발견 술어**이고, 그 출력을 그대로 루프에 먹이는 것이 규율이다. 이것은 게이트가
# 아니라 **측정 도구**라(인자 없이 부르면 판정이 아니라 사용법과 `exit 2` 다), 그 glob 에
# 들어가면 남의 루프에서 **빨간 게이트로 보인다**. 발견 술어를 안 더럽히는 쪽을 골랐다.
#
# ★ delta 가 **더해지는가**는 게이트마다 다르다 — Σdelta 로 병합을 예측하기 전에 물어라.
#   · 바늘이 **줄 단위**면 더해진다(`check-allow-reason.sh` 가 그렇다).
#   · 바늘의 좌변이 **파일 집합**이면 안 더해진다(`check-shared-walk-ratchet.sh` 가 그렇다 —
#     좌변이 `git ls-files` 로 고른 목록이라, 두 갈래가 **같은 파일을** 각각 고쳐 합쳐지면
#     delta 합과 실측이 어긋날 수 있다). 그 게이트에서 예측이 어긋나면 "예측 못 한
#     상호작용" 이라고 적기 전에 **같은 파일을 둘이 만졌는가**를 먼저 물어라.
#
# ☆ 이 도구는 base 와 HEAD **두 끝점**만 본다. 넣었다 뺀 것(상쇄된 0)과 안 건드린 0 을
#   구분하지 않는다 — 병합 예측에는 그 구분이 필요 없기 때문이다(누적 diff 는 최종 상태만
#   본다). 그 구분이 필요한 물음은 **lane 의 이력**이고, 그건 다른 계기로 재야 한다.
#
# ★ 잴 수 있는 게이트는 **값을 `… : <수>건 (상한 <수>)` 형식으로 찍는 것**뿐이다. 그 밖은
#   못 잰다고 말하고 실패한다 — 값을 지어내지 않는다. 그리고 판정기를 `scripts/lib/judge-bin.sh`
#   의 `resolve_judge` 로 찾지 않는 게이트는 환경변수를 안 읽고 자기 트리의 `target/` 만 보므로
#   **새 워크트리에서 그냥 죽는다**(그때도 값이 없으니 못 잰다고 나온다).
#
# 사용: scripts/gate-delta.sh <base-rev> [게이트 스크립트...]
#   기본 게이트: check-shared-walk-ratchet.sh · check-allow-reason.sh (여유 0 인 둘)
#
# 이 스크립트는 게이트의 **본 판정 경로를 건드리지 않는다** — 게이트를 부르기만 한다.
# 여유 0 인 래칫이라 판정을 만지는 것 자체가 위험이다.

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
BASE_SHA="$(git rev-parse "$BASE_REV")"
HEAD_SHA="$(git rev-parse HEAD)"

# 폴백 문구. 게이트들이 판정기 없이 원문에서 셀 때 표준오류로 내는 표지다. 값이 아니라
# **자리**로 묻는다 — 수를 세면 문구가 늘거나 줄 때 조용히 틀린다.
FALLBACK_MARK='원문에서 센다'

WORK=""
cleanup() {
    if [ -n "$WORK" ] && [ -d "$WORK" ]; then
        git worktree remove --force "$WORK" >/dev/null 2>&1 || true
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

WORK="$(mktemp -d)/base"
git worktree add --detach "$WORK" "$BASE_SHA" >/dev/null 2>&1

# 판정기를 base 트리에 넘긴다. base 에는 `target/` 이 없어서 이것 없이는 반드시 폴백한다.
# 넘긴다고 반드시 쓰이는 것은 아니다 — 판정기가 base 의 판정기 소스로 지어진 것이
# 아니면 `resolve_judge` 가 **낡음으로 보고 안 쓴다**. 그 경우는 아래에서 걸린다.
for _n in mask-source strip-cfg-test; do
    _v="TASTY_$(printf '%s' "$_n" | tr 'a-z-' 'A-Z_')_BIN"
    for _p in "$ROOT/target/debug/$_n" "$ROOT/target/release/$_n"; do
        if [ -x "$_p" ] && [ -z "$(eval "printf '%s' \"\${$_v:-}\"")" ]; then
            export "$_v=$_p"
            break
        fi
    done
done

# 값 줄에서 수를 뽑는다. 두 게이트 모두 `… : <수>건 (상한 <수>)` 형식이다.
read_value() {
    # 파이프를 안 쓴다. `set -o pipefail` 아래에서 `| head -1` 은 앞 단을 SIGPIPE 로
    # 죽여 파이프라인 전체를 실패로 만든다(루트의 `no_early_exit_consumer_in_shell_pipes`
    # 가 이 형태를 잡는다). here-string 으로 먹이고 첫 줄은 셸에서 자른다.
    _rv_all=$(sed -n 's/.*: \([0-9]\{1,\}\)건 (상한 \([0-9]\{1,\}\)).*/\1 \2/p' <<<"$1")
    printf '%s' "${_rv_all%%$'\n'*}"
}

status=0
measured=0
printf '%-34s %8s %8s %8s   %s\n' "게이트" "base" "HEAD" "delta" "상한"
for g in "${GATES[@]}"; do
    [ -f "$ROOT/scripts/$g" ] || { echo "[delta] 없는 게이트: $g" >&2; exit 2; }

    h_out=$(cd "$ROOT" && bash "scripts/$g" 2>&1) || true
    b_out=$(cd "$WORK" && bash "scripts/$g" 2>&1) || true

    h_fb=0; b_fb=0
    case "$h_out" in *"$FALLBACK_MARK"*) h_fb=1;; esac
    case "$b_out" in *"$FALLBACK_MARK"*) b_fb=1;; esac

    if [ "$h_fb" -ne "$b_fb" ]; then
        echo "[delta] $g — 두 트리가 **다른 술어**로 돌았다(HEAD 폴백=$h_fb, base 폴백=$b_fb)." >&2
        echo "        값 줄은 같은 모양인데 답한 물음이 다르다. 뺄셈이 성립을 안 한다." >&2
        echo "        판정기를 base 의 판정기 소스로 지어라:" >&2
        echo "          git worktree add --detach <경로> $BASE_SHA" >&2
        echo "          (그 트리에서) cargo build -p tasty-doc-guards --bin mask-source" >&2
        echo "          TASTY_MASK_SOURCE_BIN=<그 경로> scripts/gate-delta.sh $BASE_REV" >&2
        status=1
        continue
    fi

    h_v=$(read_value "$h_out"); b_v=$(read_value "$b_out")
    if [ -z "$h_v" ] || [ -z "$b_v" ]; then
        echo "[delta] $g — 값을 못 읽었다(HEAD='$h_v' base='$b_v'). 이 도구는 **값을 안 낸다**." >&2
        echo "        이 도구가 읽는 것은 \`… : <수>건 (상한 <수>)\` 형식 한 가지다. 사유는 셋 중 하나다:" >&2
        echo "        ① 그 게이트가 다른 형식으로 값을 찍는다(예: 합/예산/여유) — 이 도구로는 못 잰다." >&2
        echo "        ② 그 게이트가 값을 아예 안 찍는다(순수 통과/실패) — 잴 것이 없다." >&2
        echo "        ③ base 쪽에서 게이트가 죽었다. 판정기를 \`scripts/lib/judge-bin.sh\` 의" >&2
        echo "           resolve_judge 로 안 찾는 게이트는 환경변수를 안 읽고 자기 트리의" >&2
        echo "           target/ 만 보므로, 새 워크트리에서 그냥 죽는다." >&2
        status=1
        continue
    fi
    measured=1
    hc=${h_v%% *}; cap=${h_v##* }; bc=${b_v%% *}; bcap=${b_v##* }
    d=$((hc - bc))
    note=""
    [ "$h_fb" -eq 1 ] && note=" ★ 양쪽 다 폴백(원문 세기) — 더 많이 세는 값끼리의 뺄셈이다"
    # 상한이 움직였는가. **여유 0 인 양방향 래칫에서는 이것이 delta 보다 먼저 오는 물음이다** —
    # 그런 게이트는 `rc=0 ⟺ 값 == 상한` 이고 상한은 커밋된 상수라, 상한을 안 건드린 갈래의
    # rc=0 은 **정의상** delta 0 이다(그때 이 도구는 잉여다). 반대로 상한이 움직였으면
    # 양쪽 rc=0 이어도 값이 옮겨간 것이라, 그때부터 delta 가 필요하다.
    if [ "$cap" != "$bcap" ]; then
        note="$note ★ 상한이 움직였다($bcap → $cap) — rc 만으로는 값이 안 고정된다"
    elif [ "$hc" = "$cap" ] && [ "$bc" = "$bcap" ]; then
        note="$note ☆ 양끝이 상한에 붙어 있고 상한이 그대로다 — 이 게이트는 rc 로 충분했다"
    fi
    printf '%-34s %8s %8s %+8d   %s%s\n' "$g" "$bc" "$hc" "$d" "$cap" "$note"
done

echo
echo "base $BASE_SHA · HEAD $HEAD_SHA · dirty $(git status --porcelain | wc -l | tr -d ' ')"
if [ "$measured" -eq 1 ]; then
    echo "계기: **재측정(확정)** — 양쪽 트리에서 게이트를 그대로 다시 돌려 뺐다."
    echo "      (diff 를 grep 해서 낸 delta 는 확정이 아니라 **하한**이다 — 이 도구의 값과 섞어 적지 마라.)"
else
    echo "계기: **없다** — 위에서 아무 값도 못 얻었다. 이 줄을 delta 0 으로 적지 마라."
fi
echo "보고 형식: rc + 값 + delta(계기) + 어느 상태에서 뽑았는지(tip · dirty) + 게이트별 tip."
exit "$status"
