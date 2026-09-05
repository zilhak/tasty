#!/usr/bin/env bash
# `#[allow(...)]` 계열 억제에 근거 주석(`reason:` / `이유:` / `complexity-exempt:` /
# `SAFETY`)이 같은 줄이나 **바로 위에 붙은 주석 블록**에 있는지 감사한다.
#
# ── 술어가 무엇을 세는가 (세 번 넓혔다) ──────────────────────────────────
# 1. **형태**: `#[allow(` 만 보면 `#[cfg_attr(<조건>, allow(...))]` 을 한 건도 못 본다.
#    조건부 억제는 **어떤 조합에서만** 린트를 끄는 형태라 오히려 사유가 더 필요하다 —
#    다른 조합에서 살아 있으니 안전해 보이는데, 그 조합에서 무엇이 꺼졌는지는 아무도
#    안 본다. 실측(2026-09-05): 그 형태가 60 자리였고 감사 밖이었다.
# 2. **마커**: 이 레포는 근거를 한글 `이유:` 로 적는다(실측 41 자리). 영문 마커만
#    보면 그 41 이 전부 "근거 없음" 으로 세어진다 — 세는 대상이 아니라 표기를 센다.
# 3. **창**: 근거는 보통 여러 줄짜리 주석 블록이라 직전 한 줄만 보면 블록의 마지막
#    줄만 보게 된다. 그래서 **붙어 있는 주석 블록 전체**를 본다. 창을 1→2→3→4→6→무한
#    으로 넓히며 잰 잔여는 279 → 244 → 236 → 234 → 232 → 231 로 6 부터 포화한다.
#    임의의 상수를 박는 대신 블록 경계(빈 줄·코드 줄)를 쓴다.
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

REASON_PATTERN='reason:|이유:|complexity-exempt:|SAFETY'

# 래칫 상한. **실제 건수와 같아야 한다** — 크면 그 차이만큼 조용히 받아준다.
# 줄였으면 이 수도 같이 내려라(스크립트가 그 자리에서 시킨다).
#
# 이 수는 술어를 넓힌 회차에 다시 잰 값이다(234 → 231). **위반이 줄어서가 아니다** —
# 세는 형태가 60 자리 늘고(조건부 억제) 인정하는 표기가 41 자리 늘어(한글 마커·블록 창)
# 두 변화가 상쇄된 값이다. 그래서 이 231 은 이전 234 와 **같은 것을 센 수가 아니다.**
CAP=184

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
                # 무조건 억제와 조건부 억제(`cfg_attr(..., allow(...))`)를 함께 센다.
                if (line !~ /#!?\[allow\(/ && !(line ~ /#!?\[cfg_attr\(/ && line ~ /allow\(/)) {
                    continue
                }
                found = (line ~ reason_pat)
                # 바로 위에 붙은 주석 블록 전체를 본다 — 빈 줄이나 코드 줄에서 끊긴다.
                for (k = i - 1; k >= 1 && !found; k--) {
                    if (lines[k] !~ /^[[:space:]]*\/\//) break
                    if (lines[k] ~ reason_pat) found = 1
                }
                if (!found) printf "%d: %s\n", i, line
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
