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

# ── 문자열 리터럴을 덮은 사본에서 센다 ─────────────────────────────────────
#
# 이 검사는 텍스트 스캔이라 타입을 모르고, **문자열 안의 억제 형태를 실물로 센다.**
# 그 형태를 소스에 쓰는 것은 가드의 회귀 픽스처다 — 판정 대상 형태를 회귀로 박으려면
# 그 형태를 써야 하고, 쓴 순간 이 게이트의 모수가 된다(실측으로 여러 번 밟았다).
#
# 판정을 여기서 다시 구현하지 않는다. 러스트 층이 이미 그 판정을 갖고 있고
# (`tasty_doc_guards::source_text`), 셸이 자기 렉서를 만들면 같은 물음에 답이 둘이 된다.
# **주석은 남기는 마스크**를 쓴다 — 이 게이트는 억제의 유무만이 아니라 **사유 주석의
# 유무**를 함께 묻기 때문이다. 주석까지 덮으면 그 물음의 답이 사라진다.
#
# 줄 번호는 보존되므로 보고 좌표는 **원본 경로:줄** 그대로다.
#
# 판정기가 없거나 낡았으면 원문에서 센다 — 그쪽은 더 많이 세는 방향이라 조용한 통과를
# 안 만든다. 다만 래칫이라 **넘치면 실패한다**: 그래서 자동 채널이 판정기를 먼저 짓는다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
MASK_BIN="$(resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT")"

# **두 물음에 두 사본을 쓴다.** 억제가 *있는가* 는 코드에만 있어야 하므로 주석까지 덮은
# 사본에서 묻고, 사유 *주석*이 붙어 있는가 는 주석이 남은 사본에서 묻는다. 한 사본으로
# 둘을 다 하면 어느 쪽이든 틀린다 — 주석을 남기면 주석 안의 `#[allow(...)]` 언급이
# 억제로 세어지고(실측 1 자리), 주석을 덮으면 근거가 통째로 사라져 전부 위반이 된다.
# 줄 번호는 두 사본 모두 보존되므로 같은 i 로 맞물린다.
DET_ROOT="$ROOT"
TXT_ROOT="$ROOT"
if [ -n "$MASK_BIN" ]; then
    MASKED="$(mktemp -d)"
    trap 'rm -rf "$MASKED"' EXIT
    if "$MASK_BIN" "$MASKED/det" "$ROOT" crates src >/dev/null \
        && "$MASK_BIN" --keep-comments "$MASKED/txt" "$ROOT" crates src >/dev/null; then
        DET_ROOT="$MASKED/det"
        TXT_ROOT="$MASKED/txt"
    else
        echo "[allow-reason] 마스킹 실패 — 원문에서 센다(문자열·주석 안의 억제 형태까지 세어진다)." >&2
    fi
else
    echo "[allow-reason] 원문에서 센다 — 문자열·주석 안의 억제 형태까지 세어진다." >&2
fi

REASON_PATTERN='reason:|이유:|complexity-exempt:|SAFETY'

# 래칫 상한. **실제 건수와 같아야 한다** — 크면 그 차이만큼 조용히 받아준다.
# 줄였으면 이 수도 같이 내려라(스크립트가 그 자리에서 시킨다).
#
# 이 수는 술어를 넓힌 회차에 다시 잰 값이다(234 → 231). **위반이 줄어서가 아니다** —
# 세는 형태가 60 자리 늘고(조건부 억제) 인정하는 표기가 41 자리 늘어(한글 마커·블록 창)
# 두 변화가 상쇄된 값이다. 그래서 이 231 은 이전 234 와 **같은 것을 센 수가 아니다.**
#
# 184 → 183 → 182 도 같은 성질이다. 위반이 줄어서가 아니라 **언급을 실물로 세던 자리**가
# 마스킹으로 빠진 것이다: 184 → 183 은 문자열 안의 억제(생성 코드를 조립하는 자리),
# 183 → 182 는 **주석 안의 억제 언급** 한 자리다(실측으로 그 한 줄을 고쳐 확인했다).
# 판정기가 없어 원문에서 세면 이 값이 184 로 돌아와 래칫이 실패한다 — 자동 채널이
# 판정기를 먼저 짓는 이유다.
CAP=182

report=""
count=0

while IFS= read -r file; do
    [ -z "$file" ] && continue
    # 사본에 없는 파일(마스커와 rg 의 파일 집합이 완전히 같지는 않다)은 **그 파일만**
    # 원문에서 센다 — 건너뛰면 모수가 조용히 줄고, 줄어든 모수는 언제나 초록이다.
    det="$DET_ROOT/$file"
    [ -f "$det" ] || det="$file"
    txt="$TXT_ROOT/$file"
    [ -f "$txt" ] || txt="$file"
    hits=$(awk -v reason_pat="$REASON_PATTERN" '
        # 첫 파일 = 탐지용(주석까지 덮은 사본), 둘째 = 근거용(주석이 남은 사본).
        FNR == NR { det[FNR] = $0; ndet = FNR; next }
        { txt[FNR] = $0 }
        END {
            for (i = 1; i <= ndet; i++) {
                line = det[i]
                # 무조건 억제와 조건부 억제(`cfg_attr(..., allow(...))`)를 함께 센다.
                if (line !~ /#!?\[allow\(/ && !(line ~ /#!?\[cfg_attr\(/ && line ~ /allow\(/)) {
                    continue
                }
                found = (txt[i] ~ reason_pat)
                # 바로 위에 붙은 주석 블록 전체를 본다 — 빈 줄이나 코드 줄에서 끊긴다.
                for (k = i - 1; k >= 1 && !found; k--) {
                    if (txt[k] !~ /^[[:space:]]*\/\//) break
                    if (txt[k] ~ reason_pat) found = 1
                }
                if (!found) printf "%d: %s\n", i, txt[i]
            }
        }
    ' "$det" "$txt")
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
