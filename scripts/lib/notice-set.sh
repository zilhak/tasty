# shellcheck shell=bash
#
# 이 파일은 `source` 되는 라이브러리라 shebang 이 없다 — 실행 파일이 아니다(위 지시자가
# 정적 검사기에게 대상 셸을 알려 준다. 같은 형태의 본보기는 `scripts/lib/judge-bin.sh`).
#
# 고지 세트를 배포 트리에 스테이징하고, 산출물에 그 세트가 다 들어갔는지 본다. 세트가
# 무엇인지는 THIRD_PARTY_LICENSES.md 의 "고지 세트" 절이 정본이고, 생성하지 않고 저장소의
# 파일을 그대로 나르는 근거는 docs/adr/0317-the-notice-set-is-staged-not-generated.md 에
# 있다.
#
# 세트의 셋째 항목은 파일 이름이 아니라 `LICENSES/` 디렉토리다 — 본문이 하나 늘 때 이 파일을
# 고칠 일이 없게 하려는 것이다. 그래서 목록을 여기 적지 않고 `notice_set_files` 가 매번
# 디렉토리에서 읽는다.
#
# 부르는 쪽은 레포 루트를 cwd 로 둔 채 이 파일을 source 한다 — 아래 경로가 전부 루트
# 기준 상대 경로다.

# Print the notice set, one repo-relative path per line: LICENSE, the inventory,
# then every regular file directly under LICENSES/ in name order. An empty
# LICENSES/ is an error rather than an empty set — the inventory lists at least
# one licence text, so an empty directory means the checkout is broken.
notice_set_files() {
    local f found=0
    echo "LICENSE"
    echo "THIRD_PARTY_LICENSES.md"
    for f in LICENSES/*; do
        [[ -f "$f" ]] || continue
        echo "$f"
        found=1
    done
    if [[ "$found" -eq 0 ]]; then
        echo "Error: LICENSES/ holds no licence text" >&2
        return 1
    fi
}

# Stage the notice set into a distribution tree. `LICENSES/` keeps its
# subdirectory so that the relative links inside THIRD_PARTY_LICENSES.md still
# resolve.
stage_notice() {
    local dest="$1" files f
    files=$(notice_set_files) || return 1
    mkdir -p "$dest/LICENSES"
    while IFS= read -r f; do
        cp "$f" "$dest/$f"
    done <<<"$files"
}

# Check a staged tree against the repo copy, byte for byte. Used where the
# packed tree is still on disk (AppImage's AppDir, the macOS .app).
verify_notice_tree() {
    local root="$1" label="$2" files f
    files=$(notice_set_files) || return 1
    while IFS= read -r f; do
        if ! cmp -s "$f" "$root/$f"; then
            echo "Error: $f missing from $label (or differs from the repo copy)" >&2
            return 1
        fi
    done <<<"$files"
}

# Check an archive/package listing (one entry per line, any decoration before
# the path) for every file of the set. A line matches when it **ends** with
# `prefix` + the path — a substring match would let `…/LICENSES/x` stand in for a
# missing `…/LICENSE`. `prefix` is prepended to each path; with
# `flat` set, `LICENSES/<name>` is looked up as `<name>` — the rpm layout keeps
# the licence texts directly under /usr/share/licenses/tasty/.
#
# The listing comes in as a string, not a pipe: see the SIGPIPE note in
# build-linux.sh's verification step.
verify_notice_listing() {
    local listing="$1" prefix="$2" label="$3" flat="${4:-}" files f want
    files=$(notice_set_files) || return 1
    while IFS= read -r f; do
        want="$f"
        if [[ -n "$flat" ]]; then
            want="${f#LICENSES/}"
        fi
        if ! awk -v t="$prefix$want" '
                length($0) >= length(t) && substr($0, length($0) - length(t) + 1) == t { hit = 1 }
                END { exit !hit }' <<<"$listing"; then
            echo "Error: $prefix$want not in $label" >&2
            return 1
        fi
    done <<<"$files"
}
