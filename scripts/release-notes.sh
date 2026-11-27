#!/usr/bin/env bash
# 커밋 본문으로 GitHub 릴리스 노트를 만들어 stdout 에 쓴다. release.yml 의 create-release 잡이 태그 커밋에 부른다.
# - git interpret-trailers 가 메시지에서 트레일러를 찾으면 본문의 끝 문단(트레일러 블록)을 뺀다.
# - 코드 펜스(``` ·~~~)와 백틱 코드 스팬 밖의 <, >, & 를 HTML 엔터티로 바꾼다. GitHub 가 <s> 같은
#   자리표시를 HTML 태그로 읽지 않게 한다. 코드 표기 안은 엔터티가 글자로 보이므로 그대로 둔다.
# - 4칸 들여쓴 줄은 코드 블록으로 보지 않는다. 목록 이어 쓰기와 가르려면 Markdown 전체 해석이 필요하고,
#   코드로 잘못 보면 자리표시가 다시 HTML 로 읽힌다.
# 사용: release-notes.sh <커밋>
set -eu -o pipefail
commit="${1:?사용: release-notes.sh <커밋>}"

trailers="$(git log -1 --format=%B "$commit" | git interpret-trailers --parse --no-divider)"
strip=0
[ -z "$trailers" ] || strip=1

git log -1 --format=%b "$commit" | awk -v strip="$strip" '
function blank(s) { return s ~ /^[ \t]*$/ }

# 문단 하나(줄바꿈으로 이은 문자열)에서 코드 스팬 밖의 <, >, & 를 바꾼다.
# 백틱 n 개로 연 스팬은 다음에 오는 정확히 n 개짜리 백틱 열에서 닫힌다. 닫는 열이 없으면 백틱은 글자다.
function escape_text(p,    out, i, n, c, k, j, m, stop) {
    out = ""; i = 1; n = length(p)
    while (i <= n) {
        c = substr(p, i, 1)
        if (c == "\\" && i < n && index("`<>&", substr(p, i + 1, 1))) {
            c = substr(p, i + 1, 1)
            # \` 는 스팬을 열지 않는 글자 백틱이다. \< 같은 이스케이프는 엔터티가 같은 글자를 낸다.
            if (c == "`") out = out "\\`"
            else out = out entity(c)
            i += 2
            continue
        }
        if (c == "`") {
            k = i
            while (k <= n && substr(p, k, 1) == "`") k++
            m = k - i
            stop = 0
            j = k
            while (j <= n) {
                if (substr(p, j, 1) != "`") { j++; continue }
                k = j
                while (k <= n && substr(p, k, 1) == "`") k++
                if (k - j == m) { stop = k; break }
                j = k
            }
            if (stop) { out = out substr(p, i, stop - i); i = stop }
            else { out = out substr(p, i, m); i += m }
            continue
        }
        out = out entity(c)
        i++
    }
    return out
}
function entity(c) {
    if (c == "&") return "&amp;"
    if (c == "<") return "&lt;"
    if (c == ">") return "&gt;"
    return c
}
function flush() {
    if (para != "") print escape_text(para)
    para = ""; has_para = 0
}
{ line[++count] = $0 }
END {
    last = count
    while (last > 0 && blank(line[last])) last--
    if (strip == 1) {
        while (last > 0 && !blank(line[last])) last--
        while (last > 0 && blank(line[last])) last--
    }
    fence = ""; para = ""; has_para = 0
    for (i = 1; i <= last; i++) {
        l = line[i]
        if (fence != "") {
            print l
            if (match(l, /^(   |  | )?(`+|~+)[ \t]*$/)) {
                run = l; sub(/^ +/, "", run); sub(/[ \t]+$/, "", run)
                if (substr(run, 1, 1) == substr(fence, 1, 1) && length(run) >= length(fence)) fence = ""
            }
            continue
        }
        if (match(l, /^(   |  | )?(```+|~~~+)/)) {
            run = substr(l, RSTART, RLENGTH); sub(/^ +/, "", run)
            rest = substr(l, RSTART + RLENGTH)
            if (substr(run, 1, 1) != "`" || index(rest, "`") == 0) {
                flush()
                fence = run
                print l
                continue
            }
        }
        if (blank(l)) { flush(); print l; continue }
        para = has_para ? para "\n" l : l
        has_para = 1
    }
    flush()
}
'
