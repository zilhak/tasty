#!/usr/bin/env bash
# 통합 검사 파일의 read_dir( 출현 횟수를 마스킹 사본에서 세어 고정 상한과 비교한다.
# 함수 별칭이나 다른 표기는 찾지 못하며, 같은 개수의 추가·삭제는 상쇄된다.
# src 내부 단위 검사와 하위 모듈 파일은 집계하지 않는다. 정책은 docs/dev-guide/guard-population.md.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SCAN_SPECS=('tests/*.rs' 'crates/*/tests/*.rs')

# tests 바로 아래의 추적 파일만 센다. git pathspec은 하위 경로도 고를 수 있어 깊이를 별도로 제한한다.
SCAN_ONE_LAYER=""
SCAN_DIRS=()
for _spec in "${SCAN_SPECS[@]}"; do
    _re=${_spec//./\\.}
    _re=${_re//\*/[^\/]+}
    SCAN_ONE_LAYER+="|$_re"
    _root=${_spec%%/*}
    case " ${SCAN_DIRS[*]:-} " in *" $_root "*) ;; *) SCAN_DIRS+=("$_root") ;; esac
done
SCAN_ONE_LAYER="^(${SCAN_ONE_LAYER#|})\$"

FILES=$(git ls-files -- "${SCAN_SPECS[@]}" | grep -E "$SCAN_ONE_LAYER" || true)

if [ -z "$FILES" ]; then
    echo "통합 테스트 타깃을 찾지 못해 아무것도 안 봤다. 이 결과를 직접"
    echo "순회가 0건이라는 뜻으로 해석하지 마라."
    exit 2
fi

# 미추적 검사 파일이 있으면 누락된 개수로 상한을 조정하지 않도록 거부한다.
UNTRACKED=$(git ls-files --others --exclude-standard -- "${SCAN_SPECS[@]}" \
    | grep -E "$SCAN_ONE_LAYER" || true)

if [ -n "$UNTRACKED" ]; then
    echo "인덱스에 없는 통합 테스트 타깃이 있다 — 이 상태에서는 판정하지 않는다."
    echo "아래 미추적 파일이 Cargo 타깃일 수 있지만 현재 Git 파일 목록에는 없다."
    echo "누락된 집계로 상한을 판단하지 않도록 검사를 중단한다."
    printf '%s\n' "$UNTRACKED" | sed 's/^/  /'
    echo "git add 한 뒤 다시 돌려라."
    exit 2
fi

# 원문에 든 합성 입력과 주석을 실제 호출로 세지 않도록 마스킹한다.
target_count=$(printf '%s\n' "$FILES" | wc -l)

. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
MASK_BIN="$JUDGE_BIN"

SCAN_ROOT="$ROOT"
if [ -n "$MASK_BIN" ]; then
    MASKED="$(mktemp -d)"
    trap 'rm -rf "$MASKED"' EXIT
    if "$MASK_BIN" "$MASKED/det" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null; then
        SCAN_ROOT="$MASKED/det"
    else
        echo "[shared-walk] 마스킹 실패 — 세지 않고 판정 불가로 끝낸다." >&2
    fi
else
    echo "[shared-walk] 판정기가 없다 — 세지 않고 판정 불가로 끝낸다." >&2
fi

# 상한과 실제 개수가 같아야 한다. 감소 때도 수집·판독 변화인지 확인하고 상한을 조정한다.
CAP=54

if [ "$SCAN_ROOT" = "$ROOT" ]; then
    echo "훑을 통합 테스트 타깃 ${target_count}개 — 세지 않았다."
    echo
    echo "[shared-walk] 판정 불가 — 판정기가 없다(또는 낡았다)."
    echo "  원문에는 주석·문자열도 포함되므로 상한(${CAP})과"
    echo "  비교할 수 없다. 먼저 현재 소스의 마스킹 도구를 준비해라."
    echo
    echo "  상한을 바꾸지 말고 도구를 빌드해라:"
    echo "      cargo build -p tasty-doc-guards --bin mask-source"
    echo "      target/debug/mask-source --check-fresh ."
    echo "  --check-fresh의 rc=0으로 현재 소스와 일치하는지 확인해라."
    echo "  rebase 직후라면 이것이 첫 번째로 할 일이다."
    exit 2
fi

report=""
count=0

while IFS= read -r file; do
    [ -z "$file" ] && continue
    # 사본에서 빠진 파일은 원문으로 센다. 이 경우 주석·문자열도 집계될 수 있다.
    scan="$SCAN_ROOT/$file"
    [ -f "$scan" ] || scan="$file"
    # grep 오류도 빈 결과로 처리된다. 이 집계만으로 읽기 실패와 호출 부재를 구별하지 못한다.
    hits=$({ grep -o 'read_dir(' "$scan" || true; } | wc -l)
    [ "$hits" -eq 0 ] && continue
    report+="  ${file}: ${hits}"$'\n'
    count=$((count + hits))
done <<<"$FILES"

echo "통합 테스트 타깃 ${target_count}개를 훑었다."
echo "통합 테스트 타깃의 직접 read_dir( : ${count}건 (상한 ${CAP})"
echo

if [ "$count" -gt "$CAP" ]; then
    printf '%s' "$report"
    echo
    echo "직접 순회가 늘었다: ${count} > 상한 ${CAP}."
    echo "새 순회를 tasty_doc_guards::floored_walk::walk_with_floor 로 옮겨라 — 거기서는"
    echo "하한이 인자라 빠뜨릴 수 없고, 실패문과 '하한을 내려서 통과시키지 마라' 는"
    echo "금지를 공용 함수가 만든다. 파일을 모으면 walk_with_floor, 디렉토리를 모으면"
    echo "walk_dirs_with_floor 다(뒤쪽은 하한이 모은 수가 아니라 훑은 수에 걸린다)."
    echo "상한을 올려서 통과시키지 마라."
    exit 1
fi

if [ "$count" -lt "$CAP" ]; then
    echo "직접 순회가 ${count} 로 상한 ${CAP} 보다 적게 나왔다."
    echo
    echo "개수 감소는 원인을 말하지 않는다. 다음을 확인해라."
    echo "  1. 순회를 공용 헬퍼로 옮겼는지."
    echo "  2. 검사 대상 파일이 빠졌는지. 위의 '통합 테스트 타깃 N개'를 이전 결과와 비교해라."
    echo "     빈 목록은 exit 2로 거부하지만 일부 누락은 이 분기로 올 수 있다."
    echo "  3. 호출 수집·판독 조건이 바뀌었는지."
    echo "     수집·판독 오류를 고치기 전에 상한을 조정하지 마라."
    echo
    echo "실제 순회 감소임을 확인한 뒤에만 CAP 을 ${count} 로 내려라. 상한이 실제보다"
    echo "크면 그 차이만큼 새 순회를 허용하게 된다."
    exit 1
fi
