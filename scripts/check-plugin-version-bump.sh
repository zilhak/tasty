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
# ── 무엇을 그 plugin 의 산출물로 세는가 ────────────────────────────────
#
# **plugin 디렉토리만 보면 공유 크레이트를 통째로 놓친다.** 번들 plugin 은
# `tasty-plugin-agent-common`·`tasty-plugin-sdk`·`tasty-utils` 같은 워크스페이스 크레이트를
# 링크하고, 그것들이 바뀌면 plugin 바이너리가 달라진다. 그런데 그 크레이트들은 매니페스트가
# 없어 판정 대상에서 자연히 빠졌다 — 버전 그대로인 채 내용만 다른 산출물이 발행되고,
# `upgrade-builtins` 는 same-version skip 이라 **이미 그 버전을 받은 인스턴스는 재시작 없이
# 그 수정을 영영 못 받는다.** 빌드도 테스트도 초록인데 반영만 안 된다.
#
# 실측(가드 도입 이후 main 395 커밋): 공유 크레이트를 건드린 커밋 22 건, 그 22 건이 요구했어야
# 할 plugin bump 147 건 중 실제로 오른 것은 7 건. 영향 plugin 을 전부 올린 커밋은 1 건뿐이었다.
#
# 그래서 판정 대상 파일 집합을 **워크스페이스 내부 의존 폐포**로 넓힌다(`cargo tree`,
# normal+build 만 — dev-의존은 산출물에 안 들어간다). 그 값은 변경된 크레이트 중 매니페스트
# 없는 것이 하나라도 있을 때만 구한다(plugin 하나당 ~0.15 s).
#
# ── 그리고 **출하되는 내용**만 센다 ────────────────────────────────────
#
# 폐포로 넓히면 fan-out 이 생긴다 — `tasty-utils` 한 줄이 9 개 plugin 전부를 건드린다. 그래서
# 무엇을 "달라졌다" 로 셀지가 전보다 훨씬 크게 작동한다. 산출물에 안 들어가는 것을 세면 안 된다:
#
#   · 인라인 `#[cfg(test)]` 범위
#   · `#[cfg(test)] mod x;` 로만 선언된 **파일 전체**
#   · 술어가 `test` 를 **요구하는** `cfg_attr` 의 **속성 줄**(그 자리에 있으라고 요구되는
#     근거 주석 포함). 범위가 다르다 — `cfg_attr` 은 붙는 속성만 조건부라 **항목은 출하된다.**
#
# 셋 다 `strip-cfg-test --blank-test-only-files` 가 지운다 — 파일 SLOC 게이트가 이미 쓰는
# 판정기다(ADR-0165). 여기서 술어를 다시 구현하지 않는다. 실측에서 이 축이 147 요구 중
# 36 을 없앴고, 그 36 은 전부 전체-테스트 파일이었다.
#
# 셋째 형태는 처음에 빠져 있었고, 이 게이트의 실회차 첫 발화가 그 때문에 거짓 양성이었다
# (공유 크레이트 둘의 크레이트 루트 `#![cfg_attr(test, …)]` 한 줄씩이 plugin 6 개에 bump 를
# 요구했다 — 출하 산출물은 비트 단위로 동일). 경위와 재검토 조건은 ADR-0166.
#
# **빈 줄을 접는 이유**: 그 판정기는 줄 번호 보존용으로 지운 자리를 빈 줄로 남긴다. 안 접으면
# `#[cfg(test)] mod x;` 두 줄이 는 것이 "내용이 달라졌다" 로 읽힌다(실측으로 밟았다).
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

# 출하 판정기. **여기서 빌드하지 않는다** — pre-commit 에서 cargo 를 부르면 바깥 cargo 와
# 락을 다툰다(파일 SLOC 게이트가 같은 이유로 같은 형태를 쓴다). 없으면 판정 불가다.
# 판정기를 찾는 것과 **낡았는지 보는 것**은 공용이다 — 이 계열 게이트 셋이 같은 크레이트의
# CLI 판정기를 부르고, 규칙을 게이트마다 따로 쓰면 같은 물음에 답이 셋이 된다.
# 스크립트 자신의 위치에서 읽는다 — `$ROOT` 는 **판정 대상 저장소**라, 합성 픽스처
# 저장소에서 부르면 거기엔 이 파일이 없다.
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"
STRIP_BIN="$(resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT")"
# 없거나 낡았으면 **더 넓게** 본다. 여기서 판정 불가로 죽이지 않는 이유는, 이 스크립트가 갓
# 클론한 트리의 pre-commit 에서도 불리기 때문이다. 넓게 보는 방향은 조용한 통과를 안 만든다 —
# 출하 밖 변경(테스트 전용)이 bump 를 요구하는 오탐이 될 뿐이고, ADR-0137 이 적은 비대칭
# (오탐 하나 vs 라이브 반영이 조용히 깨짐)에서 감수하는 쪽이다. 사유는 헬퍼가 말한다.
if [ -z "$STRIP_BIN" ]; then
    echo "[plugin-version] 출하 범위를 못 좁힌다 — 테스트 전용 변경까지 bump 를 요구한다." >&2
fi

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

# ── 한쪽 끝의 크레이트 트리를 펼친다 ─────────────────────────────────
# 전체-테스트 파일 판정은 **다른 파일의 선언**(`#[cfg(test)] mod x;`)을 봐야 하므로
# 파일 하나만 꺼내면 답이 안 나온다. 그래서 관련 크레이트를 통째로 펼친 뒤 판정기를 한 번
# 돌린다. 펼치는 대상은 변경에 걸린 크레이트뿐이라 워크스페이스 규모와 무관하다.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# 이 plugin 이 링크하는 **워크스페이스 내부** 크레이트 이름들(한 줄에 하나). 값을 캐시한다 —
# 후보 선정과 파일 집합 계산에서 두 번 묻는다.
#
# 이름 **정확 일치**로 본다. 부분 문자열로 보면 `tasty-utils` 가 `tasty-utils-extra` 에도
# 걸려 엉뚱한 plugin 을 판정 대상으로 끌어들인다.
closure_of() {
    local pname="$1" cache="$WORK/closure.$1"
    if [ ! -f "$cache" ]; then
        cargo tree -p "$pname" -e normal,build --prefix none --offline 2>/dev/null \
            | awk '{print $1}' | sort -u > "$cache" \
            || die "판정 불가: cargo tree 가 실패했다 ($pname)."
        [ -s "$cache" ] || die "판정 불가: $pname 의 의존 폐포가 비었다 — 빈 모수는 측정 실패다."
    fi
    cat "$cache"
}

# 이 plugin 의 폐포에 그 크레이트가 있는가.
#
# **파이프로 쓰지 않는다.** `grep -q` 는 첫 일치에서 입력을 닫고, 그러면 왼쪽의
# `closure_of`(마지막이 `cat`)가 SIGPIPE 로 죽는다 — `pipefail` 이 켜져 있으므로
# 파이프라인 rc 가 141 이 되어 **찾았는데 못 찾은 것이 된다.** 이 함수의 반환값은
# 판정 대상 집합을 정하므로, 뒤집히면 게이트가 조용히 반대로 판정한다.
# 가드: tests/no_early_exit_consumer_in_shell_pipes.rs
links() {  # <pname> <crate-name>
    grep -qxF "$2" <<<"$(closure_of "$1")"
}

# 사용: materialize <라벨> <tree-ish> <crate 경로>...
# 라벨 디렉토리에 `crates/<...>` 를 펼치고 출하 밖 줄을 지운 사본을 남긴다.
materialize() {
    local label="$1" tree="$2"; shift 2
    [ "$#" -gt 0 ] || return 0
    [ -n "$STRIP_BIN" ] || return 0
    local raw="$WORK/$label.raw" cooked="$WORK/$label.cooked"
    mkdir -p "$raw" "$cooked" || die "판정 불가: 작업 디렉토리를 못 만들었다."
    # 없는 경로가 섞이면 git archive 가 통째로 실패한다 — 있는 것만 넘긴다.
    local present=() p
    for p in "$@"; do
        if git cat-file -e "$tree:$p" 2>/dev/null; then present+=("$p"); fi
    done
    [ "${#present[@]}" -gt 0 ] || return 0
    git archive --format=tar "$tree" -- "${present[@]}" | tar -x -C "$raw" \
        || die "판정 불가: 트리를 펼치지 못했다 ($label)."
    [ -d "$raw/crates" ] || return 0
    "$STRIP_BIN" --blank-test-only-files "$cooked" "$raw" crates >/dev/null \
        || die "판정 불가: 출하 판정기가 실패했다 ($label)."
}

# 출하 내용의 체크섬. 파일이 없으면 고정 토큰을 낸다 — "없음" 과 "빈 파일" 을 같게 보면
# 파일 추가·삭제를 무변경으로 오판한다.
#
# `.rs` 는 판정기를 거친 사본을 쓰고 rustfmt 로 포맷을 지운다. rustfmt 가 실패하면(옛
# edition 등) **그 앞 단계 내용을 그대로 쓴다** — 정규화 실패를 "같음" 으로 흘리지 않는다.
# 그 방향(오탐)이 조용한 통과보다 낫다. 마지막에 **빈 줄을 접는다**(위 헤더의 이유).
shipped_sum() {
    local label="$1" ref="$2" path="$3" out fmt cooked
    case "$path" in
        *.rs)
            # **출하가 0 이면 없는 것과 같다.** `.rs` 에서는 "파일이 없다" 와 "전부
            # 출하 밖이다" 가 산출물에 미치는 영향이 똑같이 0 이므로 같은 토큰을 낸다.
            # 안 그러면 테스트 전용 파일을 **새로 추가**하는 것이 "없음 → 빈 사본" 으로
            # 읽혀 발화한다(실측으로 밟았다). 진짜 파일을 지우는 쪽은 여전히 갈린다 —
            # 그쪽은 before 가 내용 있는 체크섬이다.
            if ! exists_at "$ref"; then printf '<no-ship>'; return; fi
            cooked="$WORK/$label.cooked/$path"
            if [ -f "$cooked" ]; then out=$(cat "$cooked"); else out=$(git show "$ref"); fi
            fmt=$(printf '%s' "$out" | rustfmt --edition "$RUST_EDITION" --emit stdout 2>/dev/null)
            [ -n "$fmt" ] && out="$fmt"
            out=$(printf '%s' "$out" | sed '/^[[:space:]]*$/d')
            if [ -z "$out" ]; then printf '<no-ship>'; return; fi
            ;;
        *)
            # 소스가 아닌 것(`lang/*.toml`·`assets/*`)은 없음과 빈 파일을 가른다 —
            # 삭제를 무변경으로 오판하지 않기 위해서다.
            if ! exists_at "$ref"; then printf '<absent>'; return; fi
            out=$(git show "$ref")
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

# 변경에 걸린 크레이트(산출물 경로만). 트리 전체를 훑지 않으므로 규모와 무관하다.
CHANGED_CRATES=""
while IFS= read -r f; do
    [ -z "$f" ] && continue
    build_affecting "$f" || continue
    CHANGED_CRATES="$CHANGED_CRATES
$(printf '%s' "$f" | sed -n 's|^\(crates/[^/]*\)/.*$|\1|p')"
done <<EOF
$CHANGED
EOF
CHANGED_CRATES=$(printf '%s\n' "$CHANGED_CRATES" | sed -n '/./p' | sort -u)

# 매니페스트를 가진 것 = 번들 plugin. 나머지가 **공유 크레이트**이고, 그것이 이 게이트가
# 여태 못 보던 자리다.
SHARED_CHANGED=""
for c in $CHANGED_CRATES; do
    [ -f "$ROOT/$c/tasty-plugin.toml" ] || SHARED_CHANGED="$SHARED_CHANGED $c"
done

# plugin 후보 ① 변경된 크레이트 중 매니페스트를 가진 것
PLUGINS=""
for c in $CHANGED_CRATES; do
    [ -f "$ROOT/$c/tasty-plugin.toml" ] && PLUGINS="$PLUGINS $c"
done

# plugin 후보 ② 바뀐 공유 크레이트를 폐포에 포함하는 번들 plugin.
#
# **`cargo tree` 는 작업 트리의 의존 그래프를 읽는다** — `--range` 로 과거 구간을 볼 때도
# 그렇다. CI 는 after 쪽을 체크아웃한 상태로 부르므로 그 자리에서는 after 그래프가 맞고,
# 로컬 `--staged` 도 마찬가지다. 의존 자체가 바뀐 구간을 소급해서 볼 때만 어긋나는데,
# 그 방향은 **더 넓게 보는 쪽**이라 조용한 통과를 만들지 않는다.
if [ -n "${SHARED_CHANGED// /}" ]; then
    for man in "$ROOT"/crates/*/tasty-plugin.toml; do
        [ -f "$man" ] || continue
        pdir=$(dirname "$man"); pname=$(basename "$pdir"); prel="crates/$pname"
        case " $PLUGINS " in *" $prel "*) continue ;; esac
        for c in $SHARED_CHANGED; do
            if links "$pname" "$(basename "$c")"; then PLUGINS="$PLUGINS $prel"; break; fi
        done
    done
fi
PLUGINS=$(printf '%s\n' $PLUGINS | sed -n '/./p' | sort -u)

# 양끝 트리를 한 번씩만 펼친다.
INVOLVED=$(printf '%s\n' $CHANGED_CRATES | sed -n '/./p' | sort -u)
if [ "$MODE" = staged ]; then
    AFTER_TREE=$(git write-tree) || die "판정 불가: 인덱스 트리를 못 만들었다."
else
    AFTER_TREE="$AFTER"
fi
# shellcheck disable=SC2086
materialize before "$BEFORE_REV" $INVOLVED
# shellcheck disable=SC2086
materialize after "$AFTER_TREE" $INVOLVED

for base in $PLUGINS; do
    man="$base/tasty-plugin.toml"
    # bin plugin 인가 — after 쪽에 매니페스트가 있어야 한다. 라이브러리 크레이트
    # (protocol/sdk 등)는 매니페스트가 없어 여기서 자연히 빠진다.
    exists_at "$(after_ref "$man")" || continue
    # before 에 매니페스트가 없으면 새로 추가된 plugin 이다 — 올릴 이전 값이 없다.
    exists_at "$BEFORE_REV:$man" || continue

    # 이 plugin 의 산출물에 닿는 변경 = 자기 디렉토리 + 워크스페이스 내부 의존 폐포.
    scope="$base"
    if [ -n "${SHARED_CHANGED// /}" ]; then
        for c in $SHARED_CHANGED; do
            if links "$(basename "$base")" "$(basename "$c")"; then scope="$scope $c"; fi
        done
    fi
    files=""
    for d in $scope; do
        files="$files
$(printf '%s\n' "$CHANGED" | sed -n "s|^\($d/.*\)$|\1|p")"
    done
    files=$(printf '%s\n' "$files" | sed -n '/./p' | sort -u)
    content_changed=0
    changed_list=""
    for f in $files; do
        build_affecting "$f" || continue
        a=$(shipped_sum before "$BEFORE_REV:$f" "$f")
        b=$(shipped_sum after "$(after_ref "$f")" "$f")
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
