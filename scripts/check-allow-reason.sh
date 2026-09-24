#!/usr/bin/env bash
# allow와 cfg_attr의 allow를 줄 단위로 찾고 같은 줄·바로 위 주석 블록의 사유를 검사한다.
# 마커 뒤나 다음 주석 줄에 내용이 있어야 한다. 실제 근거의 타당성이나 여러 줄 attribute 전체는 판정하지 않는다.
# 개수가 CAP과 다르면 실패한다. 위반 추가와 제거가 상쇄되면 개수만으로는 발견하지 못한다.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Git pathspec에서 검색 디렉터리를 파생해 파일 수집과 마스킹 범위를 맞춘다.
SCAN_SPECS=('crates/*.rs' 'src/*.rs')
SCAN_DIRS=()
for _spec in "${SCAN_SPECS[@]}"; do
    _root=${_spec%%/*}
    case " ${SCAN_DIRS[*]:-} " in *" $_root "*) ;; *) SCAN_DIRS+=("$_root") ;; esac
done

# rg는 미추적 파일도, git ls-files는 인덱스 파일만 수집하므로 사용한 경로를 출력한다.
# 수집 명령 오류는 여기서 무시한다. 아래 빈 목록 검사는 일부만 누락된 경우까지 찾지 못한다.
if command -v rg >/dev/null 2>&1; then
    LEFT_SOURCE=rg
    FILES=$(rg --files -g '*.rs' "${SCAN_DIRS[@]}") || true
else
    LEFT_SOURCE=git-ls-files
    FILES=$(git ls-files -- "${SCAN_SPECS[@]}") || true
fi

# 빈 수집을 위반 0건으로 해석해 CAP을 내리지 않도록 별도 오류로 처리한다.
if [ -z "$FILES" ]; then
    echo "검사 대상 .rs를 찾지 못해 아무것도 안 봤다. 이 결과를 근거 없는" >&2
    echo "억제가 0건이라는 뜻으로 해석하거나 상한을 내리지 마라." >&2
    exit 2
fi

# Git 분기는 미추적 Rust 파일을 세지 못하므로 그 파일이 있으면 판정을 거부한다.
if [ "$LEFT_SOURCE" = git-ls-files ]; then
    UNTRACKED=$(git ls-files --others --exclude-standard -- "${SCAN_SPECS[@]}") || true
    if [ -n "$UNTRACKED" ]; then
        echo "인덱스에 없는 .rs 가 있다 — 이 상태에서는 판정하지 않는다." >&2
        echo "git ls-files는 아래 미추적 파일을 검사하지 못한다. 컴파일에 쓰이는 파일일 수 있으므로" >&2
        echo "인덱스에 추가한 뒤 다시 검사해라. 누락된 결과를 근거로 상한을 내리지 마라." >&2
        printf '%s\n' "$UNTRACKED" | sed 's/^/  /' >&2
        echo "git add 한 뒤 다시 돌려라." >&2
        exit 2
    fi
fi

# 문자열·주석의 allow 예시를 실제 억제로 세지 않도록 공용 mask-source를 사용한다.
scanned_count=$(printf '%s\n' "$FILES" | wc -l)

. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
MASK_BIN="$JUDGE_BIN"

# 탐지에는 문자열·주석을 지운 사본, 사유에는 문자열만 지운 사본을 쓴다. 줄 번호는 같다.
DET_ROOT="$ROOT"
TXT_ROOT="$ROOT"
if [ -n "$MASK_BIN" ]; then
    MASKED="$(mktemp -d)"
    trap 'rm -rf "$MASKED"' EXIT
    if "$MASK_BIN" "$MASKED/det" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null \
        && "$MASK_BIN" --keep-comments "$MASKED/txt" "$ROOT" "${SCAN_DIRS[@]}" >/dev/null; then
        DET_ROOT="$MASKED/det"
        TXT_ROOT="$MASKED/txt"
    else
        echo "[allow-reason] 마스킹 실패 — 세지 않고 판정 불가로 끝낸다." >&2
    fi
else
    echo "[allow-reason] 판정기가 없다 — 세지 않고 판정 불가로 끝낸다." >&2
fi

# 마커와 내용이 함께 있어야 한다. awk split이 정규식을 쓰므로 마커 목록 구분자는 | 대신 ;다.
REASON_MARKERS='reason:;이유:;complexity-exempt:;SAFETY[[:space:]]*:'

# 실제 잔여 수와 같아야 한다. 감소 시에도 수집 누락인지 확인한 뒤 CAP을 조정한다.
CAP=173

# 마스킹 도구가 없거나 실패했으면 원문 수를 CAP과 비교하지 않는다.
if [ "$DET_ROOT" = "$ROOT" ]; then
    echo "훑을 .rs ${scanned_count}개 (좌변=${LEFT_SOURCE}) — 세지 않았다."
    echo
    echo "[allow-reason] 판정 불가 — 판정기가 없다(또는 낡았다)."
    echo "  문자열·주석을 구분하지 않은 원문 결과는 상한(${CAP})과"
    echo "  비교할 수 없다. 먼저 현재 소스의 마스킹 도구를 빌드해라."
    echo
    echo "  상한을 바꾸지 말고 도구를 준비해라:"
    echo "      cargo build -p tasty-doc-guards --bin mask-source"
    echo "      target/debug/mask-source --check-fresh ."
    echo "  --check-fresh가 rc=0이어야 현재 소스로 만든 도구를 사용할 수 있다."
    echo "  rebase 직후라면 이것이 첫 번째로 할 일이다."
    exit 2
fi

report=""
count=0

while IFS= read -r file; do
    [ -z "$file" ] && continue
    # 수집됐지만 마스킹 사본에 없는 파일은 현재 구현상 원문으로 검사한다.
    det="$DET_ROOT/$file"
    [ -f "$det" ] || det="$file"
    txt="$TXT_ROOT/$file"
    [ -f "$txt" ] || txt="$file"
    hits=$(awk -v markers="$REASON_MARKERS" '
        # 첫 입력은 억제 탐지용, 둘째는 사유 주석 조회용이다.
        function reason_here(i, arr, n,   p, nm, M, t, nx, sawmarker) {
            nm = split(markers, M, ";")
            sawmarker = 0
            for (p = 1; p <= nm; p++) {
                if (!match(arr[i], M[p])) continue
                sawmarker = 1
                t = substr(arr[i], RSTART + RLENGTH)
                gsub(/^[[:space:]]+/, "", t)
                if (t != "") return 1
            }
            if (!sawmarker) return 0
            if (i + 1 <= n && arr[i + 1] ~ /^[[:space:]]*\/\//) {
                nx = arr[i + 1]
                sub(/^[[:space:]]*\/\/[[:space:]]*/, "", nx)
                if (nx != "") return 1
            }
            return 0
        }
        FNR == NR { det[FNR] = $0; ndet = FNR; next }
        { txt[FNR] = $0; ntxt = FNR }
        END {
            for (i = 1; i <= ndet; i++) {
                line = det[i]
                if (line !~ /#!?\[allow\(/ && !(line ~ /#!?\[cfg_attr\(/ && line ~ /allow\(/)) {
                    continue
                }
                found = reason_here(i, txt, ntxt)
                # 사유는 빈 줄·코드 줄 전까지 붙어 있는 주석 블록에서 찾는다.
                for (k = i - 1; k >= 1 && !found; k--) {
                    if (txt[k] !~ /^[[:space:]]*\/\//) break
                    if (reason_here(k, txt, ntxt)) found = 1
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

echo "훑은 .rs ${scanned_count}개 (좌변=${LEFT_SOURCE})."
echo "근거 없는 #[allow(...)] : ${count}건 (상한 ${CAP})"
echo

if [ "$count" -gt "$CAP" ]; then
    printf '%s' "$report"
    echo
    echo "근거 없는 #[allow(...)] 가 늘었다: ${count} > 상한 ${CAP}."
    echo "새로 붙인 억제에 근거 주석(reason: / 이유: / complexity-exempt: / SAFETY:)을"
    echo "같은 줄이거나, 그 억제 바로 위로 이어지는 주석 블록 안에 달아라 — 그 블록은"
    echo "빈 줄이나 주석 아닌 줄에서 끊긴다(줄 수 제한은 없다)."
    echo "마커 뒤 같은 줄이나 바로 다음 주석 줄에 근거를 적어라."
    echo "\`// 이유:\`만 적은 빈 사유는 허용하지 않는다."
    echo "SAFETY도 콜론과 설명을 요구한다(\`// SAFETY: <안전 조건>\`)."
    echo "콜론 없는 단순 SAFETY 언급은 사유로 인정하지 않는다."
    echo "상한을 올려서 통과시키지 마라."
    exit 1
fi

if [ "$count" -lt "$CAP" ]; then
    echo "근거 없는 #[allow(...)] 가 ${count} 로 상한 ${CAP} 보다 적게 나왔다."
    echo
    echo "개수 감소만으로 원인을 알 수 없다. 다음을 확인해라."
    echo "  1. 억제를 제거했거나 사유를 추가했는지."
    echo "  2. 검사 대상 파일이 일부 빠졌는지. 위의 '훑은 .rs N개'를 이전 결과와 비교해라."
    echo "     빈 목록만 exit 2로 거부하므로 일부 누락은 이 분기로 올 수 있다."
    echo "  3. 억제나 사유를 찾는 조건이 달라졌는지."
    echo "     수집·판독 오류를 고치기 전에 상한을 조정하지 마라."
    echo
    echo "실제 개선으로 줄었음을 확인한 뒤에만 CAP 을 ${count} 로 내려라."
    echo "실제 개수보다 높은 상한을 남기면 그 차이만큼 새 위반을 허용하게 된다."
    exit 1
fi
