#!/usr/bin/env bash
# `#[allow(...)]` / `#![allow(...)]` 억제에 근거 주석(`reason:` / `complexity-exempt:` /
# `SAFETY`)이 같은 줄이나 직전 줄에 있는지 감사한다.
#
# ── 왜 hard-fail 이 아니라 래칫인가 ──────────────────────────────────────
# 잔여가 0 이 아니라서 hard-fail 로 걸면 main 이 그 자리에서 빨개진다. 그렇다고
# 리포트로 두면 **건수와 무관하게 항상 통과**하는 칸이 하나 생긴다 — 채널은 도는데
# 술어가 아무것도 안 보는 상태이고, 초록이 뜨니 더 위험하다.
#
# 그래서 상한을 박은 **래칫**이다. 세 방향을 다 본다:
#   늘면        실패한다 — 오늘부터 새로 들어오는 것은 전부 빨갛다.
#   줄면        실패한다 — 상한을 같이 내리라는 뜻이다. 상한이 실제보다 크면 그
#               차이만큼 조용히 받아주므로, **여유는 곧 안 보는 구간**이다.
#   스캐너가 깨지면 실패한다 — `set -euo pipefail` 로 그 자리에서 죽는다.
#
# 상한은 줄어들 수만 있다. 늘리려면 이 수를 고쳐야 하고, 그 한 줄이 리뷰에 보인다.
#
# 채널: .github/workflows/script-gates.yml (main push · PR)
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

# 래칫 상한. **실제 건수와 같아야 한다** — 크면 그 차이만큼 조용히 받아준다.
# 줄였으면 이 수도 같이 내려라(스크립트가 그 자리에서 시킨다).
CAP=234

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

echo "근거 없는 #[allow(...)] : ${count}건 (상한 ${CAP})"
echo

if [ "$count" -gt "$CAP" ]; then
    printf '%s' "$report"
    echo
    echo "근거 없는 #[allow(...)] 가 늘었다: ${count} > 상한 ${CAP}."
    echo "새로 붙인 억제에 근거 주석(reason: / complexity-exempt: / SAFETY)을 같은 줄이나"
    echo "직전 줄에 달아라. 상한을 올려서 통과시키지 마라 — 래칫은 한 방향으로만 돈다."
    exit 1
fi

if [ "$count" -lt "$CAP" ]; then
    echo "근거 없는 #[allow(...)] 가 줄었다: ${count} < 상한 ${CAP}."
    echo "이 스크립트의 CAP 을 ${count} 로 내려라. 상한이 실제보다 크면 그 차이만큼"
    echo "새 위반을 조용히 받아준다 — 남는 여유가 곧 안 보는 구간이다."
    exit 1
fi
