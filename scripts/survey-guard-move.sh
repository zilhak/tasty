#!/usr/bin/env bash
# 가드 이동 전에 경로 인용·수집 범위·실행 경로 설명의 영향 후보를 출력한다.
# 사용: bash scripts/survey-guard-move.sh <현재 파일> <목적지> (저장소 상대 경로).
# 목록의 기준은 docs/dev-guide/guard-relocation.md. 정규식 후보 조사이며 위반 판정이나 완전한 수집을 보장하지 않는다.
# 일부 검색 오류는 빈 결과와 구별하지 못한다. 이동·rebase 후에는 실제 가드도 실행해야 한다.
# 정상 조사 종료 0, 인자 오류 2. 도구별 검색 실패를 모두 rc 2로 반환하지는 않는다.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

die() { echo "$*" >&2; exit 2; }

[ $# -eq 2 ] || die "사용법: bash scripts/survey-guard-move.sh <옮길-파일> <목적지-경로>"
FROM="$1"
TO="$2"
[ -f "$FROM" ] || die "옮길 파일이 없다: $FROM"
case "$FROM" in /*) die "레포 상대 경로로 줘라: $FROM" ;; esac
case "$TO"   in /*) die "레포 상대 경로로 줘라: $TO" ;; esac

# 아래 ROOT는 저장소 경로의 첫 구성 요소다. 실제 검사기의 수집 범위와 같지는 않다.
FROM_ROOT="${FROM%%/*}"
TO_ROOT="${TO%%/*}"

EXCL=(--exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=dist)

section() {
    echo
    echo "━━━ $1"
}
cmd() { echo "    \$ $1"; }
emit() {
    local body="$1" empty_msg="$2"
    if [ -z "$body" ]; then
        echo "    (없음) $empty_msg"
    else
        printf '%s\n' "$body" | sed 's/^/    /'
    fi
}

echo "가드 이동 조사: $FROM  →  $TO"
echo "최상위 경로: $FROM_ROOT → $TO_ROOT"
if [ "$FROM_ROOT" = "$TO_ROOT" ]; then
    echo "두 최상위 경로가 같다. 최상위 경로만 보는 검사에서는 범위가 같지만,"
    echo "  하위 경로까지 제한하는 검사(예: crates/*/src/만)는 결과가 달라질 수 있다."
fi

section "(가) 이동 후 갱신할 옛 경로 인용"
cmd "grep -rn '$FROM' --exclude-dir=target … ."
CITES="$(grep -rn -- "$FROM" "${EXCL[@]}" . 2>/dev/null | grep -v "^\./$FROM:" || true)"
emit "$CITES" "옛 경로를 가리키는 자리가 없다."
echo
echo "    ※ 전부 갱신 대상은 아니다. **픽스처**(파서 시험의 합성 입력, 술어 probe)는 그"
echo "      파일을 가리키는 인용이 아니라 경로의 **모양**을 시험하는 것이라 그대로 둔다."
echo "      가르는 법: 그 문자열이 없어지면 시험이 뜻을 잃는가, 아니면 그냥 다른 예로"
echo "      바꿔도 되는가."

section "M-c 위치에 따라 달라지는 기준 경로 후보"
cmd "grep -n 'CARGO_MANIFEST_DIR' $FROM"
MANIFEST_USE="$(grep -n "CARGO_MANIFEST_DIR" "$FROM" || true)"
if [ -z "$MANIFEST_USE" ]; then
    echo "    (없음) 이 파일에서 CARGO_MANIFEST_DIR을 찾지 못했다. 간접 호출은 별도 확인이 필요하다."
else
    printf '%s\n' "$MANIFEST_USE" | sed 's/^/    /'
    echo
    echo "    각 사용처에서 경로를 어떻게 조합하는지 확인해라. .parent() / pop()이"
    echo "      없으면 시작점이 해당 패키지일 수 있다. CARGO_MANIFEST_DIR은 루트 패키지에서는"
    echo "      레포 루트지만 crates/<이름>/tests/ 에서는 그 크레이트 디렉토리다."
    echo "      다음 세 줄의 parent/pop 표지만 찾은 결과다. 실제 경로까지 증명하지는 않는다:"
    AWKED="$(awk -v f="$FROM" '
        /CARGO_MANIFEST_DIR/ {
            ctx = $0
            for (i = 1; i <= 3; i++) { if ((getline nxt) > 0) ctx = ctx "\n" nxt; else break }
            up = (ctx ~ /parent|pop\(\)/) ? "parent/pop 표지 있음 — 실제 경로 확인" : "parent/pop 표지 없음 — 기준 경로 확인"
            print NR ": " up
        }' "$FROM" || true)"
    emit "$AWKED" "문맥을 못 읽었다."
    echo "      저장소 루트가 필요하다면 tasty_doc_guards::repo_root()를 검토해라. 표지 파일로"
    echo "      자기가 잡은 경로를 검증한다."
fi

section "M-d 자동 실행 경로 설명의 변경 후보"
CHANNEL_RE='check-headless|자동 채널|수동 전용|doc-guards|자동 실행은|자동으로 돈다|paths-ignore|경로 필터'
cmd "grep -nE '<채널 표지>' $FROM   그리고 이 파일을 지목하는 파일들"
OWN="$(grep -nE "$CHANNEL_RE" "$FROM" || true)"
echo "    [이 파일 자신]"
emit "$OWN" "등록된 실행 경로 표지를 찾지 못했다."
echo
echo "    [이 파일을 지목하는 다른 파일 — 주장하는 파일과 주장 대상은 다를 수 있다]"
BASENAME="$(basename "$FROM")"
OTHERS=""
while IFS= read -r hf; do
    [ -z "$hf" ] && continue
    [ "$hf" = "./$FROM" ] && continue
    # 파일명 언급 앞뒤 세 줄의 표지만 찾는다. 정확한 주장 대상은 사람이 확인해야 한다.
    NEAR="$(awk -v name="$BASENAME" -v re="$CHANNEL_RE" '
        { line[NR] = $0 }
        END {
            for (i = 1; i <= NR; i++) {
                if (index(line[i], name) == 0) continue
                lo = (i - 3 < 1) ? 1 : i - 3
                hi = (i + 3 > NR) ? NR : i + 3
                for (j = lo; j <= hi; j++) {
                    if (line[j] ~ re) { print i ": " substr(line[i], 1, 110); break }
                }
            }
        }' "$hf" || true)"
    [ -n "$NEAR" ] && OTHERS="${OTHERS}[$hf]
$NEAR
"
done <<< "$(grep -rln -- "$BASENAME" "${EXCL[@]}" . 2>/dev/null || true)"
emit "$OTHERS" "파일명 언급 주변에서 등록된 실행 경로 표지를 찾지 못했다."
echo
echo "    자동 실행 설명은 다음 검사도 실행해 확인해라:"
echo "      cargo test -p tasty-doc-guards --test ci_channel_claims_match_workflows"
echo "      검사가 지원하는 설명 형식과 실제 workflow를 함께 확인해라."

section "M-a 목적지 경로를 읽을 수 있는 검사 후보"
cmd "grep -rln '\"$TO_ROOT\"' <뿌리 목록 상수를 가진 타깃>"
# 상수 목록뿐 아니라 경로 접두를 직접 비교하는 검사기도 후보로 찾는다.
ROOT_DECLS="$(grep -rln -E "ROOTS: &\[|SCAN_ROOTS|fn is_scan_target|starts_with\(\"$TO_ROOT/" --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
CANDS=""
while IFS= read -r f; do
    [ -z "$f" ] && continue
    if grep -qE "\"$TO_ROOT(\"|/)" "$f" 2>/dev/null; then
        # 옮기는 검사기가 새 위치에서 자기 소스도 읽게 될 수 있다.
        if [ "$f" = "$FROM" ]; then
            CANDS="$CANDS$f   ← 옮기는 검사기 자신이다. 자기 소스를 읽게 되는지 M-b도 확인해라.
"
        else
            CANDS="$CANDS$f
"
        fi
    fi
done <<< "$ROOT_DECLS"
emit "$CANDS" "등록된 검색 패턴으로 목적지 경로를 읽는 후보를 찾지 못했다."
echo
echo "    ※ 후보 목록이다. '$TO_ROOT'를 포함해도 하위 경로를 제한한다면"
echo "      목적지는 여전히 검사 대상이 아닐 수 있다 — 예: crates/*/src/ 만 보면서 /tests/ 를 배제하는"
echo "      술어는 crates 를 뿌리로 갖고도 crates/<이름>/tests/ 를 안 센다."
echo "      각 후보의 is_scan_target 같은 조건에서 목적지를 실제로 포함하는지 확인해라."

section "M-e 디렉터리별 하한 — 전체 수가 같아도 개별 디렉터리 수는 달라진다"
cmd "grep -rn '(\"$FROM_ROOT\", <수>)' 및 '(\"$TO_ROOT\", <수>)' 형태"
PER_ROOT="$(grep -rn -E "\(\"($FROM_ROOT|$TO_ROOT)\", *[0-9]+\)" --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
echo "    [뿌리별 하한 — 출발지 쪽이 줄고 목적지 쪽이 는다]"
emit "$PER_ROOT" "뿌리별 하한을 건 자리가 없다."
echo
echo "    [총수 하한 — 이동만으로는 합이 안 변하므로 대체로 무감이지만, 함께 적어 둔다]"
TOTALS="$(grep -rn -E 'const (MIN_[A-Z_]+|[A-Z_]+_FLOOR): usize' --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
TOTAL_N="$(printf '%s' "$TOTALS" | grep -c . || true)"
echo "    총수 하한 상수 $TOTAL_N 개 (전부 출력하지 않는다. 전체 범위도 달라지면 따로 확인해라)"
echo
echo "    수집 디렉터리가 같아도 개별 하한의 유무에 따라 결과가 다를 수 있다."
echo "      no_unserialized_env_mutation과 no_unshared_fixed_temp_path처럼 SCAN_ROOTS가"
echo "      같은 검사도(src·crates·tests) 디렉터리별 하한을 각각 확인해라."

section "M-b 이동한 파일의 내용이 새 검사 대상이 되는지 확인"
if command -v python3 >/dev/null 2>&1; then
    cmd "python3 — 금지 코드포인트(U+1F000..1FAFF, U+1F1E6..1F1FF) 계수"
    EMOJI="$(python3 -c '
import io,sys,collections
s = io.open(sys.argv[1], encoding="utf-8").read()
c = collections.Counter()
for i, ch in enumerate(s):
    o = ord(ch)
    if 0x1F000 <= o <= 0x1FAFF or 0x1F1E6 <= o <= 0x1F1FF:
        c[s[:i].count("\n") + 1] += 1
if c:
    print("총 %d 개" % sum(c.values()))
    for line, n in sorted(c.items()):
        print("  line %d: %d" % (line, n))
' "$FROM" 2>/dev/null || true)"
    emit "$EMOJI" "금지 코드포인트가 없다."
else
    echo "    (미측정) python3 가 없어 코드포인트를 못 셌다 — 0 이 아니라 미측정이다."
fi
echo
echo "    이 파일에서 찾은 검사 함수 후보:"
PREDS="$(grep -nE '^(fn|    fn) (is_|has_|no_)[a-z_]+' "$FROM" || true)"
emit "$PREDS" "등록된 이름 패턴에 맞는 함수를 찾지 못했다."
echo
echo "    내용과 검사 범위를 확인한 뒤 처리해라:"
echo "      · 이모지를 설명하는 산문이면 U+XXXX 같은 표기로 바꿀 수 있다. 예외부터 늘리지 마라."
echo "      · 검사가 목적지 경로를 제외한다면 그 검사 때문에 내용을 바꿀 필요는 없다."
echo "      · mask_non_code로 문자열을 제외하는 검사라면 문자열 속 합성 입력은 그대로 유지한다."
echo "      예외가 필요하다면 근거를 확인하고 파일 전체보다 해당 위치로 한정해라."

section "이동 뒤 출발지와 목적지 양쪽의 검사를 실행해라"
echo "    cargo test -p tasty-doc-guards --no-fail-fast"
echo "    cargo test -p tasty --lib --no-fail-fast"
echo "    bash scripts/check-allow-reason.sh"
echo "    bash scripts/check-shared-walk-ratchet.sh"
echo
echo "    ※ -p <크레이트> 는 루트 패키지의 통합 타깃을 안 돌리고, 그 반대도 마찬가지다."
echo "      이동은 두 패키지를 건드리므로 한쪽만 돌리면 절반이 미측정이다."
echo "    ※ 파이프를 썼다면 Cargo의 종료 코드와 전체 test result를 함께 확인해라."
echo "    ※ 옮긴 타깃을 옛 패키지 이름으로 부르면 그 타깃은 **안 돈다**. 없는 이름을 주면"
echo "      오류가 나지만 --test를 생략하면 원래 실행하려던 타깃이 빠져도 모를 수 있다."
echo "    ※ 루트 단위 검사는 --lib로 실행해라. --bins만 실행하면"
echo "      lib 타깃의 검사를 실행하지 않는다."

section "(가′) 이동 이후 다른 변경에서 추가된 옛 경로 인용"
echo "    이 조사 이후 다른 변경이 옛 경로를 추가로 인용할 수 있다."
echo "    현재 조사 결과만으로 이후 변경까지 확인했다고 볼 수 없다."
echo "    각 변경을 따로 검사할 때는 인용 경로가 유효했더라도"
echo "    두 변경을 합치면 경로가 사라질 수 있다."
echo
echo "    rebase 후 합쳐진 트리에서도 경로 검사를 실행해라:"
echo "      cargo test -p tasty-doc-guards --locked --test cited_coordinates_exist"

echo
echo "조사 끝. 판정은 사람이 한다 — 위 어느 항목도 그 자체로 위반이 아니다."
