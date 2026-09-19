#!/usr/bin/env bash
#
# 셸 자산을 정적 검사기(shellcheck)에 태운다.
#
# 이 레포의 셸 자산은 전부 게이트·빌드 보조·git 훅이다. 사용자 기계에서 직접 도는 것은
# 없지만, **판정하는 쪽**이라 조용한 결함의 값이 비싸다. 실제로 그 형태가 있었다 —
# 한 게이트의 실패 메시지가 예시를 역따옴표로 감싸 **명령 치환**이 되어 있었고, 그
# 메시지는 실패할 때만 나오므로 아무도 그것이 깨진 것을 못 봤다. 같은 자리에서 파서가
# 멈춰 그 아래 200 줄이 어떤 검사도 안 받고 있었다.
#
# ## 왜 warning 위인가
#
# 잔여 0 hard-fail 이다. 문턱을 `style`/`info` 까지 내리면 잔여가 43 이고 그중 31 이
# 이 레포에서 오탐이다(배열로 호출하는 함수를 "안 불린다" 로, source 하는 파일을
# "못 따라간다" 로 센다). 그 수를 래칫으로 얼리면 상한 밑의 여유가 곧 안 보는 구간이
# 되고, 일괄 억제하면 같은 코드의 진짜 위반까지 같이 덮인다. warning 위는 도입 시점에
# 12 였고 전부 고쳤다 — 그 12 가 실물 결함이었다는 것이 이 문턱의 근거다.
#
# 근거·대안·재검토 조건: docs/adr/0295-shell-assets-are-judged-at-warning-and-above.md
#
# 사용법:
#   bash scripts/check-shell-assets.sh              # 레포의 셸 자산 전부
#   bash scripts/check-shell-assets.sh <파일>...     # 그중 지정한 것만 (훅이 이렇게 쓴다)
#
# 종료코드: 0 통과 · 1 위반 · 2 판정 불가(검사기 없음 · 좌변 0 · 비-git)

set -e -o pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
[ -n "$REPO_ROOT" ] || { echo "[shell-assets] 판정 불가: git 저장소가 아니다." >&2; exit 2; }
cd "$REPO_ROOT"

# 검사기가 없으면 **통과가 아니라 판정 불가**다. 이 레포의 다른 게이트가 판정기 부재를
# 같은 등급으로 다룬다 — 없는 판정을 초록으로 세면 그 칸은 영영 안 보이는 칸이 된다.
if ! command -v shellcheck >/dev/null 2>&1; then
    echo "[shell-assets] 판정 불가: shellcheck 이 PATH 에 없다." >&2
    echo "  받는 법: bash scripts/install-shellcheck.sh   (배포판 패키지도 된다)" >&2
    exit 2
fi

# 좌변. **확장자로만 고르면 훅 셋이 통째로 빠진다** — 그 셋은 확장자가 없고, 마침
# 로컬에서 실제로 돌아가는 유일한 셸 자산이다. 그래서 확장자와 shebang 둘 다로 고른다.
# (`scripts/lib/judge-bin.sh` 는 반대 방향의 예다 — source 되는 라이브러리라 shebang 이
# 없고 확장자로만 걸린다. 한쪽만 쓰면 둘 중 하나를 놓친다.)
is_shell_asset() {
    case "$1" in *.sh) return 0 ;; esac
    [ -f "$1" ] || return 1
    # 파이프를 안 만든다 — 오른쪽이 먼저 닫으면 왼쪽이 SIGPIPE 로 죽고 `pipefail`
    # 아래서는 그 실패가 조건을 뒤집는다(그 가드가 이 줄을 실제로 잡았다).
    local first=""
    IFS= read -r first < "$1" || true
    grep -qE '^#!.*[ /](ba|z)?sh( |$)' <<<"$first"
}

# 좌변을 고르는 규칙은 **한 곳뿐**이다(`is_shell_asset`). 훅이 넘기는 staged 목록도 같은
# 규칙으로 거른다 — 규칙을 호출자 쪽에 복사하면 두 모수가 같은 물음에 다른 답을 갖는다.
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
        # `|| true` — 후보가 0 건이면 `git grep` 이 1 을 낸다. `set -e` 아래서는 그 1 이
        # 좌변 수집을 **조용히 죽인다**(치환 안에서 죽으므로 출력도 안 남는다).
        git grep -lI -E '^#!' -- . 2>/dev/null || true
    } | select_shell_assets
    return 0
}

if [ "$#" -gt 0 ]; then
    TARGETS=$(printf '%s\n' "$@" | select_shell_assets | sort -u)
else
    TARGETS=$(collect_corpus | sort -u)
fi

# 빈 문자열을 `printf '%s\n'` 로 내면 **줄 하나**가 된다 — 그 값을 그대로 세면 좌변 0 이
# 1 로 세어지고, 아래 '좌변 0 은 판정 불가' 갈래가 영영 안 열린다. 실측으로 밟았다:
# 셸 자산이 없는 트리에서 이 게이트가 "자산 1 개" 라고 찍으며 검사기의 사용법을 뱉었다.
COUNT=$(printf '%s\n' "$TARGETS" | grep -c . || true)

# 좌변이 0 이면 초록이 아니라 판정 불가다. 인자를 받은 호출은 예외다 — 훅이 셸 자산이
# 안 담긴 커밋에서 이 스크립트를 부를 때가 정상이고, 그때는 볼 것이 없는 것이 맞다.
if [ "$COUNT" -eq 0 ]; then
    if [ "$#" -gt 0 ]; then
        echo "[shell-assets] 대상 0 개 — 넘어온 $# 개 중 셸 자산이 없다."
        exit 0
    fi
    echo "[shell-assets] 판정 불가: 좌변이 0 이다. 순회가 죽었는지 봐라 — 이 레포에 셸 자산이 없을 수는 없다(훅 셋)." >&2
    exit 2
fi

VERSION=$(shellcheck --version | sed -n 's/^version: //p')

set +e
# 단어 분리가 목적이다 — TARGETS 는 줄바꿈으로 이은 경로 목록이고 shellcheck 은 그것을
# 가변 인자로 받는다. 경로에 공백이 있으면 깨지지만, 이 레포의 추적 경로에는 없다.
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
