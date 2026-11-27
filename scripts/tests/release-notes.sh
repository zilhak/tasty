#!/usr/bin/env bash
# scripts/release-notes.sh 가 만드는 릴리스 노트를 일회용 저장소의 커밋으로 확인한다.
# 경우마다 메시지를 그대로(--cleanup=verbatim) 커밋하고 스크립트 출력을 기대값과 글자 단위로 비교한다.
# 사용: release-notes.sh <저장소 루트> <비어 있는 임시 폴더>
set -eu -o pipefail
root="$(cd "$1" && pwd -P)"
work="$(cd "$2" && pwd -P)"
local_vars="$(git rev-parse --local-env-vars)"
while IFS= read -r name; do
    [ -z "$name" ] || unset "$name"
done <<< "$local_vars"
notes="$root/scripts/release-notes.sh"

repo="$work/repo"
mkdir -p "$repo"
git -C "$repo" init -q
git -C "$repo" config user.name fixture
git -C "$repo" config user.email fixture@example.invalid
git -C "$repo" config commit.gpgsign false

failures=0
# check <이름> <메시지 파일> <기대 파일>
check() {
    local what="$1" message="$2" want="$3" got="$work/got" rc=0
    git -C "$repo" commit -q --allow-empty --cleanup=verbatim -F "$message"
    (cd "$repo" && bash "$notes" HEAD) > "$got" || rc=$?
    if [ "$rc" -eq 0 ] && cmp -s "$want" "$got"; then
        printf 'ok   %s\n' "$what"
    else
        printf 'FAIL %s (rc=%s)\n' "$what" "$rc" >&2
        diff "$want" "$got" | sed 's/^/    /' >&2 || true
        failures=$((failures + 1))
    fi
}

m="$work/message"
w="$work/want"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

New: tasty claude quiet --surface <N> (--secs <s> | --clear) declares
a quiet period. The rest of the notes keep their plain form.
EOF
cat > "$w" <<'EOF'
New: tasty claude quiet --surface &lt;N&gt; (--secs &lt;s&gt; | --clear) declares
a quiet period. The rest of the notes keep their plain form.
EOF
check '코드 밖 자리표시는 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Run `tasty claude quiet --secs <s>` or ``a `<s>` b`` before Tom & Jerry.

An unmatched ` backtick keeps <s> escaped.

An escaped \` does not open a span <s>.

A span can wrap `--secs
<s>` across lines.
EOF
cat > "$w" <<'EOF'
Run `tasty claude quiet --secs <s>` or ``a `<s>` b`` before Tom &amp; Jerry.

An unmatched ` backtick keeps &lt;s&gt; escaped.

An escaped \` does not open a span &lt;s&gt;.

A span can wrap `--secs
<s>` across lines.
EOF
check '코드 스팬 안은 그대로, 밖의 & 는 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Before <s>.

```sh
tasty claude quiet --secs <s> && echo done
```

~~~
a <b> & c
~~~

After <s> & more.
EOF
cat > "$w" <<'EOF'
Before &lt;s&gt;.

```sh
tasty claude quiet --secs <s> && echo done
```

~~~
a <b> & c
~~~

After &lt;s&gt; &amp; more.
EOF
check '코드 블록 안은 그대로, 블록 뒤는 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue.

Add the new feature.

Co-Authored-By: Someone <someone@example.invalid>
Signed-off-by: Fixture <fixture@example.invalid>
EOF
cat > "$w" <<'EOF'
Fix the reported issue.

Add the new feature.
EOF
check '끝의 트레일러 블록은 뺀다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Co-Authored-By: Someone <someone@example.invalid>
EOF
: > "$w"
check '트레일러만 있는 본문은 빈 노트' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue.

Breaking: the old flag is removed and the new flag
replaces it.
EOF
cat > "$w" <<'EOF'
Fix the reported issue.

Breaking: the old flag is removed and the new flag
replaces it.
EOF
check '트레일러가 없는 본문은 끝 문단까지 남긴다' "$m" "$w"

if [ "$failures" -ne 0 ]; then
    echo "실패 $failures 건" >&2
    exit 1
fi
echo "모두 통과"
