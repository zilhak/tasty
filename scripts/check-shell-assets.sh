#!/usr/bin/env bash
# 추적된 .sh 파일과 shell shebang 파일을 ShellCheck warning 수준으로 검사한다.
# 인자를 주면 그 목록에 같은 선택 규칙을 적용한다. 통과 0, 검사 실패 1, 도구·대상 부재 2.

set -e -o pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
[ -n "$REPO_ROOT" ] || { echo "[shell-assets] 판정 불가: git 저장소가 아니다." >&2; exit 2; }
cd "$REPO_ROOT"

if ! command -v shellcheck >/dev/null 2>&1; then
    echo "[shell-assets] 판정 불가: shellcheck 이 PATH 에 없다." >&2
    echo "  받는 법: bash scripts/install-shellcheck.sh   (배포판 패키지도 된다)" >&2
    exit 2
fi

is_shell_asset() {
    case "$1" in *.sh) return 0 ;; esac
    [ -f "$1" ] || return 1
    # grep -q가 producer를 조기 종료해 SIGPIPE를 일으키지 않도록 먼저 내용을 받는다.
    local first=""
    IFS= read -r first < "$1" || true
    grep -qE '^#!.*[ /](ba|z)?sh( |$)' <<<"$first"
}

select_shell_assets() {
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        if is_shell_asset "$f"; then printf '%s\n' "$f"; fi
    done
    return 0
}

collect_corpus() {
    {
        git ls-files -- '*.sh'
        # 후보가 없을 때 git grep의 rc 1을 허용한다.
        git grep -lI -E '^#!' -- . 2>/dev/null || true
    } | select_shell_assets
    return 0
}

if [ "$#" -gt 0 ]; then
    TARGETS=$(printf '%s\n' "$@" | select_shell_assets | sort -u)
else
    TARGETS=$(collect_corpus | sort -u)
fi

COUNT=$(printf '%s\n' "$TARGETS" | grep -c . || true)

# 전체 검사 대상이 비면 거부한다. 인자로 받은 staged 목록에 셸 파일이 없는 경우는 통과다.
if [ "$COUNT" -eq 0 ]; then
    if [ "$#" -gt 0 ]; then
        echo "[shell-assets] 대상 0 개 — 넘어온 $# 개 중 셸 자산이 없다."
        exit 0
    fi
    echo "[shell-assets] 판정 불가: 검사할 셸 파일을 찾지 못했다. Git 파일 수집과 선택 조건을 확인해라." >&2
    exit 2
fi

VERSION=$(shellcheck --version | sed -n 's/^version: //p')

set +e
# 이유: TARGETS의 경로 목록을 개별 인자로 넘기기 위해 단어 분리를 사용한다.
# shellcheck disable=SC2086
OUT=$(shellcheck --severity=warning --format=tty $TARGETS 2>&1)
RC=$?
set -e

if [ "$RC" -ne 0 ]; then
    printf '%s\n' "$OUT" >&2
    echo >&2
    echo "[shell-assets] 위반 — 셸 자산 $COUNT 개 · shellcheck $VERSION · 문턱 warning 이상 · 잔여 0 이 기준이다." >&2
    echo "  의도한 코드면 그 줄 바로 위에 \`# shellcheck disable=<코드>\` 와 **왜인지**를 같이 적어라." >&2
    echo "  근거 없는 억제는 다음 사람에게 '누군가 판단했다' 로 읽히고, 실제로 그렇지 않았던 자리가 있었다." >&2
    exit 1
fi

echo "[shell-assets] 통과 — 셸 자산 $COUNT 개 · shellcheck $VERSION · 문턱 warning 이상 · 위반 0"
