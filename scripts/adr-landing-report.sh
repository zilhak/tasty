#!/usr/bin/env bash
# base..tip에서 바뀐 ADR의 제목과 Decision 백틱 인용을 보고한다.
# 같은 이름을 인용하는 후보만 찾으며 결정 내용의 중복 여부는 사람이 읽어 판단한다.
# 종료 코드: 보고 완료 0, Git 객체·목록 조회 실패 2. 후보가 있어도 실패하지 않는다.
# 사용: bash scripts/adr-landing-report.sh [--candidates-only] <base> <tip>
set -euo pipefail

quiet=0
if [ "${1:-}" = "--candidates-only" ]; then
    quiet=1
    shift
fi
if [ "$#" -ne 2 ]; then
    echo "사용: bash scripts/adr-landing-report.sh [--candidates-only] <base> <tip>" >&2
    exit 2
fi
base=$1
tip=$2

for rev in "$base" "$tip"; do
    if ! git rev-parse --verify --quiet "${rev}^{commit}" > /dev/null; then
        echo "[adr-landing] 판정 불가 — '${rev}' 를 커밋으로 풀지 못했다(없는 rev 이거나 얕은 clone 이면 git fetch)." >&2
        exit 2
    fi
done

if ! changes=$(git diff --name-status --diff-filter=AMR "$base" "$tip" -- 'docs/adr/[0-9]*.md'); then
    echo "[adr-landing] 판정 불가 — git diff 가 실패했다." >&2
    exit 2
fi

if [ -z "$changes" ]; then
    echo "[adr-landing] ${base}..${tip} 에서 추가·변경된 ADR 0 건 — 대조할 것이 없다."
    exit 0
fi

pairs=""
count=0
while IFS=$'\t' read -r status first second; do
    path=$first
    if [ -n "${second:-}" ]; then
        path=$second # R 은 새 경로가 둘째 칸이다
    fi
    num=$(basename "$path")
    num=${num:0:4}
    if ! body=$(git show "${tip}:${path}"); then
        echo "[adr-landing] 판정 불가 — ${tip}:${path} 를 읽지 못했다." >&2
        exit 2
    fi
    title=$(sed -n '1p' <<< "$body")
    count=$((count + 1))
    [ "$quiet" = 1 ] || echo "── ${status:0:1} ${title}"
    decision=$(awk '/^## Decision/{f=1; next} /^## /{f=0} f' <<< "$body")
# 백틱을 셸 확장이 아닌 검색 문자로 사용한다.
    # shellcheck disable=SC2016
    names=$(grep -o '`[^` ]\{4,\}`' <<< "$decision" | tr -d '`' | sort -u || true)
    if [ -z "$names" ]; then
        [ "$quiet" = 1 ] || echo "   (Decision 절에 백틱 인용 없음 — 제목으로 읽어라)"
        continue
    fi
    while IFS= read -r name; do
        [ "$quiet" = 1 ] || echo "   · ${name}"
        root=${name%%(*}
        root=${root%%::*}
        if [ "${#root}" -ge 4 ]; then
            pairs+="${root}"$'\t'"${num}"$'\n'
        fi
    done <<< "$names"
done <<< "$changes"

[ "$quiet" = 1 ] || echo
echo "[adr-landing] 대조 대상 ADR ${count} 건 (${base}..${tip}, 상태 A·M·R)"

candidates=$(printf '%s' "$pairs" | sort -u | awk -F'\t' '
    { list[$1] = (list[$1] == "" ? $2 : list[$1] " " $2); n[$1]++ }
    END { for (k in list) if (n[k] > 1) print k "\t" list[k] }' | sort)

if [ -z "$candidates" ]; then
    echo "[adr-landing] 같은 인용 이름에 둘 이상 걸린 ADR 없음. 제목은 위 목록으로 사람이 대조한다."
    exit 0
fi

echo "[adr-landing] 겹침 후보 — 같은 이름을 Decision 에서 인용한다. 두 Decision 을 읽고 같은 조항인지 판단하라:"
while IFS=$'\t' read -r name nums; do
    echo "   ${name}  ←  ${nums}"
done <<< "$candidates"
exit 0
