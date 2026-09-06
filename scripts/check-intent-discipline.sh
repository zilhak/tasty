#!/usr/bin/env bash
# Intent 도메인 직접 호출 패턴 금지 체크 — popup / preset / surface / tab / pane /
# workspace 도메인의 **변이** API.
#
# 정책 근거: docs/design/flows/action-dispatch.md
# 채널: .github/workflows/script-gates.yml (main push · PR)
#
# ── 종료 코드: 0 통과 · 1 위반 · **2 판정 불가** ──────────────────────────
# 2 를 고른 것은 다수결이 아니라 **근거의 유무**로 갈랐다. 이 레포의 게이트들이 판정
# 불가에 쓰는 코드가 1 · 2 · 3 으로 흩어져 있는데, 근거가 글로 적힌 것은 2 뿐이다
# (scripts/check-file-size.sh: 측정 실패는 위반과도 구분한다). 3 은 한 자리뿐이고
# 근거가 없다. 1 은 위반과 안 갈려서 CI 에서 "이 회차에 무엇을 했는가" 가 안 나온다.
# 그리고 이 파일은 **이미 같은 뜻으로 2 를 쓴다**(아래 면제 경로 실재 검사 — 그쪽도
# "조용히 무시되고 쌓인다" 를 막는 자리다). 새 표기를 만들지 않는다: 같은 물음의 답이
# 표기마다 흩어지면 다음 게이트를 쓰는 사람이 규칙을 **볼 수는 있어도 복사할 대상을
# 못 고른다.**
#
# ── 술어의 성질 (되돌리지 마라) ──────────────────────────────────────────
# 이 검사는 **텍스트 스캔**이라 타입을 모른다. 그래서 오탐이 나는 자리를 세 가지로
# 나눠 각각 다르게 막는다. 셋을 하나로 합치면(예: 파일 통째 제외) 그 파일의 다른
# 위반까지 같이 사라진다.
#
#   ① 코드가 아닌 것        → 줄 주석·블록 주석·문자열·문자 리터럴을 덮은 **사본**에서
#                             찾는다. 그 마스킹은 이 스크립트가 아니라 판정기
#                             (`mask-source`)가 한다 — 같은 물음에 답을 둘로 만들지
#                             않으려는 것이다. 사유 주석은 원본에서 읽는다(주석은
#                             사본에 없다).
#   ② 도메인이 피험자인 것  → `#[cfg(test)] mod` 본문과 `*_tests.rs` 는 대상이 아니다.
#                             테스트가 popup 을 직접 열고 닫는 것은 popup 이 **피험자**
#                             이기 때문이다. 규율은 "도메인을 도구로 쓸 때" 의 규칙이다.
#   ③ 이름만 같은 것        → 패턴별 면제 경로(`EXEMPT_<패턴>`). 파일 통째가 아니라
#                             **그 패턴에 대해서만** 면제한다.
#
# 면제 경로가 실재하지 않으면 **그 자리에서 실패한다.** 예전에 트리를 재조직하면서
# 면제 경로 다섯이 죽었는데 아무도 몰랐다 — 없는 경로는 조용히 무시되고, 그 파일들의
# 정당한 호출이 위반으로 쌓였다.
#
# `// intent-exempt: <사유>` 는 **같은 줄 · 바로 위 줄 · 바로 아래 줄** 에서 인정한다.
# 여러 줄에 걸친 호출에서 사유를 어디에 적는지가 사람마다 다르고, 셋 다 읽는 사람에게
# 같은 뜻이기 때문이다.
#
# ── 사유의 진위 (두 번째 검사) ────────────────────────────────────────────
# 위 검사는 **사유가 있는가**만 본다. 사유가 참인가는 통째로는 기계가 못 본다 —
# 그래서 예외 하나를 붙일 때 아무 말이나 적어도 통과한다.
#
# 통째로는 못 봐도 **사유가 좌표·형태를 들면 그 조각은 볼 수 있다.** 그래서 사유
# 형식에 검사 가능한 조각을 **일부러 넣게** 만든다. 자유 서술만 받으면 아무것도
# 못 본다. 지금 두 조각을 읽는다:
#
#   [결과사용]              그 자리가 큐를 우회하는 이유가 "응답이 필요해서" 라는
#                           주장. 호출 결과를 버리는 문장이면 그 주장은 거짓이다.
#   [부재 <파일> <정규식>]   "그 변형이 아직 없어서" 라는 주장. 정규식이 그 파일에
#                           나타나면 전제가 사라진 것이라 예외를 없애야 한다.
#
# 모르는 `[...]` 태그는 통과가 아니라 실패다 — 오타 하나로 검사가 조용히 꺼지면
# 검사가 있다는 사실 자체가 거짓이 된다.
#
# 태그가 없는 사유는 **검사되지 않는다.** 그건 한계지 통과가 아니다.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── 면제 경로 ────────────────────────────────────────────────────────────
# 전 패턴 면제: 도메인 핸들러 본문과 IPC handler(sync return contract).
EXEMPT_ALL=(
    "src/intent/popup.rs"
    "src/intent/preset.rs"
    "src/intent/surface.rs"
    "src/intent/tab.rs"
    "src/intent/pane.rs"
    "src/intent/workspace.rs"
    "src/state/preset_apply.rs"
    "src/state/pane.rs"
    "src/state/tab.rs"
    "src/state/tests.rs"
    "src/state/workspace.rs"
    "src/adapters/ipc/handler/surface.rs"
    "src/adapters/ipc/handler/image.rs"
    "src/adapters/ipc/handler/tab.rs"
    "src/adapters/ipc/handler/workspace.rs"
    "src/view/settings/ui.rs"
)
# pane 패턴만 면제: preset 미리보기 위젯이 **자기 트리**에 `split_pane` 을 갖고 있다.
# `AppState::split_pane` 과 이름만 같고 도메인이 아니다(`DemoLayout`).
EXEMPT_PANE=(
    "src/adapters/ui/preset/demo_layout.rs"
)

missing=()
for f in "${EXEMPT_ALL[@]}" "${EXEMPT_PANE[@]}"; do
    [ -f "$f" ] || missing+=("$f")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "면제 경로가 실재하지 않는다 — 트리가 옮겨졌는데 이 목록이 안 따라갔다."
    echo "없는 경로는 조용히 무시되고, 그 파일의 정당한 호출이 위반으로 쌓인다."
    printf '  %s\n' "${missing[@]}"
    exit 2
fi

exempt_all_re=$(printf '%s|' "${EXEMPT_ALL[@]}"); exempt_all_re=${exempt_all_re%|}
exempt_pane_re=$(printf '%s|' "${EXEMPT_PANE[@]}"); exempt_pane_re=${exempt_pane_re%|}

# ── 코드가 아닌 부분을 덮는 일은 **판정기가 한다** ────────────────────────────
# 이 스크립트는 한때 awk 로 자기 렉서를 갖고 있었다. 같은 물음("이 자리가 코드인가")에
# 답이 둘이면 둘이 따로 틀린다 — 실측: awk 판은 문자 리터럴과 raw string 을 몰라
# src 585 파일 중 166 에서 러스트 판정기와 다른 답을 냈다.
#
# 판정기가 없으면 **원문을 그대로 본다.** 그쪽은 문자열·주석 안의 언급까지 세는 방향,
# 즉 더 많이 잡는 쪽이라 조용한 통과를 안 만든다. 자동 채널은 판정기를 먼저 짓는다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
MASK_BIN="$(resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT")"
SCAN_ROOT="$ROOT"
if [ -n "$MASK_BIN" ]; then
    MASKED="$(mktemp -d)"
    trap 'rm -rf "$MASKED"' EXIT
    if "$MASK_BIN" "$MASKED" "$ROOT" src >/dev/null; then
        SCAN_ROOT="$MASKED"
    else
        echo "[intent-discipline] 마스킹 실패 — 원문에서 본다(문자열·주석 안의 호출까지 세어진다)." >&2
    fi
else
    echo "[intent-discipline] 원문에서 본다 — 문자열·주석 안의 호출까지 세어진다." >&2
fi

# ── 안 본 것과 없는 것을 가른다 ────────────────────────────────────────────
# 이 게이트는 잔여 0 을 요구하는 hard-fail 이다. 그래서 좌변이 비면 위반이 0 이 되고
# **조용히 통과한다** — 래칫과 달리 빨개질 하한이 없다. 0 이 "직접 mutation 호출이
# 없다" 인지 "아무것도 안 봤다" 인지 가르는 자리가 없었다. 종료 코드 2 의 근거는 머리말.
SRC_LIST=$(find "$SCAN_ROOT/src" -name '*.rs' -type f 2>/dev/null || true)
if [ -z "$SRC_LIST" ]; then
    echo "src/ 아래에서 .rs 를 하나도 못 찾았다 — 좌변이 깨졌다. 이 상태의 0 은 '직접"
    echo "mutation 호출이 없다' 가 아니라 '아무것도 안 봤다' 다. 통과로 읽지 마라."
    echo "  훑으려던 뿌리: $SCAN_ROOT/src"
    exit 2
fi
SRC_COUNT=$(printf '%s\n' "$SRC_LIST" | wc -l)

matches=$(find "$SCAN_ROOT/src" -name '*.rs' -type f -print0 \
  | xargs -0 awk -v exempt_all="$exempt_all_re" -v exempt_pane="$exempt_pane_re" \
        -v scan_root="$SCAN_ROOT/" -v root="$ROOT/" '
# 훑는 것은 **마스킹 사본**이다(M). 보고 좌표와 사유 주석은 원본에서 읽는다(L) —
# 사유는 주석에 적히므로 마스킹된 쪽에는 없다. 줄 번호는 사본이 보존한다.
FNR == 1 {
    if (NR > 1) flush()
    file = FILENAME
    sub("^" scan_root, "", file)      # 면제 경로 매칭과 보고 좌표는 레포 기준이다
    nl = 0
    is_test_file = (file ~ /_tests\.rs$/)
    # 원본을 같은 줄 번호로 들여온다. 못 읽으면 사본으로 대신한다 — 사유를 못 찾아
    # 더 많이 잡는 방향이라 조용한 통과가 안 된다.
    orig = root file
    no = 0
    while ((getline oline < orig) > 0) { no++; O[no] = oline }
    close(orig)
}
{ nl++; M[nl] = $0; L[nl] = (nl <= no ? O[nl] : $0) }
END { flush() }

function flush(   i, j, depth, k, hay, exA, exP) {
    if (nl == 0) return
    exA = (file ~ ("^(" exempt_all ")$"))
    exP = (file ~ ("^(" exempt_pane ")$"))
    # `#[cfg(test)]` 바로 뒤의 `mod ... {` 만 중괄호 깊이로 추적한다. 파일 앞머리의
    # `#[cfg(test)] mod x;` 선언(중괄호 없음)은 본문이 아니라 범위가 아니다.
    for (i = 1; i <= nl; i++) T[i] = is_test_file ? 1 : 0
    for (i = 1; i <= nl; i++) {
        if (M[i] !~ /#\[cfg\(test\)\]/) continue
        j = i + 1
        while (j <= nl && M[j] ~ /^[[:space:]]*$/) j++
        if (j > nl || M[j] !~ /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z0-9_]+[[:space:]]*\{/) continue
        depth = 0
        for (k = j; k <= nl; k++) {
            depth += gsub(/\{/, "{", M[k]) - gsub(/\}/, "}", M[k])
            T[k] = 1
            if (depth <= 0 && k > j) break
        }
        i = k
    }
    for (i = 1; i <= nl; i++) {
        if (T[i]) continue
        # 사유는 같은 줄 · 바로 위 · 바로 아래에서 인정한다.
        hay = L[i] (i > 1 ? L[i-1] : "") (i < nl ? L[i+1] : "")
        if (hay ~ /intent-exempt/) continue
        if (!exA && M[i] ~ /\.popups\.(open|open_centered|open_centered_focused|open_with_scope|open_at_top_of_scope|open_at_focused|close|toggle|toggle_focused)\(/) hit(i, "popup")
        else if (!exA && M[i] ~ /\.(save_workspace|save_workspace_overwrite|save_tab|save_tab_overwrite|save_pane|save_pane_overwrite|apply_workspace_preset|apply_tab_preset|apply_pane_preset)\(/) hit(i, "preset")
        else if (!exA && M[i] ~ /\.(delete|rename)\([[:space:]]*PresetKind/) hit(i, "preset")
        else if (!exA && M[i] ~ /\.(split_surface|close_surface_by_id|close_surface_by_id_no_snapshot|convert_surface_to_terminal|convert_surface_to_markdown|convert_surface_to_image|convert_surface_to_html|convert_surface_to_kind)\(/) hit(i, "surface")
        else if (!exA && M[i] ~ /\.(add_kind_tab|add_kind_tab_to_pane|add_markdown_tab|add_html_tab|add_image_tab|add_empty_tab|add_tab_to_pane|close_tab_by_tab_id)\(/) hit(i, "tab")
        else if (!exA && !exP && M[i] ~ /\.split_pane\(/) hit(i, "pane")
        else if (!exA && M[i] ~ /\.add_workspace\(/) hit(i, "workspace")
    }
    nl = 0
}
function hit(i, dom) { printf "%s:%d: [%s] %s\n", file, i, dom, L[i] }
' || true)

if [ -n "$matches" ]; then
    echo "Intent discipline 위반: 도메인 직접 mutation 호출이 발견되었습니다."
    echo "Intent 큐로 발화하거나, 정당한 사유면 같은 줄 / 바로 위 / 바로 아래에"
    echo "'// intent-exempt: <사유>' 주석을 추가하세요."
    echo
    echo "$matches"
    exit 1
fi

# ── 사유의 검사 가능한 조각을 검증한다 ──────────────────────────────────
claim_fail=""
while IFS= read -r line; do
    file=${line%%:*}; rest=${line#*:}
    lno=${rest%%:*}
    text=$(sed -n "${lno}p" "$file")

    # 주장이 걸리는 줄: 주석 줄에 호출이 함께 있으면 그 줄, 아니면 다음 줄.
    target_no=$lno
    case "$text" in
        *"//"*) before=${text%%//*}
                case "$before" in
                    *"("*) ;;
                    *) target_no=$((lno + 1)) ;;
                esac ;;
    esac
    target=$(sed -n "${target_no}p" "$file")

    # 모르는 태그는 실패. 알려진 둘만 통과한다.
    # 공백이 든 태그가 있으므로 단어 분리가 아니라 줄 단위로 읽는다.
    while IFS= read -r tag; do
        [ -z "$tag" ] && continue
        case "$tag" in
            "[결과사용]"|"[부재 "*) ;;
            *) claim_fail+="$file:$lno: 모르는 사유 태그 $tag — 오타면 검사가 조용히 꺼진다"$'\n' ;;
        esac
    done < <(printf '%s\n' "$text" | grep -oE '\[[^]]+\]' || true)

    case "$text" in
        *"[결과사용]"*)
            trimmed=$(printf '%s' "$target" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
            discard=""
            case "$trimmed" in
                # `let _ =` 는 문법적으로는 소비지만 의미로는 버리는 것이다. 대입이
                # 있다는 것만 보면 이 모양이 그대로 통과한다(실측으로 뚫렸다).
                *"let _ ="*|"_ = "*) discard=1 ;;
                # 그 밖에 결과를 버리는 모양: `... );` 로 끝나면서 대입도 분기도 아니다.
                *");")
                    case "$trimmed" in
                        *"="*|return*|"if "*|"match "*|"let "*) ;;
                        *) discard=1 ;;
                    esac ;;
            esac
            [ -n "$discard" ] && claim_fail+="$file:$target_no: [결과사용] 이 거짓이다 — 결과를 버리는 문장이다: $trimmed"$'\n' ;;
    esac

    case "$text" in
        *"[부재 "*)
            claim=${text#*"[부재 "}; claim=${claim%%"]"*}
            cf=${claim%% *}; cre=${claim#* }
            if [ ! -f "$cf" ]; then
                claim_fail+="$file:$lno: [부재] 가 가리키는 파일이 없다: $cf"$'\n'
            elif grep -Eq "$cre" "$cf"; then
                claim_fail+="$file:$lno: [부재] 의 전제가 사라졌다 — $cf 에 /$cre/ 가 생겼다. 이 예외를 없애고 큐로 옮겨라"$'\n'
            fi ;;
    esac
done < <(grep -rn "intent-exempt" src --include='*.rs' || true)

if [ -n "$claim_fail" ]; then
    echo "intent-exempt 사유가 든 주장이 지금 거짓이다."
    echo
    printf '%s' "$claim_fail"
    exit 1
fi

# 초록일 때도 **무엇을 몇 개 봤는지** 찍는다. 수를 안 찍으면 이 게이트의 초록은
# "위반이 없다" 와 "아무것도 안 봤다" 가 화면에서 같은 모양이 된다 — 위 판정 불가가
# 막는 것은 좌변이 **완전히** 빈 경우뿐이고, 반쯤 줄어든 좌변은 이 수를 봐야 보인다.
echo "Intent discipline check passed — src/ 의 .rs ${SRC_COUNT}개를 훑어 위반 0."
