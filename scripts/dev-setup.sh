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
yellow "[1/1] git hooks 설정 (core.hooksPath = .githooks)"

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

# hook 실행 권한 보장 (clone 시 일부 환경에서 실행 권한 누락)
chmod +x .githooks/pre-commit .githooks/pre-push .githooks/pre-merge-commit 2>/dev/null || true

echo
green "✓ 셋업 완료"
echo
echo "이제 git commit / git push 시 자동으로 검사가 실행됩니다."
echo "검사 규칙: docs/dev-guide/git-hooks.md"
echo "긴급 우회: git commit --no-verify / git push --no-verify"
