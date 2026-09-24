#!/usr/bin/env bash
# 등록된 도메인 변경 메서드의 직접 호출을 텍스트로 찾는다. 타입을 해석하지 않는다.
# 코드 탐지는 마스킹 사본, 사유는 원문을 읽는다. test 모듈·파일과 명시된 경로 예외를 적용한다.
# intent-exempt가 인접 줄에 있으면 제외한다. [결과사용]·[부재 파일 정규식] 태그만 추가로 확인하며 사유의 의미 전체는 검증하지 않는다.
# 종료 코드: 통과 0, 위반 1, 도구·수집 문제 2. 정책은 docs/design/flows/action-dispatch.md.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# 수집·마스킹·호출 탐지·사유 조회가 같은 디렉터리 목록을 사용한다.
SCAN_DIRS=(src)

# 도메인 구현과 동기 응답이 필요한 IPC 경로는 명시된 파일에서 제외한다.
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
# DemoLayout의 동명 메서드는 앱 도메인 호출이 아니므로 pane 패턴만 제외한다.
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

# 공용 mask-source로 코드가 아닌 부분을 지운다. 도구가 없거나 실패하면 아래에서 판정을 거부한다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
MASK_BIN="$JUDGE_BIN"
SCAN_ROOT="$ROOT"
if [ -n "$MASK_BIN" ]; then
    MASKED="$(mktemp -d)"
    trap 'rm -rf "$MASKED"' EXIT
    if "$MASK_BIN" "$MASKED" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null; then
        SCAN_ROOT="$MASKED"
    else
        echo "[intent-discipline] 마스킹 실패 — 원문에서 본다(문자열·주석 안의 호출까지 세어진다)." >&2
    fi
else
    echo "[intent-discipline] 판정기가 없다 — 원문은 문자열·주석 안의 호출까지 센다." >&2
fi

if [ "$SCAN_ROOT" = "$ROOT" ]; then
    echo "[intent-discipline] 판정 불가 — 마스킹 사본 없이 원문을 훑게 된다." >&2
    echo "  원문에는 주석·문자열 안의 호출까지 들어 있어, 여기서 나오는 위반은 실재하지" >&2
    echo "  않을 수 있다. 그 값으로 규율을 판정하지 않는다." >&2
    echo >&2
    echo "  ★ 면제 주석을 달지 마라. 판정기를 지어라:" >&2
    echo "      cargo build -p tasty-doc-guards --bin mask-source" >&2
    echo "      target/debug/mask-source --check-fresh ." >&2
    echo "  --check-fresh 가 rc=0 이어야 이 게이트의 값이 값이다(낡은 판정기도 없는 것으로 다룬다)." >&2
    echo "  rebase 직후라면 이것이 첫 번째로 할 일이다." >&2
    exit 2
fi

# 빈 수집을 위반 0건으로 처리하지 않는다. 일부 파일 누락까지 검출하는 것은 아니다.
SCAN_LABEL=$(printf '%s/ ' "${SCAN_DIRS[@]}"); SCAN_LABEL=${SCAN_LABEL% }
# 등록된 검사 디렉터리가 없으면 누락된 범위로 판정을 진행하지 않는다.
absent=()
for d in "${SCAN_DIRS[@]}"; do [ -d "$ROOT/$d" ] || absent+=("$d"); done
if [ ${#absent[@]} -gt 0 ]; then
    echo "좌변에 적힌 디렉토리가 실재하지 않는다 — 그만큼이 조용히 안 보인다."
    printf '  %s\n' "${absent[@]}"
    exit 2
fi
SCANNED_LIST=$(find "${SCAN_DIRS[@]/#/$SCAN_ROOT/}" -name '*.rs' -type f 2>/dev/null || true)
if [ -z "$SCANNED_LIST" ]; then
    echo "좌변($SCAN_LABEL) 아래에서 .rs 를 하나도 못 찾았다 — 좌변이 깨졌다. 이 상태의"
    echo "0 은 '직접 mutation 호출이 없다' 가 아니라 '아무것도 안 봤다' 다. 통과로 읽지 마라."
    printf '  훑으려던 뿌리: %s\n' "${SCAN_DIRS[@]/#/$SCAN_ROOT/}"
    exit 2
fi
SCANNED_COUNT=$(printf '%s\n' "$SCANNED_LIST" | wc -l)

matches=$(find "${SCAN_DIRS[@]/#/$SCAN_ROOT/}" -name '*.rs' -type f -print0 \
  | xargs -0 awk -v exempt_all="$exempt_all_re" -v exempt_pane="$exempt_pane_re" \
        -v scan_root="$SCAN_ROOT/" -v root="$ROOT/" '
# 마스킹 사본으로 호출을 찾고 같은 줄 번호의 원문에서 사유를 읽는다.
FNR == 1 {
    if (NR > 1) flush()
    file = FILENAME
    sub("^" scan_root, "", file)      # 면제 경로 매칭과 보고 좌표는 레포 기준이다
    nl = 0
    is_test_file = (file ~ /_tests\.rs$/)
    # 원문을 읽지 못한 줄은 사본을 사용해 사유를 인정하지 않을 수 있다.
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
    # cfg(test) 다음의 별도 mod 본문만 추적한다. 모든 cfg 형태나 외부 test 파일 선언을 해석하지는 않는다.
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
    echo "Intent discipline 위반: 도메인을 직접 변경하는 호출을 찾았습니다."
    echo "Intent 큐에 요청을 넣거나, 정당한 사유면 같은 줄 / 바로 위 / 바로 아래에"
    echo "'// intent-exempt: <사유>' 주석을 추가하세요."
    echo
    echo "$matches"
    exit 1
fi

claim_fail=""
while IFS= read -r line; do
    file=${line%%:*}; rest=${line#*:}
    lno=${rest%%:*}
    text=$(sed -n "${lno}p" "$file")

    target_no=$lno
    case "$text" in
        *"//"*) before=${text%%//*}
                case "$before" in
                    *"("*) ;;
                    *) target_no=$((lno + 1)) ;;
                esac ;;
    esac
    target=$(sed -n "${target_no}p" "$file")

    # 태그 안에 공백이 있을 수 있어 줄 단위로 처리한다.
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
                # let _ =처럼 결과를 버리는 형태는 대입이 있어도 인정하지 않는다.
                *"let _ ="*|"_ = "*) discard=1 ;;
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
done < <(grep -rn "intent-exempt" "${SCAN_DIRS[@]}" --include='*.rs' || true)

if [ -n "$claim_fail" ]; then
    echo "intent-exempt 사유가 실제 코드와 맞지 않는다."
    echo
    printf '%s' "$claim_fail"
    exit 1
fi

echo "Intent discipline check passed — 좌변($SCAN_LABEL) 의 .rs ${SCANNED_COUNT}개를 훑어 위반 0."
