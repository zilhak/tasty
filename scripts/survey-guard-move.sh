#!/usr/bin/env bash
# 가드 파일을 옮기기 **전에** 그 이동이 무엇을 건드리는지 재는 조사 도구.
#
# 이름이 `check-` 가 아닌 이유가 있다. 이 레포에서 `check-*.sh` 는 전부 **판정**한다 —
# 위반이면 0 이 아닌 값으로 끝나고 훅이나 CI 가 그걸로 커밋을 막는다. 이 도구는 판정하지
# 않는다. 축마다 **찾은 것과 찾은 명령**을 찍고 판단은 사람에게 넘긴다. 접두를 빌리면
# 다음 사람이 이것을 게이트로 알고 훅에 걸게 되고, 그 순간 **정당한 예외가 빨강이 된다** —
# 아래 축들에는 예외가 실제로 있다(M-b 만 해도 세 갈래인데 그중 둘은 처방이 "아무것도 하지
# 않는다" 이다).
#
# ## 왜 이 도구가 필요한가
#
# 파일을 옮기면 두 방향으로 깨지는데 채널은 한쪽에만 있다.
#   (가) 옛 경로를 가리키는 인용이 죽는다 — 전수 grep 이 잡는다.
#   (나) 새 경로가 다른 스캐너의 뿌리 안으로 들어가 그 스캐너가 그것을 세기 시작한다 —
#        아무도 안 본다. 늘어난 항이 우연히 통과하면 아무 일도 안 난다.
# (나)가 (가)보다 조용하다. (가)는 "읽지 못했다" 로 죽지만 (나)는 모수가 하나 늘 뿐이다.
# 그리고 옮기는 사람이 도는 테스트는 자기 크레이트라, (나)는 **그 사람의 모수 밖**에 있다.
#
# ## 축 일곱
#
# 정본은 `docs/dev-guide/guard-relocation.md` 다. 아래는 그 목록의 **사본**이고, 축을
# 더하거나 이름을 바꾸면 **두 곳을 함께** 고쳐야 한다 — 한쪽만 고치면 두 곳이 서로 다른
# 축 체계를 말하게 되고 그것은 조용하다. 각 축의 판정법과 처방은 그 문서에 있다.
#
#   M-a  새 뿌리가 그것을 세기 시작한다
#   M-b  세기 시작하는데 그 내용이 위반의 모양이다
#   M-c  뿌리를 얻는 **표현식**이 위치 의존이라 뿌리가 파일을 따라 움직인다
#   M-d  파일이 자기 **채널에 대한 주장**을 담고 있어 위치가 바뀌면 거짓이 된다
#   M-e  뿌리 **목록**이 같아도 뿌리별 **하한**이 있으면 합이 불변인 채로 하나가 뚫린다
#   (가) 옛 경로를 가리키는 인용
#   (가′) 이동 **뒤에** 생긴 인용 — 이 도구는 못 잰다(아래)
#
# ## 이 주석이 예시 경로를 형태로만 드는 이유
#
# 이 파일도 방금 그 부류에 걸렸다. 사용법 예시를 레포 경로 꼴(`<디렉토리>/<파일>.rs`)로
# 적었더니 `cited_coordinates_exist` 가 그것을 인용으로 읽고 "따라갈 곳이 없다" 로 빨개졌다.
# 그 가드 자신이 같은 함정에 걸린 뒤 "예시는 형태로 든다" 로 처방을 적어 뒀고, 여기서도
# 그렇게 한다 — 실재하는 경로를 예시로 쓰면 그 파일이 움직일 때 이 설명이 죽는다.
#
# ## 이 도구가 못 하는 것
#
# M-a 와 M-d 는 **후보만** 낸다. 정확히 답하려면 순회 범위를 소스에서 읽어야 하고 그것은
# 임의의 코드라 언제나 근사가 된다(`tasty_doc_guards::floored_walk` 모듈 주석에 그 시도가
# 세 번 반례를 맞은 기록이 있다). 그래서 여기서는 **읽을 자리를 좁혀 주는 것**까지만 한다.
# M-b 도 절반만 기계적이다 — 이모지는 셀 수 있지만 "이 파일이 담은 것이 저 가드에게
# 위반인가" 는 가드마다 다르다.
#
# **(가′) 는 이 도구가 원리적으로 못 잰다.** 이 도구는 이동 **전에** 도는데, 그 축은 다른
# 사람이 이동 **뒤에** 쓴 문단이 옛 경로를 인용하는 것이다 — 지금 없는 글을 볼 수 없다.
# 그 축이 잡히는 자리는 조사가 아니라 **rebase 직후**이고, 아래 마지막 절이 그 명령을 찍는다.
#
# 사용법:  bash scripts/survey-guard-move.sh <옮길-파일> <목적지-경로>
#   예:  루트 통합 타깃 하나를 의존 0 크레이트의 tests 아래로 옮기는 경우
# 두 경로 모두 **레포 상대**로 준다. 목적지는 아직 없어도 된다(이동 전에 쓰는 도구다).
#
# 끝값: 0 = 재기를 마쳤다(위반 유무와 무관). 2 = 잴 수 없었다(인자가 틀렸거나 도구 부재).

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

# 뿌리 = 경로의 첫 조각. 첫 슬래시 앞이 그대로 뿌리 이름이 된다
# (루트 통합 타깃이면 그 디렉토리 이름, 크레이트 아래면 `crates`).
FROM_ROOT="${FROM%%/*}"
TO_ROOT="${TO%%/*}"

# 스캔에서 뺄 곳. 빌드 산출물과 VCS.
EXCL=(--exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=dist)

section() {
    echo
    echo "━━━ $1"
}
cmd() { echo "    \$ $1"; }
# 결과를 찍는다. 빈손이면 그렇게 말한다 — 빈손과 "안 쟀다" 는 다르다.
emit() {
    local body="$1" empty_msg="$2"
    if [ -z "$body" ]; then
        echo "    (없음) $empty_msg"
    else
        printf '%s\n' "$body" | sed 's/^/    /'
    fi
}

echo "가드 이동 조사: $FROM  →  $TO"
echo "뿌리: $FROM_ROOT → $TO_ROOT"
if [ "$FROM_ROOT" = "$TO_ROOT" ]; then
    echo "★ 두 뿌리가 같다. 뿌리 목록으로 소속이 갈리는 축(M-a·M-e)은 대체로 무감이지만,"
    echo "  뿌리 **아래**를 더 좁게 보는 술어(예: crates/*/src/ 만)는 여전히 갈릴 수 있다."
fi

# ── (가) 옛 경로를 가리키는 인용 ──────────────────────────────────────
section "(가) 옛 경로 인용 — 이동하면 죽는다"
cmd "grep -rn '$FROM' --exclude-dir=target … ."
CITES="$(grep -rn -- "$FROM" "${EXCL[@]}" . 2>/dev/null | grep -v "^\./$FROM:" || true)"
emit "$CITES" "옛 경로를 가리키는 자리가 없다."
echo
echo "    ※ 전부 갱신 대상은 아니다. **픽스처**(파서 시험의 합성 입력, 술어 probe)는 그"
echo "      파일을 가리키는 인용이 아니라 경로의 **모양**을 시험하는 것이라 그대로 둔다."
echo "      가르는 법: 그 문자열이 없어지면 시험이 뜻을 잃는가, 아니면 그냥 다른 예로"
echo "      바꿔도 되는가."

# ── M-c 뿌리를 얻는 표현식 ────────────────────────────────────────────
section "M-c 뿌리 표현식이 위치 의존인가 — 기계적으로 답한다"
cmd "grep -n 'CARGO_MANIFEST_DIR' $FROM"
MANIFEST_USE="$(grep -n "CARGO_MANIFEST_DIR" "$FROM" || true)"
if [ -z "$MANIFEST_USE" ]; then
    echo "    (없음) 이 파일은 CARGO_MANIFEST_DIR 을 안 쓴다 — M-c 무감."
else
    printf '%s\n' "$MANIFEST_USE" | sed 's/^/    /'
    echo
    echo "    ★ 각 자리가 **올라가는지** 봐라. 같은 줄이나 뒤 몇 줄에 .parent() / pop() 이"
    echo "      없으면 그 표현식은 **이 타깃이 사는 패키지**를 가리킨다 — 루트 패키지에서는"
    echo "      레포 루트지만 crates/<이름>/tests/ 에서는 그 크레이트 디렉토리다."
    echo "      올라가는 자리(무해)와 안 올라가는 자리(수선 대상)를 가른 문맥:"
    AWKED="$(awk -v f="$FROM" '
        /CARGO_MANIFEST_DIR/ {
            ctx = $0
            for (i = 1; i <= 3; i++) { if ((getline nxt) > 0) ctx = ctx "\n" nxt; else break }
            up = (ctx ~ /parent|pop\(\)/) ? "올라감(무해)" : "★ 안 올라감 — repo_root() 로 바꿔라"
            print NR ": " up
        }' "$FROM" || true)"
    emit "$AWKED" "문맥을 못 읽었다."
    echo "      이 레포에는 이미 답이 있다: tasty_doc_guards::repo_root() 는 표지 파일로"
    echo "      자기가 잡은 경로를 검증한다."
fi

# ── M-d 채널 주장 ─────────────────────────────────────────────────────
section "M-d 채널에 대한 주장 — 위치가 바뀌면 거짓이 될 수 있다 (후보)"
CHANNEL_RE='check-headless|자동 채널|수동 전용|doc-guards|자동 실행은|자동으로 돈다|paths-ignore|경로 필터'
cmd "grep -nE '<채널 표지>' $FROM   그리고 이 파일을 지목하는 파일들"
OWN="$(grep -nE "$CHANNEL_RE" "$FROM" || true)"
echo "    [이 파일 자신]"
emit "$OWN" "자기 채널을 주장하지 않는다."
echo
echo "    [이 파일을 지목하는 다른 파일 — 주장하는 파일과 주장 대상은 다를 수 있다]"
BASENAME="$(basename "$FROM")"
OTHERS=""
while IFS= read -r hf; do
    [ -z "$hf" ] && continue
    [ "$hf" = "./$FROM" ] && continue
    # 지목 줄과 채널 표지 줄이 **가까운** 것만 낸다. 파일 어딘가에 표지가 있다는 것만으로는
    # 이 타깃에 대한 주장이 아니다 — `ci_channel_claims_match_workflows` 가 보는 단위도
    # 파일이 아니라 마크다운 항목 하나 · 이어진 주석 블록 하나다.
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
    [ -n "$NEAR" ] && OTHERS="$OTHERS[$hf]
$NEAR
"
done <<< "$(grep -rln -- "$BASENAME" "${EXCL[@]}" . 2>/dev/null || true)"
emit "$OTHERS" "이 타깃을 지목하면서 채널을 주장하는 파일이 없다."
echo
echo "    ※ 판정 채널이 하나 있다 — 이 축만은 조용하지 않다:"
echo "      cargo test -p tasty-doc-guards --test ci_channel_claims_match_workflows"
echo "      그 가드는 주장하는 파일이 누구인지 안 보고 **지목**만으로 잡는다."

# ── M-a 목적지를 세기 시작하는 스캐너 ─────────────────────────────────
section "M-a 목적지 뿌리를 세는 스캐너 (후보)"
cmd "grep -rln '\"$TO_ROOT\"' <뿌리 목록 상수를 가진 타깃>"
# 소속을 정하는 자리는 **상수 목록만이 아니다.** 뿌리를 상수로 선언하지 않고 술어 함수로
# 판정하는 스캐너가 있고(`fn is_scan_target` 이 경로 접두를 직접 본다), 그런 자리는 `ROOTS`
# 를 찾는 grep 으로 영원히 안 나온다 — R937 이 "뿌리 목록이 아니라 뿌리를 얻는 표현식" 이었던
# 것과 같은 종류의 구멍이다. 그래서 셋을 함께 찾는다: 뿌리 상수 · 소속 술어 · 경로 접두 판정.
ROOT_DECLS="$(grep -rln -E "ROOTS: &\[|SCAN_ROOTS|fn is_scan_target|starts_with\(\"$TO_ROOT/" --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
CANDS=""
while IFS= read -r f; do
    [ -z "$f" ] && continue
    # `"crates"`(뿌리 목록 원소)와 `"crates/`(경로 접두 판정) 둘 다 받는다. 하나만 보면
    # 소속을 술어로 정하는 스캐너를 통째로 놓친다.
    if grep -qE "\"$TO_ROOT(\"|/)" "$f" 2>/dev/null; then
        # ★ 옮기는 파일 자신이 후보로 나오는 것은 오류가 아니라 **이 부류의 대표 사고**다.
        #    스캐너를 그 스캐너의 뿌리 안으로 옮기면 자기 자신을 세기 시작한다.
        if [ "$f" = "$FROM" ]; then
            CANDS="$CANDS$f   ← ★ 옮기는 파일 자신이다. 이 스캐너가 **자기를 세기 시작한다** — M-b 를 반드시 함께 봐라.
"
        else
            CANDS="$CANDS$f
"
        fi
    fi
done <<< "$ROOT_DECLS"
emit "$CANDS" "목적지 뿌리를 뿌리 목록에 담은 타깃이 없다."
echo
echo "    ※ **후보다.** 뿌리에 '$TO_ROOT' 가 있어도 그 아래를 더 좁히는 술어가 있으면"
echo "      목적지는 여전히 모수 밖이다 — 예: crates/*/src/ 만 보면서 /tests/ 를 배제하는"
echo "      술어는 crates 를 뿌리로 갖고도 crates/<이름>/tests/ 를 안 센다."
echo "      각 후보에서 is_scan_target 류 술어를 열어 목적지 경로를 먹여 봐라."

# ── M-e 뿌리별 하한 ───────────────────────────────────────────────────
section "M-e 뿌리별 하한 — 합이 불변인 채로 한 뿌리가 뚫린다"
cmd "grep -rn '(\"$FROM_ROOT\", <수>)' 및 '(\"$TO_ROOT\", <수>)' 형태"
PER_ROOT="$(grep -rn -E "\(\"($FROM_ROOT|$TO_ROOT)\", *[0-9]+\)" --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
echo "    [뿌리별 하한 — 출발지 쪽이 줄고 목적지 쪽이 는다]"
emit "$PER_ROOT" "뿌리별 하한을 건 자리가 없다."
echo
echo "    [총수 하한 — 이동만으로는 합이 안 변하므로 대체로 무감이지만, 함께 적어 둔다]"
TOTALS="$(grep -rn -E 'const (MIN_[A-Z_]+|[A-Z_]+_FLOOR): usize' --include='*.rs' "${EXCL[@]}" src crates tests 2>/dev/null || true)"
TOTAL_N="$(printf '%s' "$TOTALS" | grep -c . || true)"
echo "    총수 하한 상수 $TOTAL_N 개 (전부 찍지 않는다 — 위 뿌리별 목록이 실제 위험이다)"
echo
echo "    ★ 뿌리 목록이 같아도 하한의 **형태**가 다르면 민감도가 다르다. 실측 본보기:"
echo "      no_unserialized_env_mutation 과 no_unshared_fixed_temp_path 는 SCAN_ROOTS 가"
echo "      같은데(src·crates·tests) 앞쪽만 뿌리별 하한을 걸어서, 같은 이동에 앞쪽만 터졌다."

# ── M-b 위반의 모양 ───────────────────────────────────────────────────
section "M-b 옮기는 파일이 목적지 판정의 위반 모양을 담는가 (절반만 기계적)"
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
echo "    이 파일이 정의한 술어(자기 자신을 세게 될 때 무엇이 위반이 되는지 읽을 자리):"
PREDS="$(grep -nE '^(fn|    fn) (is_|has_|no_)[a-z_]+' "$FROM" || true)"
emit "$PREDS" "술어 함수가 없다."
echo
echo "    ★ 처방이 셋으로 갈린다. 앞의 둘이면 아무것도 하지 않는다:"
echo "      · 담은 것이 **산문**이면 표기로 바꿔라(U+XXXX 등). 면제 명부를 늘리지 마라."
echo "      · 그 가드의 **술어가 목적지를 배제**하면 구조적 면역이다 — 무처방."
echo "      · 판정기가 **문자열을 마스킹**하면(mask_non_code) 원문 리터럴은 코드로 안 세어진다 — 무처방."
echo "      셋 다 아닐 때만 면제가 정당하다. 그때도 파일 통째가 아니라 자리 단위로."

# ── 이동 뒤 무엇을 돌릴 것인가 ────────────────────────────────────────
section "이동 뒤 돌릴 것 — 출발지와 목적지 **양쪽**을 돌려라"
echo "    cargo test -p tasty-doc-guards --no-fail-fast"
echo "    cargo test -p tasty --bins --no-fail-fast"
echo "    bash scripts/check-allow-reason.sh"
echo "    bash scripts/check-shared-walk-ratchet.sh"
echo
echo "    ※ -p <크레이트> 는 루트 패키지의 통합 타깃을 안 돌리고, 그 반대도 마찬가지다."
echo "      이동은 두 패키지를 건드리므로 한쪽만 돌리면 절반이 미측정이다."
echo "    ※ rc 는 파이프 끝 단계의 것이다. 판정은 'test result' 줄로 해라."
echo "    ※ 옮긴 타깃을 옛 패키지 이름으로 부르면 그 타깃은 **안 돈다**. 없는 이름을 주면"
echo "      시끄럽게 죽지만, --test 를 아예 안 붙이면 조용히 빠진다 — 뒤엣것이 위험하다."

section "이 도구가 못 재는 축 — (가′) 이동 **뒤에** 생긴 인용"
echo "    이 도구는 이동 전에 돈다. 그러니 다른 사람이 이동 **뒤에** 쓴 문단이 옛 경로를"
echo "    인용하는 것은 원리적으로 못 본다 — 지금 없는 글은 볼 수 없다."
echo "    어느 쪽도 혼자서는 틀리지 않는다. 그 문단이 쓰인 트리에서 그 경로는 실재했고,"
echo "    이동한 트리에는 그 문단이 없었다. **겹치는 순간에만** 죽는다."
echo
echo "    잡히는 자리는 조사가 아니라 rebase 직후 한 번이다:"
echo "      cargo test -p tasty-doc-guards --locked --test cited_coordinates_exist"

echo
echo "조사 끝. 판정은 사람이 한다 — 위 어느 항목도 그 자체로 위반이 아니다."
