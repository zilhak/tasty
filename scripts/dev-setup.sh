#!/usr/bin/env bash
# Tasty 개발 환경 초기 셋업 스크립트.
#
# clone 직후 1회 실행한다. 멱등 (여러 번 실행해도 안전).
#
# 현재 수행 작업:
#   1. git hooks 디렉토리를 `.githooks/` 로 변경 (core.hooksPath)
#
# 향후 추가 예정 (필요 시):
#   - 추가 lint 도구 설치 검증
#   - 빌드 의존성 확인

set -e

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "✘ git 저장소 안에서 실행해주세요." >&2
    exit 1
}

cd "$REPO_ROOT"

red()    { printf '\033[31m%s\033[0m\n' "$*" >&2; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

yellow "▸ Tasty 개발 환경 셋업"
echo

# ────────────────────────────────────────────────────────────
# 1. git hooks 디렉토리 설정
# ────────────────────────────────────────────────────────────
yellow "[1/2] git hooks 설정 (core.hooksPath = .githooks)"

CURRENT="$(git config --local --get core.hooksPath || echo '')"
if [ "$CURRENT" = ".githooks" ]; then
    green "  이미 설정됨 — skip"
else
    git config --local core.hooksPath .githooks
    green "  설정 완료"
fi

if [ ! -d .githooks ]; then
    red "  ✘ .githooks 디렉토리가 없습니다. repo 가 손상되었는지 확인하세요."
    exit 1
fi

# hook 실행 권한 보장 (clone 시 일부 환경에서 실행 권한 누락).
# 이름을 하나하나 적지 않는다 — 그러면 훅 명부의 사본이 여기 하나 더 생기고, 훅을
# 더한 사람이 이 줄을 같이 안 고치면 그 훅만 조용히 실행 권한 없이 남는다.
# 명부 밖의 파일이 이 디렉토리에 있을 수 없다는 것은 가드가 따로 본다
# (crates/tasty-doc-guards/tests/githooks_are_pinned.rs).
chmod +x .githooks/* 2>/dev/null || true

# ────────────────────────────────────────────────────────────
# 2. 셸 정적 검사기
#
# pre-commit 의 A.3 과 CI 의 script-gates 가 이것을 부른다. **없으면 통과가 아니라
# 판정 불가로 실패한다** — 그래서 셋업이 받아 둔다. 이미 있으면 아무것도 안 한다.
# ────────────────────────────────────────────────────────────
echo
yellow "[2/2] 셸 정적 검사기 (shellcheck)"
if bash "$(dirname "$0")/install-shellcheck.sh"; then
    green "  준비됨"
else
    yellow "  ▸ 못 받았다. 배포판 패키지로 받아도 된다 (apt/brew/pacman 의 shellcheck)."
    yellow "    그때까지 pre-commit 의 A.3 이 판정 불가로 멈춘다 — 통과가 아니라 멈춤이다."
fi

echo
green "✓ 셋업 완료"
echo
echo "이제 git commit / git push 시 자동으로 검사가 실행됩니다."
echo "검사 규칙: docs/dev-guide/git-hooks.md"
echo "긴급 우회: git commit --no-verify / git push --no-verify"
