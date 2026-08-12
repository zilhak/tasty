#!/usr/bin/env bash
# `#[allow(...)]` / `#![allow(...)]` 억제에 근거 주석(`reason:` / `complexity-exempt:` /
# `SAFETY`)이 같은 줄이나 직전 줄에 있는지 감사한다. 없으면 리포트에 출력.
#
# 현재는 리포트 전용이다 — CI hard-fail 로 거는 건 스코프 밖 (docs/adr 필요시 별도 결정).
#
# 사용: scripts/check-allow-reason.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if command -v rg >/dev/null 2>&1; then
    FILES=$(rg --files -g '*.rs' crates src)
else
    FILES=$(git ls-files -- 'crates/*.rs' 'src/*.rs')
fi

REASON_PATTERN='reason:|complexity-exempt:|SAFETY'

report=""
count=0

while IFS= read -r file; do
    [ -z "$file" ] && continue
    hits=$(awk -v reason_pat="$REASON_PATTERN" '
        {
            lines[NR] = $0
        }
        END {
            for (i = 1; i <= NR; i++) {
                line = lines[i]
                if (line ~ /#!?\[allow\(/) {
                    prev = (i > 1) ? lines[i - 1] : ""
                    if (line !~ reason_pat && prev !~ reason_pat) {
                        printf "%d: %s\n", i, line
                    }
                }
            }
        }
    ' "$file")
    [ -z "$hits" ] && continue
    while IFS= read -r hit; do
        report+="${file}:${hit}"$'\n'
        count=$((count + 1))
    done <<<"$hits"
done <<<"$FILES"

echo "근거 없는 #[allow(...)] : ${count}건"
echo
if [ "$count" -gt 0 ]; then
    printf '%s' "$report"
fi
