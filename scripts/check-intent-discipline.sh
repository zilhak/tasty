#!/usr/bin/env bash
# Intent 도메인 직접 호출 패턴 금지 체크 — popup / preset / surface / tab / pane /
# workspace 도메인의 **변이** API.
#
# 정책 근거: docs/design/flows/action-dispatch.md
# 채널: .github/workflows/script-gates.yml (main push · PR)
#
# ── 술어의 성질 (되돌리지 마라) ──────────────────────────────────────────
# 이 검사는 **텍스트 스캔**이라 타입을 모른다. 그래서 오탐이 나는 자리를 세 가지로
# 나눠 각각 다르게 막는다. 셋을 하나로 합치면(예: 파일 통째 제외) 그 파일의 다른
# 위반까지 같이 사라진다.
#
#   ① 코드가 아닌 것        → 줄 주석·블록 주석·문자열 리터럴을 **지우고** 찾는다.
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

matches=$(find src -name '*.rs' -type f -print0 \
  | xargs -0 awk -v exempt_all="$exempt_all_re" -v exempt_pane="$exempt_pane_re" '
# ── 코드가 아닌 부분을 지운다. 블록 주석 상태는 파일을 가로질러 들고 간다. ──
function mask(s,   out, i, c, n, instr) {
    out = ""; n = length(s); i = 1; instr = 0
    while (i <= n) {
        c = substr(s, i, 1)
        if (inblock) {
            if (c == "*" && substr(s, i+1, 1) == "/") { inblock = 0; i += 2; continue }
            i++; continue
        }
        if (!instr && c == "/" && substr(s, i+1, 1) == "/") break
        if (!instr && c == "/" && substr(s, i+1, 1) == "*") { inblock = 1; i += 2; continue }
        if (c == "\"") { instr = !instr; i++; continue }
        if (instr && c == "\\") { i += 2; continue }
        if (!instr) out = out c
        i++
    }
    return out
}
FNR == 1 {
    if (NR > 1) flush()
    file = FILENAME; nl = 0; inblock = 0
    is_test_file = (file ~ /_tests\.rs$/)
}
{ nl++; L[nl] = $0; M[nl] = mask($0) }
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

echo "Intent discipline check passed."
