#!/usr/bin/env bash
# plugin 매니페스트 version 이 **산출물이 달라질 때** 올랐는지 판정한다.
#
# 규칙의 목적은 라이브 반영이다 — `upgrade-builtins` 재sync 는 매니페스트 `version` 이
# 올랐을 때만 동작한다(same-version skip). 그래서 판정 기준도 "파일이 staged 되었는가"
# 가 아니라 **"그 plugin 의 빌드 산출물이 달라지는가"** 다. 근거·대안·재검토 조건은
# docs/adr/ 의 plugin 버전 채널 ADR.
#
# 판별식이 **문턱이 아닌 이유**: "큰 커밋은 전역 sweep 이니 봐준다" 는 식의 파일 수
# 문턱은 임의값이다. 실측(규칙 도입 이후 이탈 105 건)에서 문턱을 5→∞ 로 옮기면 국소
# 이탈이 56→105 로 약 2 배 흔들렸고, 같은 문턱에서도 세는 대상을 crates 파일/전체
# 파일 중 무엇으로 하느냐로 83→71 이 갈렸다. 그래서 여기서는 knob 을 쓰지 않는다.
#
# 대신 **rustfmt 로 정규화한 뒤 내용이 같은가**로 가른다. `git diff -w` 로는 부족하다 —
# rustfmt 재정렬은 줄바꿈을 바꾸는데 `-w` 는 줄 *안* 의 공백만 무시한다(실측으로 밟았다).
# 공백을 통째로 제거하는 방식도 쓰지 않는다 — 문자열 리터럴 안의 공백까지 지워서
# `"a b"` 와 `"ab"` 를 같다고 판정하는 **거짓 음성**의 구멍이 생긴다. rustfmt 는 진짜
# 파서라 리터럴 내용을 보존한다.
#
# 실측(가드 이후 창, 이탈 40 쌍)에서 세 정규화를 **양극성으로** 비교했다:
#
#     정규화              부채로 남음   오탐으로 배제   비고
#     exact                    39             0
#     공백 전부 제거            35             4         배제된 4 는 전부 `style:` 커밋
#     rustfmt                  33             6         배제된 6 도 전부 `style:` 커밋
#
# rustfmt 정규화가 **삼킨 6 쌍은 `style: cargo fmt` 커밋 두 개에서만** 나왔다. 남은
# 33 쌍은 fix 8 · docs 8 · feat 6 · refactor 5 · style 1 로, 그 style 1 이 커밋 type 을
# 판별식으로 쓸 수 없다는 증거다 — 한 `style:` 커밋이 plugin 별로, 나아가 파일별로
# 갈린다. 판별식은 커밋보다도 파일 종류보다도 잘게 자른다.
#
# **알려진 오탐: 주석만 바뀐 변경 8 쌍(41 중 20%)** — rustfmt 는 주석을 지우지 않으므로
# `docs:` 류가 여기 걸린다. 주석까지 제거하면 배제가 14 로 늘지만, 줄 단위 정규식
# 주석 제거는 raw string 안의 `//` 를 잘못 지워 **거짓 음성**을 만든다. 오탐(불필요한
# bump 하나)과 거짓 음성(라이브 반영이 조용히 깨짐)의 대가가 비대칭이므로 여기서는
# 오탐 쪽을 감수한다. 재검토 조건은 ADR-0137.
#
# 사용:
#   check-plugin-version-bump.sh --staged [--base <rev>]   # index 를 base 와 비교 (기본 base=HEAD)
#   check-plugin-version-bump.sh --range <before> <after>   # 두 커밋의 끝점 비교
#
# 끝점 비교인 이유: 목적이 "반영될 때 version 이 올라 있는가" 라, 한 lane 의 커밋마다
# 올릴 필요가 없다. 한 번 오르면 재sync 는 동작한다.
#
# exit: 0 통과 · 1 위반 · 2 판정 불가(인자·git 환경). **판정 불가를 통과로 내지 않는다.**

set -uo pipefail

die() { printf '%s\n' "$*" >&2; exit 2; }

git rev-parse --git-dir >/dev/null 2>&1 || die "판정 불가: git 저장소가 아니다 (배포 tarball 등)."
ROOT="$(git rev-parse --show-toplevel)" || die "판정 불가: 저장소 루트를 못 찾았다."
cd "$ROOT" || die "판정 불가: 저장소 루트로 이동 실패."

# 정규화기가 없으면 **판정 불가**다. 없는 채로 통과시키면 0 을 통과로 세는 형태가 된다.
command -v rustfmt >/dev/null 2>&1 \
    || die "판정 불가: rustfmt 가 없다 — 포맷 변경과 실변경을 가를 수 없다. rustup component add rustfmt"

# edition 을 루트 Cargo.toml 에서 읽는다. 여기 박아두면 워크스페이스가 올릴 때 만료된다.
RUST_EDITION=$(sed -n 's/^edition[[:space:]]*=[[:space:]]*"\([0-9]*\)".*/\1/p' Cargo.toml | sed -n '1p')
[ -n "$RUST_EDITION" ] || die "판정 불가: 루트 Cargo.toml 에서 edition 을 못 읽었다."

MODE=""; BASE="HEAD"; BEFORE=""; AFTER=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --staged) MODE=staged; shift ;;
        --base)   BASE="${2:-}"; [ -n "$BASE" ] || die "판정 불가: --base 에 rev 가 없다."; shift 2 ;;
        --range)  MODE=range; BEFORE="${2:-}"; AFTER="${3:-}"
                  [ -n "$BEFORE" ] && [ -n "$AFTER" ] || die "판정 불가: --range 는 두 rev 를 요구한다."
                  shift 3 ;;
        *) die "판정 불가: 알 수 없는 인자 '$1'" ;;
    esac
done
[ -n "$MODE" ] || die "판정 불가: --staged 또는 --range 중 하나를 줘야 한다."

# ── 비교 양끝 확정 ────────────────────────────────────────────
# staged 모드에서 after 쪽은 **인덱스**다. git 은 인덱스 블롭을 `:<경로>` 로 준다.
if [ "$MODE" = staged ]; then
    if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
        # 첫 커밋 — 비교 대상이 없다. 의무도 없다(새 plugin 의 최초 버전).
        echo "[plugin-version] skip: 비교할 커밋이 없다(첫 커밋). 판정 대상 0."
        exit 0
    fi
    BEFORE_REV="$BASE"
    AFTER_PREFIX=""          # `:경로`
else
    for r in "$BEFORE" "$AFTER"; do
        git rev-parse --verify --quiet "$r^{commit}" >/dev/null \
            || die "판정 불가: rev 를 못 찾았다: $r (shallow clone 이면 fetch-depth 를 늘려라)"
    done
    BEFORE_REV="$BEFORE"
    AFTER_PREFIX="$AFTER"
fi

# 경로의 after 쪽 좌표. staged 는 `:경로`, range 는 `<after>:경로`.
after_ref() { printf '%s:%s' "$AFTER_PREFIX" "$1"; }

exists_at() { git cat-file -e "$1" 2>/dev/null; }

# 정규화한 내용의 체크섬. 파일이 없으면 고정 토큰을 낸다 — "없음" 과 "빈 파일" 을
# 같게 보면 파일 추가·삭제를 무변경으로 오판한다.
#
# `.rs` 는 rustfmt 를 통과시킨다. rustfmt 가 실패하면(옛 edition 등) **원문을 그대로
# 쓴다** — 정규화 실패를 "같음" 으로 흘리지 않는다. 그 경우 포맷 차이가 남아 게이트가
# 발화하는데, 그 방향(오탐)이 조용한 통과보다 낫다.
normalized_sum() {
    local ref="$1" path="$2" out fmt
    if ! exists_at "$ref"; then printf '<absent>'; return; fi
    out=$(git show "$ref")
    case "$path" in
        *.rs)
            fmt=$(printf '%s' "$out" | rustfmt --edition "$RUST_EDITION" --emit stdout 2>/dev/null)
            [ -n "$fmt" ] && out="$fmt"
            ;;
    esac
    printf '%s' "$out" | cksum
}

# [package] 절의 version. 매니페스트(tasty-plugin.toml)는 최상위 version 을 쓴다.
version_at() {
    local ref="$1" text
    if ! exists_at "$ref"; then printf ''; return; fi
    text=$(git show "$ref")
    printf '%s' "$text" \
        | sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
        | sed -n '1p'
}

# semver 를 정수 셋으로. 비교 가능한 형태가 아니면 빈 값.
ver_key() {
    printf '%s' "$1" | sed -n 's/^\([0-9][0-9]*\)\.\([0-9][0-9]*\)\.\([0-9][0-9]*\).*$/\1 \2 \3/p'
}

# a 가 b 보다 큰가.
ver_gt() {
    local ka kb; ka=$(ver_key "$1"); kb=$(ver_key "$2")
    [ -n "$ka" ] && [ -n "$kb" ] || return 1
    # shellcheck disable=SC2086
    set -- $ka $kb
    [ "$1" -gt "$4" ] && return 0
    [ "$1" -lt "$4" ] && return 1
    [ "$2" -gt "$5" ] && return 0
    [ "$2" -lt "$5" ] && return 1
    [ "$3" -gt "$6" ]
}

# ── 변경된 파일 목록 ─────────────────────────────────────────
if [ "$MODE" = staged ]; then
    CHANGED=$(git diff --cached --name-only --diff-filter=ACMRD -- 'crates/')
else
    CHANGED=$(git diff --name-only --diff-filter=ACMRD "$BEFORE" "$AFTER" -- 'crates/')
fi

# 산출물에 닿는 경로인가. 문서(.md)·`.sig`·러너 스크립트 등은 여기 없다.
build_affecting() {
    case "$1" in
        */src/*|*/lang/*|*/assets/*|*/Cargo.toml|*/tasty-plugin.toml|*/build.rs) return 0 ;;
        *) return 1 ;;
    esac
}

VIOLATIONS=0
CONSIDERED=0
# plugin 후보는 변경 목록에서 뽑는다 — 트리 전체를 훑지 않으므로 규모와 무관하다.
PLUGINS=$(printf '%s\n' "$CHANGED" \
    | sed -n 's|^\(crates/tasty-plugin-[^/]*\)/.*$|\1|p' \
    | sort -u)

for base in $PLUGINS; do
    man="$base/tasty-plugin.toml"
    # bin plugin 인가 — after 쪽에 매니페스트가 있어야 한다. 라이브러리 크레이트
    # (protocol/sdk 등)는 매니페스트가 없어 여기서 자연히 빠진다.
    exists_at "$(after_ref "$man")" || continue
    # before 에 매니페스트가 없으면 새로 추가된 plugin 이다 — 올릴 이전 값이 없다.
    exists_at "$BEFORE_REV:$man" || continue

    files=$(printf '%s\n' "$CHANGED" | sed -n "s|^\($base/.*\)$|\1|p")
    content_changed=0
    changed_list=""
    for f in $files; do
        build_affecting "$f" || continue
        a=$(normalized_sum "$BEFORE_REV:$f" "$f")
        b=$(normalized_sum "$(after_ref "$f")" "$f")
        if [ "$a" != "$b" ]; then
            content_changed=1
            changed_list="$changed_list
      $f"
        fi
    done
    [ "$content_changed" = 1 ] || continue

    CONSIDERED=$((CONSIDERED + 1))
    vb=$(version_at "$BEFORE_REV:$man")
    va=$(version_at "$(after_ref "$man")")
    if [ -z "$vb" ] || [ -z "$va" ]; then
        printf '✘ [plugin-version] %s: 매니페스트에서 version 을 못 읽었다 (before=%s after=%s)\n' \
            "$base" "${vb:-<없음>}" "${va:-<없음>}" >&2
        VIOLATIONS=$((VIOLATIONS + 1))
        continue
    fi
    if ! ver_gt "$va" "$vb"; then
        printf '✘ [plugin-version] %s: 산출물이 달라지는데 version 이 안 올랐다 (%s → %s)\n' \
            "$base" "$vb" "$va" >&2
        printf '    내용이 바뀐 파일:%s\n' "$changed_list" >&2
        printf '    고쳐라: %s 와 %s 의 version 을 **같은 값**으로 patch +1.\n' \
            "$base/Cargo.toml" "$man" >&2
        VIOLATIONS=$((VIOLATIONS + 1))
    fi
done

if [ "$VIOLATIONS" -gt 0 ]; then
    printf '[plugin-version] 위반 %d 건 / 판정 대상 %d 건\n' "$VIOLATIONS" "$CONSIDERED" >&2
    exit 1
fi
printf '[plugin-version] 통과 — 판정 대상 %d 건 (변경된 crates 파일 %d 개 중)\n' \
    "$CONSIDERED" "$(printf '%s\n' "$CHANGED" | sed -n '/./p' | wc -l)"
exit 0
