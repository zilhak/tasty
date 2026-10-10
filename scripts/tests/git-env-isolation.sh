#!/usr/bin/env bash
# 임시 Git 저장소를 만드는 시험 타깃을 바깥 저장소의 GIT_DIR 를 넘긴 채 실행하고,
# 바깥 저장소의 config·index·HEAD 가 그대로인지 비교한다. 바깥 저장소는 이 스크립트가 만든 픽스처다.
# linked worktree 에서 git rebase --exec 가 GIT_DIR 를 넘기는 상황을 재현한다.
# 사용: bash scripts/tests/git-env-isolation.sh   (저장소 루트에서)
# TASTY_E2E_DISPLAY 가 있으면 GUI 서버를 띄우는 attach_git_query_loopback 도 함께 실행한다.
set -eu -o pipefail

# 이 스크립트 자신이 바깥 환경을 상속하지 않게 먼저 지운다.
local_vars="$(git rev-parse --local-env-vars)"
while IFS= read -r name; do
    [ -z "$name" ] || unset "$name"
done <<< "$local_vars"

root="$(git rev-parse --show-toplevel)"
cd "$root"

# 패키지:타깃. 저장소 안의 시험 중 임시 폴더에서 git 이나 git 을 부르는 스크립트·바이너리를 띄우는 것.
targets=(
    "tasty-doc-guards:adr_renumber_bin"
    "tasty:allow_reason_gate"
    "tasty:shared_walk_gate"
    "tasty:plugin_bump_fixup"
    "tasty:plugin_version_bump_channel"
)
if [ -n "${TASTY_E2E_DISPLAY:-}" ]; then
    targets+=("tasty:attach_git_query_loopback")
fi

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
outer="$fixture/outer"
mkdir -p "$outer"
git -C "$outer" init -q
printf 'outer\n' > "$outer/f"
git -C "$outer" add f
git -C "$outer" -c user.name=fixture -c user.email=fixture@example.invalid -c commit.gpgsign=false commit -qm outer
cp "$outer/.git/config" "$fixture/config.before"
cp "$outer/.git/index" "$fixture/index.before"
head_before="$(git -C "$outer" rev-parse HEAD)"

ran=0
failed=0
for spec in "${targets[@]}"; do
    pkg="${spec%%:*}"
    target="${spec#*:}"
    # 빌드는 넘겨줄 GIT_DIR 없이 한다. 실행 파일과 패키지 경로를 cargo 의 JSON 출력에서 읽는다.
    line="$(cargo test --locked -q --no-run --message-format=json -p "$pkg" --test "$target" 2>/dev/null \
        | grep '"reason":"compiler-artifact"' \
        | grep "\"name\":\"$target\"" \
        | grep '"test":true' || true)"
    exe="$(printf '%s\n' "$line" | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -n 1)"
    manifest="$(printf '%s\n' "$line" | sed -n 's/.*"manifest_path":"\([^"]*\)".*/\1/p' | tail -n 1)"
    if [ -z "$exe" ] || [ -z "$manifest" ]; then
        echo "[git-env] $spec 의 시험 실행 파일을 찾지 못했다." >&2
        exit 2
    fi
    # cargo test 처럼 패키지 디렉터리에서 실행한다.
    if (cd "$(dirname "$manifest")" && GIT_DIR="$outer/.git" "$exe" -q > "$fixture/$target.log" 2>&1); then
        echo "[git-env] 통과 $spec"
    else
        failed=$((failed + 1))
        echo "[git-env] 시험 실패 $spec — 마지막 출력:" >&2
        tail -n 20 "$fixture/$target.log" >&2
    fi
    ran=$((ran + 1))
done

changed=0
cmp -s "$fixture/config.before" "$outer/.git/config" || { echo "[git-env] 바깥 config 가 바뀌었다:" >&2; diff "$fixture/config.before" "$outer/.git/config" >&2 || true; changed=1; }
cmp -s "$fixture/index.before" "$outer/.git/index" || { echo "[git-env] 바깥 index 가 바뀌었다." >&2; changed=1; }
[ "$(git -C "$outer" rev-parse HEAD)" = "$head_before" ] || { echo "[git-env] 바깥 HEAD 가 움직였다." >&2; changed=1; }

if [ "$changed" -ne 0 ] || [ "$failed" -ne 0 ]; then
    echo "[git-env] 실패 — 실행 ${ran} 타깃 · 시험 실패 ${failed} · 바깥 저장소 변경 ${changed}" >&2
    exit 1
fi
echo "[git-env] 통과 — 실행 ${ran} 타깃, 바깥 저장소의 config·index·HEAD 가 그대로다."
