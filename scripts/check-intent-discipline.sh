#!/usr/bin/env bash
# popup 도메인 직접 호출 패턴 (`state.popups.open*` / `.close` / `.toggle*`) 을
# 금지하는 grep 기반 CI 체크. 예외는 `// intent-exempt: <사유>` 주석으로 suppress.
#
# 정책 근거: docs/design/action-dispatch.md
# 후속 도메인 (preset, surface, tab, ...) 추가 시 본 스크립트의 패턴/예외 경로 확장.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# 매칭 대상 패턴 — popup 도메인 mutation API.
# (open / open_centered / open_centered_focused / open_with_scope /
#  open_at_top_of_scope / open_at_focused / close / toggle)
PATTERN='\.popups\.(open([_a-z]+)?|close|toggle([_a-z]+)?)\b'

# 핸들러 본문 + UI/Settings 내부 예외 경로.
EXEMPT_FILES=(
    "src/intent/popup.rs"
    "src/settings_ui/mod.rs"
)

# rg 가 있으면 사용, 없으면 grep -E.
if command -v rg >/dev/null 2>&1; then
    SEARCH="rg --no-heading -n --color never -e"
else
    SEARCH="grep -rEn"
fi

# 검색 후 예외 파일 / `is_open` query / `get_mut` query / intent-exempt 주석 라인 제거.
matches=$($SEARCH "$PATTERN" src/ 2>/dev/null \
    | grep -v 'is_open\|get_mut\|register\b' \
    | grep -v 'intent-exempt' \
    || true)

# 예외 파일 라인 제외.
for f in "${EXEMPT_FILES[@]}"; do
    matches=$(echo "$matches" | grep -v "^$f:" || true)
done

# `// intent-exempt:` 주석이 동일/직전 라인에 있으면 false positive — 정확한 라인 추적은
# clippy lint 로 미루고, 여기서는 같은 라인의 `intent-exempt` 텍스트만 suppress 한다.
# 동일 라인에 `intent-exempt` 키워드가 있어야 인정 (개발자가 의도적으로 표시한 경우).

if [ -n "$matches" ]; then
    echo "Intent discipline 위반: popup 도메인 직접 mutation 호출이 발견되었습니다."
    echo "Intent 큐로 발화하거나, 정당한 사유면 동일 라인에 '// intent-exempt: <사유>' 주석을 추가하세요."
    echo
    echo "$matches"
    exit 1
fi

echo "Intent discipline check passed."
