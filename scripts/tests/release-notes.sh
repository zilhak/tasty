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

An escaped \` does not open a span <s> before this ` one.

A span can wrap `--secs
<s>` across lines.
EOF
cat > "$w" <<'EOF'
Run `tasty claude quiet --secs <s>` or ``a `<s>` b`` before Tom &amp; Jerry.

An unmatched ` backtick keeps &lt;s&gt; escaped.

An escaped \` does not open a span &lt;s&gt; before this ` one.

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

```
c & <d>
~~~
a <b>
``
```

Between <s> & more.

~~~
x <y>
~~
```
~~~ after <z>
EOF
cat > "$w" <<'EOF'
```
c & <d>
~~~
a <b>
``
```

Between &lt;s&gt; &amp; more.

~~~
x <y>
~~
```
~~~ after <z>
EOF
check '짧거나 다른 문자의 줄은 펜스를 닫지 않는다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Run this:

    tasty claude quiet --secs <s> && echo done

    second <line>
Back to text <s>.

- A list item with <s>

    continued list text <s>

After the list <s>.

    code again <s>
EOF
cat > "$w" <<'EOF'
Run this:

    tasty claude quiet --secs <s> && echo done

    second <line>
Back to text &lt;s&gt;.

- A list item with &lt;s&gt;

    continued list text &lt;s&gt;

After the list &lt;s&gt;.

    code again <s>
EOF
check '들여쓴 코드 블록은 그대로, 목록 이어 쓰기는 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue.

Keep the crash fix note in the release
notes even next to trailers.
Signed-off-by: Fixture <fixture@example.invalid>
Co-Authored-By: Someone
  <someone@example.invalid>
EOF
cat > "$w" <<'EOF'
Fix the reported issue.

Keep the crash fix note in the release
notes even next to trailers.
EOF
check '섞인 트레일러 블록은 트레일러와 접힌 줄만 뺀다' "$m" "$w"

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

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

- Quote the ` character in names.
- Fix --secs <s> parsing with `foo`.

## The ` key
Fix <s> in `foo`.

Use a ` quote:
- Fix <s> `x`

Title ` a
---
next <s> ` b

Text ` here
> quoted <s> ` b

- Mention a lone `
- Run --secs <s> then a lone `

Escaped \\`<s>` stays code.

| Flag ` | Value <s> ` |
| --- | --- |
| `a | b` <s> |
EOF
cat > "$w" <<'EOF'
- Quote the ` character in names.
- Fix --secs &lt;s&gt; parsing with `foo`.

## The ` key
Fix &lt;s&gt; in `foo`.

Use a ` quote:
- Fix &lt;s&gt; `x`

Title ` a
---
next &lt;s&gt; ` b

Text ` here
> quoted &lt;s&gt; ` b

- Mention a lone `
- Run --secs &lt;s&gt; then a lone `

Escaped \\`<s>` stays code.

| Flag ` | Value &lt;s&gt; ` |
| --- | --- |
| `a | b` &lt;s&gt; |
EOF
check '스팬은 목록 항목·제목·밑줄·인용·표 칸을 넘어 짝짓지 않는다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

- Example
  ~~~sh
  echo hi

Outside --secs <s>.

> ~~~
> a <s>
After the quote <s>.

10. Example
    ```
    a <b> & c
    ```

# Heading
    --path <path> & more

- Run:

      a <s> && b

```sh
x
```
    after <fence>

Text <s>.
EOF
cat > "$w" <<'EOF'
- Example
  ~~~sh
  echo hi

Outside --secs &lt;s&gt;.

> ~~~
> a <s>
After the quote &lt;s&gt;.

10. Example
    ```
    a <b> & c
    ```

# Heading
    --path <path> & more

- Run:

      a <s> && b

```sh
x
```
    after <fence>

Text &lt;s&gt;.
EOF
check '목록·인용 안 코드 블록은 컨테이너와 함께 끝나고, 제목 뒤 들여쓴 코드는 그대로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue.

Signed-off-by: Fixture <fixture@example.invalid>

# Notes
EOF
cat > "$w" <<'EOF'
Fix the reported issue.

# Notes
EOF
check '트레일러 뒤에 주석(#) 줄이 있어도 Git 이 찾은 트레일러 문단을 뺀다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue.

Signed-off-by: Fixture <fixture@example.invalid>
(cherry picked from commit 0123456789abcdef0123456789abcdef01234567)
  Keep this indented note.
EOF
cat > "$w" <<'EOF'
Fix the reported issue.

(cherry picked from commit 0123456789abcdef0123456789abcdef01234567)
  Keep this indented note.
EOF
check 'Git 이 트레일러로 내지 않는 cherry-pick 줄과 그 뒤 들여쓴 산문은 남긴다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Already written &amp; and &lt;path&gt; and &#60; show as typed.
EOF
cat > "$w" <<'EOF'
Already written &amp;amp; and &amp;lt;path&amp;gt; and &amp;#60; show as typed.
EOF
check '이미 쓴 엔터티도 적은 글자 그대로 보이게 & 를 바꾼다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

See [the guide](https://example.invalid/a "`") then --secs <s> `x`.

[ref]: https://example.invalid/b '`'
Run <s> `y`.

| Flag ` |
| --- | --- |
| <s> ` |

Keep [the guide](https://example.invalid/a) and `--secs <s>` as code.
EOF
cat > "$w" <<'EOF'
See [the guide](https://example.invalid/a "`") then --secs &lt;s&gt; `x`.

[ref]: https://example.invalid/b '`'
Run &lt;s&gt; `y`.

| Flag ` |
| --- | --- |
| &lt;s&gt; ` |

Keep [the guide](https://example.invalid/a) and `--secs <s>` as code.
EOF
check '링크 목적지·제목, 참조 정의, 칸 수가 다른 표처럼 스팬 짝이 애매한 블록은 모두 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Docs at www.example.invalid/a`b then <div> `c`.

Visit https://example.invalid/a and run `--secs <s>` as code.
EOF
cat > "$w" <<'EOF'
Docs at www.example.invalid/a`b then &lt;div&gt; `c`.

Visit https://example.invalid/a and run `--secs <s>` as code.
EOF
check '공백 전까지 백틱을 삼키는 자동 링크(www., ://)가 있는 블록은 모두 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

[ref
name]: /url "`"
Run --secs <s> `x`.

[ref]: /url
===
    <s> & more
EOF
cat > "$w" <<'EOF'
[ref
name]: /url "`"
Run --secs &lt;s&gt; `x`.

[ref]: /url
===
    &lt;s&gt; &amp; more
EOF
check '참조 정의(여러 줄 레이블 포함)로 시작하는 문단과 그 뒤 밑줄 모양 줄·들여쓴 줄은 모두 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

| A | B |
| --- | --- |
| lone ` \\| `<s>` |
EOF
cat > "$w" <<'EOF'
| A | B |
| --- | --- |
| lone ` \\| `&lt;s&gt;` |
EOF
check '백슬래시가 있는 표는 칸 경계가 애매하므로 본문 전체를 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

| Flag | Value |
| --- | --- |
| `--secs <s>` | <n> |
EOF
cat > "$w" <<'EOF'
| Flag | Value |
| --- | --- |
| `--secs <s>` | &lt;n&gt; |
EOF
check '백슬래시가 없는 표는 칸 안 코드 스팬을 그대로' "$m" "$w"

printf 'chore: bump version to 9.9.9\r\n\r\n~~~sh\r\necho <ok>\r\n~~~\r\nOutside --secs <s>.\r\n' > "$m"
printf '~~~sh\necho <ok>\n~~~\nOutside --secs &lt;s&gt;.\n' > "$w"
check 'CRLF 줄 끝도 줄 끝으로 보아 펜스가 닫히고, 출력은 LF' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fetch `https://example.invalid/?a=1&b=2` or `www.example.invalid/?c=3&d=4` then <s>.
EOF
cat > "$w" <<'EOF'
Fetch `https://example.invalid/?a=1&b=2` or `www.example.invalid/?c=3&d=4` then &lt;s&gt;.
EOF
check '코드 스팬 안의 URL 은 자동 링크로 보지 않아 스팬이 그대로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Fix the reported issue <s>.

Signed-off-by: Fixture <fixture@example.invalid>
# ------------------------ >8 ------------------------
Co-Authored-By: Someone <someone@example.invalid>
EOF
cat > "$w" <<'EOF'
Fix the reported issue &lt;s&gt;.

# ------------------------ &gt;8 ------------------------
Co-Authored-By: Someone &lt;someone@example.invalid&gt;
EOF
check '가위 줄 앞의 트레일러만 빼고, Git 이 보지 않는 가위 줄 아래는 남긴다' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\nFix the reported issue <s>.\n\nSigned-off-by: Fixture <fixture@example.invalid>\n\nConflicts:\n\tsrc/a <b>.rs\n' > "$m"
printf 'Fix the reported issue &lt;s&gt;.\n\nConflicts:\n\tsrc/a &lt;b&gt;.rs\n' > "$w"
check '끝의 Conflicts: 꼬리를 건너뛴 앞 문단의 트레일러만 뺀다' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Lone ` here
<div> `<s>` ok

Lone ` here
</div> `<s>` ok

Lone ` here
<!-- c --> `<s>` ok

Lone ` here
<script> `<s>` ok

Lone ` here
<? x ?> `<s>` ok

Lone ` here
<![CDATA[ x ]]> `<s>` ok

Lone ` here
<!DOCTYPE html> `<s>` ok

> Lone ` here
<div> `<s>` ok

> Lone ` here
> <div> `<s>` ok

- Lone ` here
<div> `<s>` ok

- Lone ` here
  </div> `<s>` ok

Text `
<div> `
EOF
cat > "$w" <<'EOF'
Lone ` here
&lt;div&gt; `&lt;s&gt;` ok

Lone ` here
&lt;/div&gt; `&lt;s&gt;` ok

Lone ` here
&lt;!-- c --&gt; `&lt;s&gt;` ok

Lone ` here
&lt;script&gt; `&lt;s&gt;` ok

Lone ` here
&lt;? x ?&gt; `&lt;s&gt;` ok

Lone ` here
&lt;![CDATA[ x ]]&gt; `&lt;s&gt;` ok

Lone ` here
&lt;!DOCTYPE html&gt; `&lt;s&gt;` ok

> Lone ` here
&lt;div&gt; `&lt;s&gt;` ok

> Lone ` here
> &lt;div&gt; `&lt;s&gt;` ok

- Lone ` here
&lt;div&gt; `&lt;s&gt;` ok

- Lone ` here
  &lt;/div&gt; `&lt;s&gt;` ok

Text `
&lt;div&gt; `
EOF
check '둘째 줄 이후에 HTML 블록 시작 꼴 줄(div·주석·script·?·CDATA·DOCTYPE)이 있는 문단·인용·목록 항목은 모두 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

Wrap with `` and use `foo` or `--secs <s>`.

`` `>` `<s>`

Keep `--secs <s>` and `a & b` as code.
EOF
cat > "$w" <<'EOF'
Wrap with `` and use `foo` or `--secs &lt;s&gt;`.

`` `&gt;` `&lt;s&gt;`

Keep `--secs <s>` and `a & b` as code.
EOF
check '짝 없는 백틱 열이 있는 블록은 GitHub 가 스팬을 다르게 짝지을 수 있으므로 모두 엔터티로' "$m" "$w"

# 드문 표지가 있는 본문은 블록 구조를 읽지 않고 본문 전체를 엔터티로 바꾼다(코드 안 포함).
printf 'chore: bump version to 9.9.9\n\n[^1]\n\n[^1]: x\n\n    <s>\n' > "$m"
printf '[^1]\n\n[^1]: x\n\n    &lt;s&gt;\n' > "$w"
check '각주 정의가 있는 본문은 본문 전체를 엔터티로' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\nFixed a crash[^1].\n\n- [^1]: Reported in the forum.\n\n    Run tasty --secs <s> to reproduce.\n' > "$m"
printf 'Fixed a crash[^1].\n\n- [^1]: Reported in the forum.\n\n    Run tasty --secs &lt;s&gt; to reproduce.\n' > "$w"
check '목록 표지 뒤의 각주 정의도 표지로 본다' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\n>\t[r]: /u\n> ===\n>     <s>\n\n  \ttab <code> & more\n' > "$m"
printf '&gt;\t[r]: /u\n&gt; ===\n&gt;     &lt;s&gt;\n\n  \ttab &lt;code&gt; &amp; more\n' > "$w"
check '탭이 있는 본문은 본문 전체를 엔터티로' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\n`<s>\r\r`\n' > "$m"
printf '`&lt;s&gt;\r\r`\n' > "$w"
check '줄 끝이 아닌 CR 이 있는 본문은 본문 전체를 엔터티로' "$m" "$w"

ticks="$(printf '%81s' '' | tr ' ' '`')"
printf 'chore: bump version to 9.9.9\n\nx %s<s>%s\n' "$ticks" "$ticks" > "$m"
printf 'x %s&lt;s&gt;%s\n' "$ticks" "$ticks" > "$w"
check '4개 이상 이어진 백틱이 있는 본문은 본문 전체를 엔터티로' "$m" "$w"

t256="$(printf '%256s' '' | tr ' ' '~')"
printf 'chore: bump version to 9.9.9\n\n%s\n%s\n<s>\n' "$t256" "${t256%?}" > "$m"
printf '%s\n%s\n&lt;s&gt;\n' "$t256" "${t256%?}" > "$w"
check '4개 이상 이어진 물결표가 있는 본문은 본문 전체를 엔터티로' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\n|a\\\\|b|\n|-|-|\n2. ```<s>\n' > "$m"
printf '|a\\\\|b|\n|-|-|\n2. ```&lt;s&gt;\n' > "$w"
check '겹 백슬래시로 칸 경계가 애매한 표 뒤의 줄도 엔터티로' "$m" "$w"

cat > "$m" <<'EOF'
chore: bump version to 9.9.9

| lone ` \\| `<s>` |
| --- | --- |
| a | b |
EOF
cat > "$w" <<'EOF'
| lone ` \\| `&lt;s&gt;` |
| --- | --- |
| a | b |
EOF
check '머리 행에만 백슬래시가 있는 표도 본문 전체를 엔터티로' "$m" "$w"

deep="$(printf '1. %.0s' $(seq 100))"
printf 'chore: bump version to 9.9.9\n\n%s```<s>\n' "$deep" > "$m"
printf '%s```&lt;s&gt;\n' "$deep" > "$w"
check '한 줄에서 목록 표지를 10개 이상 여는 본문은 본문 전체를 엔터티로' "$m" "$w"

# Git 은 CR 을 줄 끝으로 보지 않는다. 가위 줄·Conflicts: 줄만 CRLF 로 끝나면 Git 에는 그 줄이 그 꼴이 아니다.
printf 'chore: bump version to 9.9.9\n\nSigned-off-by: A <a@example.invalid>\n\n# ------------------------ >8 ------------------------\r\nCo-Authored-By: B <b@example.invalid>\n' > "$m"
printf 'Signed-off-by: A &lt;a@example.invalid&gt;\n\n# ------------------------ &gt;8 ------------------------\n' > "$w"
check 'CRLF 로 끝난 가위 줄은 Git 처럼 가위 줄로 보지 않고 그 뒤 트레일러를 뺀다' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\nSigned-off-by: A <a@example.invalid>\n\nConflicts:\r\n\tfile\n' > "$m"
printf 'Signed-off-by: A &lt;a@example.invalid&gt;\n' > "$w"
check 'CRLF 로 끝난 Conflicts: 는 Git 처럼 꼬리로 보지 않고 트레일러로 뺀다' "$m" "$w"

printf 'chore: bump version to 9.9.9\n\nFix it <s>.\n\nSigned-off-by: A <a@example.invalid>\n\n#\n  \n' > "$m"
printf 'Fix it &lt;s&gt;.\n\n#\n' > "$w"
check '공백만 있는 줄 앞의 주석 줄도 Git 처럼 건너뛰고 그 앞 트레일러를 뺀다' "$m" "$w"

if [ "$failures" -ne 0 ]; then
    echo "실패 $failures 건" >&2
    exit 1
fi
echo "모두 통과"
