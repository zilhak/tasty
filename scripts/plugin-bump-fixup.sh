#!/usr/bin/env bash
# HEAD 커밋이 바꾼 번들 플러그인의 patch 버전을 올려 HEAD 커밋에 합친다(git commit --amend).
# 판정은 check-plugin-version-bump.sh --range HEAD^ HEAD 를 그대로 쓴다. 이미 올라가 있으면 아무것도 하지 않는다.
# 병합 단계에서 git rebase --exec 'bash scripts/plugin-bump-fixup.sh' <base> 로 커밋마다 실행한다.
# 절차: docs/dev-guide/git-hooks.md#lane-작업-트리의-p1-보류. 종료 코드: 완료·할 일 없음 0, 실패 1, 판정 불가 2.

set -uo pipefail

TAG='[plugin-bump-fixup]'
die() { printf '%s %s\n' "$TAG" "$*" >&2; exit 2; }
fail() { printf '%s %s\n' "$TAG" "$*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHECK="$SCRIPT_DIR/check-plugin-version-bump.sh"
[ -f "$CHECK" ] || die "판정 불가: 검사 스크립트가 없다: $CHECK"

git rev-parse --git-dir >/dev/null 2>&1 || die "판정 불가: git 저장소가 아니다."
ROOT="$(git rev-parse --show-toplevel)" || die "판정 불가: 저장소 루트를 못 찾았다."
cd "$ROOT" || die "판정 불가: 저장소 루트로 이동 실패."

[ "$#" -eq 0 ] || die "판정 불가: 인자를 받지 않는다 (HEAD 커밋만 다룬다)."
git rev-parse --verify --quiet 'HEAD^{commit}' >/dev/null || die "판정 불가: HEAD 커밋이 없다."
if ! git rev-parse --verify --quiet 'HEAD^1^{commit}' >/dev/null; then
    printf '%s 부모가 없는 커밋이다. 비교할 이전 버전이 없어 아무것도 하지 않는다.\n' "$TAG"
    exit 0
fi
git rev-parse --verify --quiet 'HEAD^2' >/dev/null && die "판정 불가: merge 커밋은 다루지 않는다."
# amend 가 커밋 밖의 변경을 섞지 않도록 추적 파일이 깨끗할 때만 진행한다.
if ! git diff --quiet || ! git diff --cached --quiet; then
    die "판정 불가: 커밋하지 않은 추적 파일 변경이 있다. 정리한 뒤 다시 실행해라."
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

bash "$CHECK" --range HEAD^ HEAD --violations-out "$WORK/violations"
rc=$?
case "$rc" in
    0) printf '%s 올릴 플러그인 버전이 없다.\n' "$TAG"; exit 0 ;;
    1) ;;
    *) die "판정 불가: 버전 검사가 종료 코드 $rc 로 끝났다. 위 원인을 먼저 해결해라." ;;
esac
[ -s "$WORK/violations" ] \
    || fail "버전 검사가 위반을 보고했지만 자동으로 올릴 플러그인 목록이 비었다. 위 메시지를 확인해라."

VERSION_LINE_RE='^version[[:space:]]*=[[:space:]]*"'
first_version_line() { awk -v re="$VERSION_LINE_RE" '$0 ~ re { print NR; exit }' "$1"; }
version_in() {
    local n
    n=$(first_version_line "$1")
    [ -n "$n" ] || return 0
    sed -n "${n}s/${VERSION_LINE_RE}\([^\"]*\)\".*/\1/p" "$1"
}
set_version_in() {  # <file> <new>
    local n
    n=$(first_version_line "$1")
    sed -i "${n}s/\(${VERSION_LINE_RE}\)[^\"]*\"/\1$2\"/" "$1" || fail "$1 의 version 을 바꾸지 못했다."
}
package_name() {
    awk '/^\[/ { in_pkg = ($0 == "[package]") } in_pkg && /^name[[:space:]]*=/ {
        sub(/^name[[:space:]]*=[[:space:]]*"/, ""); sub(/".*/, ""); print; exit }' "$1"
}

BUMPED=()
NAMES=()
while IFS= read -r base; do
    [ -n "$base" ] || continue
    cargo_toml="$base/Cargo.toml"
    manifest="$base/tasty-plugin.toml"
    if [ ! -f "$cargo_toml" ] || [ ! -f "$manifest" ]; then
        fail "$base 에 Cargo.toml 이나 tasty-plugin.toml 이 없다."
    fi
    vc=$(version_in "$cargo_toml")
    vm=$(version_in "$manifest")
    [ "$vc" = "$vm" ] || fail "$base: Cargo.toml($vc)과 매니페스트($vm)의 version 이 달라 어느 쪽을 올릴지 정할 수 없다."
    if ! [[ "$vc" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        fail "$base: version '$vc' 가 MAJOR.MINOR.PATCH 숫자 형식이 아니다."
    fi
    new="${vc%.*}.$(( ${vc##*.} + 1 ))"
    set_version_in "$cargo_toml" "$new"
    set_version_in "$manifest" "$new"
    if [ "$(version_in "$cargo_toml")" != "$new" ] || [ "$(version_in "$manifest")" != "$new" ]; then
        fail "$base: version 을 $new 로 쓰지 못했다."
    fi
    name=$(package_name "$cargo_toml")
    [ -n "$name" ] || fail "$cargo_toml 에서 [package] name 을 못 읽었다."
    printf '%s %s %s → %s\n' "$TAG" "$name" "$vc" "$new"
    BUMPED+=("$cargo_toml" "$manifest")
    NAMES+=("$name")
done < "$WORK/violations"

if [ -f Cargo.lock ]; then
    # Cargo 는 path 패키지의 version 변화를 lock 에 반영한다. 오프라인이라 레지스트리 의존은 바꾸지 않는다.
    cargo metadata --offline --format-version 1 >/dev/null 2>"$WORK/metadata.err" \
        || fail "Cargo.lock 을 갱신하지 못했다. cargo 의 stderr:
$(cat "$WORK/metadata.err")"
    # 올린 패키지의 version 줄 말고는 lock 이 바뀌지 않았는지 확인한다.
    changed=$(git diff --numstat -- Cargo.lock | awk '{ print $1 + $2 }')
    in_lock=0
    for name in "${NAMES[@]}"; do
        grep -qxF "name = \"$name\"" Cargo.lock && in_lock=$((in_lock + 1))
    done
    [ "${changed:-0}" -eq $((in_lock * 2)) ] \
        || fail "Cargo.lock 이 올린 패키지 $in_lock 개의 version 줄보다 많이 바뀌었다 (바뀐 줄 ${changed:-0}). git diff Cargo.lock 을 확인해라."
    BUMPED+=(Cargo.lock)
fi

git add -- "${BUMPED[@]}" || fail "git add 가 실패했다."
git commit --amend --no-edit --quiet || fail "git commit --amend 가 실패했다. 위 훅 메시지를 확인해라."

bash "$CHECK" --range HEAD^ HEAD >/dev/null 2>"$WORK/recheck.err"
rc=$?
[ "$rc" -eq 0 ] || fail "버전을 올린 뒤 다시 검사했는데 종료 코드 $rc 다:
$(cat "$WORK/recheck.err")"
printf '%s HEAD 커밋에 버전 증가 %d 건을 합쳤다.\n' "$TAG" "${#NAMES[@]}"
