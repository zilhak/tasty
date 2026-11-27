#!/usr/bin/env bash
# 커밋 본문으로 GitHub 릴리스 노트를 만들어 stdout 에 쓴다. release.yml 의 create-release 잡이 태그 커밋에 부른다.
# - git interpret-trailers 가 메시지에서 트레일러를 찾으면 Git 과 같은 규칙으로 트레일러 블록을 찾아 트레일러 줄과
#   그 접힌 이어 줄을 뺀다. 블록은 끝의 주석(#) 줄·빈 줄을 건너뛴 마지막 문단이다. 섞인 문장 줄은 남긴다.
# - 코드 펜스(``` ·~~~), 들여쓴 코드 블록, 백틱 코드 스팬 밖의 <, >, & 를 HTML 엔터티로 바꾼다.
#   GitHub 가 <s> 같은 자리표시를 HTML 태그로 읽지 않게 한다. 코드 표기 안은 엔터티가 글자로 보이므로 그대로 둔다.
# - 블록 구조는 CommonMark 를 따른다. 인용(>)과 목록 항목 안의 코드 블록은 그 항목의 내용 들여쓰기를 기준으로 보고,
#   목록·인용이 끝나면 그 안의 코드 블록도 끝난다. 탭은 4칸 탭 정지로 센다. 코드 스팬은 블록 하나 안에서만 짝짓는다.
# 사용: release-notes.sh <커밋>
set -eu -o pipefail
commit="${1:?사용: release-notes.sh <커밋>}"

trailers="$(git log -1 --format=%B "$commit" | git interpret-trailers --parse --no-divider)"
strip=0
[ -z "$trailers" ] || strip=1

git log -1 --format=%b "$commit" | awk -v strip="$strip" '
function blank(s) { return s ~ /^[ \t]*$/ }

# 블록 하나(줄바꿈으로 이은 문자열)를 바꾼다. 스팬 짝이 GitHub 와 어긋날 수 있으면 블록 전체를 엔터티로 바꾼다.
function render(s, cons,    o) {
    o = escape_text(s, cons)
    if (!cons && index(o, "`") && risky(o, escape_text(s, 2))) o = escape_text(s, 1)
    return o
}
# 스팬 짝을 GitHub 와 다르게 볼 수 있는 블록인가: 링크 목적지·제목 안의 백틱, 링크 참조 정의 줄,
# 참조 링크 [글][이름], 판마다 HTML 블록 여부가 다른 태그로 시작하는 줄, 백틱까지 이어지는 GFM 자동 링크(www., ://).
# 자동 링크는 스팬을 가린 m 에서 찾는다. GitHub 는 이미 열린 코드 스팬 안에서 자동 링크를 만들지 않는다.
function risky(o, m,    n, k, ln, t, j, e, z) {
    if (autolink_tick(m, "www.") || autolink_tick(m, "://")) return 1
    n = split(o, ln, "\n")
    for (k = 1; k <= n; k++) {
        t = tolower(ln[k]); sub(/^[ \t]+/, "", t)
        if (k > 1 && t ~ /^<\/?search([^a-z0-9]|$)/) return 1
        if (substr(t, 1, 1) == "[" && index(t, "]:")) return 1
    }
    if (HAS_DEF && index(o, "][")) return 1
    j = 0
    while ((e = index(substr(o, j + 1), "](")) > 0) {
        j += e + 1
        z = link_end(o, j)
        if (z && index(substr(o, j, z - j + 1), "`")) return 1
    }
    return 0
}
# o 에서 key 뒤로 공백 전까지 백틱이 있는가. GitHub 의 자동 링크는 공백 전까지의 글자를 링크로 삼는다.
function autolink_tick(o, key,    j, e, t) {
    j = 0
    while ((e = index(substr(o, j + 1), key)) > 0) {
        j += e
        t = substr(o, j)
        match(t, /^[^ \t\n]*/)
        if (index(substr(t, 1, RLENGTH), "`")) return 1
    }
    return 0
}
# o 의 j 자리 ( 에서 인라인 링크의 목적지와 제목을 읽어 닫는 ) 의 자리를 낸다. 링크가 아니면 0.
function link_end(o, j,    n, k, c, depth, q) {
    n = length(o); k = j + 1
    while (k <= n && substr(o, k, 1) ~ /[ \t\n]/) k++
    if (substr(o, k, 1) == "<") {
        for (k++; k <= n; k++) {
            c = substr(o, k, 1)
            if (c == "\\") { k++; continue }
            if (c == ">") break
            if (c == "<" || c == "\n") return 0
        }
        if (k > n) return 0
        k++
    } else {
        depth = 0
        for (; k <= n; k++) {
            c = substr(o, k, 1)
            if (c == "\\") { k++; continue }
            if (c == "(") depth++
            else if (c == ")") { if (depth == 0) break; depth-- }
            else if (c ~ /[ \t\n]/) break
        }
        if (depth) return 0
    }
    while (k <= n && substr(o, k, 1) ~ /[ \t\n]/) k++
    c = substr(o, k, 1)
    if (c == "\"" || c == "\047" || c == "(") {
        q = (c == "(") ? ")" : c
        for (k++; k <= n && substr(o, k, 1) != q; k++) if (substr(o, k, 1) == "\\") k++
        if (k > n) return 0
        k++
        while (k <= n && substr(o, k, 1) ~ /[ \t\n]/) k++
    }
    return substr(o, k, 1) == ")" ? k : 0
}
# 코드 스팬 밖의 <, >, & 를 바꾼다. all 이 1 이면 스팬 안도 바꾸고, 2 이면 스팬을 여는 백틱만 두고 안을 공백으로 가린다.
# 백틱 n 개로 연 스팬은 다음에 오는 정확히 n 개짜리 백틱 열에서 닫힌다. 닫는 열이 없으면 백틱은 글자다.
function escape_text(p, all,    out, i, n, c, d, k, j, m, stop, seg) {
    out = ""; i = 1; n = length(p)
    while (i <= n) {
        c = substr(p, i, 1)
        if (c == "\\" && i < n && index(PUNCT, substr(p, i + 1, 1))) {
            d = substr(p, i + 1, 1)
            # \<, \>, \& 는 엔터티가 같은 글자를 낸다. 다른 이스케이프(\`, \\ 등)는 그대로 두고 두 글자를 함께 넘긴다.
            if (index("<>&", d)) out = out entity(d)
            else out = out c d
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
            if (stop) {
                seg = substr(p, i, stop - i)
                if (all == 2) { gsub(/./, " ", seg); seg = "`" substr(seg, 2) }
                else if (all) seg = escape_all(seg)
                out = out seg; i = stop
            }
            else { out = out substr(p, i, m); i += m }
            continue
        }
        out = out entity(c)
        i++
    }
    return out
}
function escape_all(t,    out, i) {
    out = ""
    for (i = 1; i <= length(t); i++) out = out entity(substr(t, i, 1))
    return out
}
# 표의 칸 수. \| 가 아닌 | 로 나누고 앞뒤의 | 는 세지 않는다. | 가 없으면 0.
function cells(s,    t, n, i, c, bars) {
    t = s; sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t)
    n = 1; bars = 0
    for (i = 1; i <= length(t); i++) {
        c = substr(t, i, 1)
        if (c == "\\") { i++; continue }
        if (c == "|") { n++; bars++; if (i == 1 || i == length(t)) n-- }
    }
    return bars ? n : 0
}
# 표의 행은 칸마다 따로 바꾼다. GitHub 는 \| 가 아닌 | 에서 칸을 나누고 코드 스팬은 칸을 넘지 못한다.
function escape_row(s,    out, cell, i, n, c) {
    out = ""; cell = ""; n = length(s)
    for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        if (c == "\\" && i < n) { cell = cell c substr(s, i + 1, 1); i++; continue }
        if (c == "|") { out = out render(cell, 0) "|"; cell = ""; continue }
        cell = cell c
    }
    return out render(cell, 0)
}
function row(s) { return tall ? escape_text(s, 1) : escape_row(s) }
# HTML 블록(CommonMark 1~6종)의 시작 꼴인가. 태그 이름은 cmark-gfm 의 목록이다.
function html_start(r,    t, c) {
    if (r ~ /^<[!?]/) return 1
    if (!match(r, /^<\/?[A-Za-z][A-Za-z0-9]*/)) return 0
    t = tolower(substr(r, 1, RLENGTH)); sub(/^<\/?/, "", t)
    c = substr(r, RLENGTH + 1, 1)
    return index(HTML_TAGS, " " t " ") && c !~ /[A-Za-z0-9]/
}
function entity(c) {
    if (c == "&") return "&amp;"
    if (c == "<") return "&lt;"
    if (c == ">") return "&gt;"
    return c
}

# 줄의 탭을 4칸 탭 정지로 펼쳐 X 에 둔다. cmap[열] 은 그 열을 차지한 원래 글자의 위치다.
function expand(l,    i, n, c, col, w) {
    X = ""; col = 0; n = length(l)
    for (i = 1; i <= n; i++) {
        c = substr(l, i, 1)
        if (c == "\t") {
            for (w = 4 - col % 4; w > 0; w--) { X = X " "; cmap[++col] = i }
        } else { X = X c; cmap[++col] = i }
    }
    cmap[col + 1] = n + 1
}
# X 의 p 열부터 이어지는 공백 수.
function indent(p,    q) { q = p; while (substr(X, q, 1) == " ") q++; return q - p }
# 원래 줄에서 X 의 p 열 앞(컨테이너 표지)과 p 열부터(내용)를 나눈다.
function head(l, p) { return substr(l, 1, cmap[p] - 1) }
function body(l, p) { return substr(l, cmap[p]) }

# 모은 문단을 내보낸다. 코드 스팬은 이 문단 안에서만 짝짓는다.
function flush(    s, k, n, part) {
    if (np == 0) return
    s = pbody[1]
    for (k = 2; k <= np; k++) s = s "\n" pbody[k]
    n = split(render(s, tcons || defp() || htmlp()), part, "\n")
    for (k = 1; k <= np; k++) print phead[k] part[k]
    np = 0
}
# 열린 문단이 링크 참조 정의로 시작하는가([ 로 시작하고 ]: 가 있다). 레이블·제목이 여러 줄에 걸칠 수 있고,
# GitHub 는 정의를 떼어 낸 뒤 남은 줄로 문단·밑줄을 다시 본다. 그런 문단은 애매로 보고 블록 전체를 엔터티로 바꾼다.
function defp(    k) {
    if (np == 0 || pbody[1] !~ /^ *\[/) return 0
    for (k = 1; k <= np; k++) if (index(pbody[k], "]:")) return 1
    return 0
}
# 열린 문단의 둘째 줄 이후에 HTML 블록 시작 꼴 줄이 있는가. 그 줄의 < 는 엔터티로 바뀌어 출력에서는 HTML 블록이
# 아니라 앞 줄의 이어 줄이 되고, GitHub 는 앞 줄과 그 줄의 백틱을 짝지을 수 있다. 그런 문단은 블록 전체를 엔터티로 바꾼다.
function htmlp(    k, r) {
    for (k = 2; k <= np; k++) { r = pbody[k]; sub(/^[ \t]+/, "", r); if (html_start(r)) return 1 }
    return 0
}
function end_leaf() { flush(); leaf = "" }
function close_to(d) { if (d < nc) { end_leaf(); nc = d } }
function para_line(l, p) { np++; phead[np] = head(l, p); pbody[np] = body(l, p); leaf = "para" }

# GitHub 처럼 CRLF 를 줄 끝으로 본다. 줄 끝의 CR 을 떼고 LF 로 내보낸다.
{ sub(/\r$/, ""); line[++count] = $0 }
END {
    HTML_TAGS = " address article aside base basefont blockquote body caption center col colgroup dd details dialog dir div dl dt fieldset figcaption figure footer form frame frameset h1 h2 h3 h4 h5 h6 head header hr html iframe legend li link main menu menuitem nav noframes ol optgroup option p param pre script section source style summary table tbody td textarea tfoot th thead title tr track ul "
    for (k = 1; k <= count; k++) if (index(line[k], "]:")) HAS_DEF = 1
    PUNCT = "!\"#$%&\047()*+,-./:;<=>?@[\\]^_`{|}~"
    last = count
    while (last > 0 && blank(line[last])) last--
    if (strip == 1) {
        # Git 의 트레일러 블록: 가위 줄 앞에서, 끝의 주석(#) 줄·빈 줄·옛 Conflicts: 블록을 건너뛴 마지막 문단.
        cut = count
        for (k = 1; k <= count; k++) if (line[k] == "# ------------------------ >8 ------------------------") { cut = k - 1; break }
        boc = 0; conflicts = 0
        for (k = 1; k <= cut; k++) {
            if (substr(line[k], 1, 1) == "#" || line[k] == "") { if (!boc) boc = k }
            else if (line[k] == "Conflicts:") { conflicts = 1; if (!boc) boc = k }
            else if (conflicts && substr(line[k], 1, 1) == "\t") { }
            else if (boc) { boc = 0; conflicts = 0 }
        }
        stop = boc ? boc - 1 : cut
        while (stop > 0 && blank(line[stop])) stop--
        start = stop
        while (start > 0 && !blank(line[start])) start--
        # git interpret-trailers --parse 가 내는 줄(구분자 Key: 가 있는 트레일러)과 그 바로 뒤의 들여쓴 이어 줄만 뺀다.
        # (cherry picked from commit ...) 줄은 Git 이 블록 판정에만 쓰고 트레일러로 내지 않으므로 남긴다.
        keep = 0; folded = 0
        for (k = start + 1; k <= stop; k++) {
            l = line[k]
            if (folded && l ~ /^[ \t]/) continue
            folded = 0
            if (l ~ /^[A-Za-z0-9-]+[ \t]*:/) { folded = 1; continue }
            kept[++keep] = l
        }
        n = 0
        for (k = 1; k <= start; k++) out[++n] = line[k]
        for (k = 1; k <= keep; k++) out[++n] = kept[k]
        tail = stop + 1
        if (keep == 0) while (tail <= count && blank(line[tail])) tail++
        for (k = tail; k <= count; k++) out[++n] = line[k]
        for (k = 1; k <= n; k++) line[k] = out[k]
        last = n
        while (last > 0 && blank(line[last])) last--
    }

    # 열린 컨테이너: ctype[d] 는 q(인용) 또는 l(목록 항목), cwidth[d] 는 목록 항목의 내용 들여쓰기.
    # 열린 잎 블록 leaf 는 para·code(들여쓴 코드)·fence 중 하나이고 늘 가장 안쪽 컨테이너에 속한다.
    nc = 0; leaf = ""; np = 0
    for (i = 1; i <= last; i++) {
        l = line[i]
        expand(l)
        p = 1; matched = 0
        for (d = 1; d <= nc; d++) {
            s = indent(p)
            if (ctype[d] == "q") {
                if (s > 3 || substr(X, p + s, 1) != ">") break
                p += s + 1
                if (substr(X, p, 1) == " ") p++
            } else if (substr(X, p) ~ /^ *$/) {
                # 내용 없이 시작한 항목은 빈 줄에서 끝난다.
                if (cempty[d]) break
            } else {
                if (s < cwidth[d]) break
                p += cwidth[d]
            }
            matched++
        }
        if (substr(X, p) !~ /^ *$/) for (d = 1; d <= matched; d++) cempty[d] = 0
        all = (matched == nc)
        if (leaf == "fence") {
            if (all) {
                print l
                s = indent(p); r = substr(X, p + s)
                if (s <= 3 && r ~ /^(`+|~+) *$/) {
                    sub(/ +$/, "", r)
                    if (substr(r, 1, 1) == fchar && length(r) >= flen) leaf = ""
                }
                continue
            }
            leaf = ""
        }
        if (leaf == "code") {
            if (all && (substr(X, p) ~ /^ *$/ || indent(p) >= 4)) { print l; continue }
            leaf = ""
        }
        # 새 블록의 시작을 찾는다. 문단이 열린 채 컨테이너가 다 이어지면(para_open) 문단을 끊을 수 있는 것만 본다.
        para_open = (all && leaf == "para")
        started = 0; kind = ""
        while (1) {
            s = indent(p); r = substr(X, p + s)
            if (s >= 4) break
            if (substr(r, 1, 1) == ">") {
                end_leaf(); close_to(matched)
                ctype[++nc] = "q"; cempty[nc] = 0; matched = nc; started = 1; para_open = 0
                p += s + 1
                if (substr(X, p, 1) == " ") p++
                continue
            }
            if (r ~ /^(```+|~~~+)/) {
                match(r, /^(`+|~+)/)
                if (substr(r, 1, 1) != "`" || index(substr(r, RLENGTH + 1), "`") == 0) {
                    kind = "fence"; fchar = substr(r, 1, 1); flen = RLENGTH
                    break
                }
            }
            if (r ~ /^#+( |$)/) {
                match(r, /^#+/)
                if (RLENGTH <= 6) { kind = "atx"; break }
            }
            # 참조 정의 문단 뒤의 밑줄 모양 줄은 제목 밑줄로 보지 않고 문단에 넣는다(뒤의 들여쓴 줄도 문단으로).
            if (para_open && r ~ /^(=+|-+) *$/) { if (defp()) break; kind = "setext"; break }
            # 표의 구분 행. 문단의 끝 줄이 머리 행이 된다. GitHub 가 표로 볼지 확실하지 않으면(칸 수가 다르거나,
            # 머리 행에 | 가 없거나, 구분 행이 목록 표지로 시작하면) 다음 빈 줄까지 블록 전체를 엔터티로 바꾼다.
            if (para_open && r ~ /[|:]/ && r ~ /^\|?( *:?-+:? *\|)* *:?-+:? *\|? *$/) {
                if (r !~ /^[-*+] / && cells(pbody[np]) && cells(pbody[np]) == cells(r)) { kind = "table"; break }
                tcons = 1
                if (r !~ /^[-*+] /) break
            }
            t = r; gsub(/ /, "", t)
            if (t ~ /^(---+|\*\*\*+|___+)$/) { kind = "hr"; break }
            if (match(r, /^([-*+]|[0-9]+[.)])( |$)/)) {
                mlen = RLENGTH - (substr(r, RLENGTH, 1) == " " ? 1 : 0)
                marker = substr(r, 1, mlen)
                if (mlen <= 10) {
                    after = substr(r, mlen + 1)
                    empty = (after ~ /^ *$/)
                    ordered = (marker ~ /^[0-9]/)
                    if (para_open && (empty || (ordered && marker !~ /^0*1[.)]$/))) break
                    gap = indent(p + s + mlen)
                    w = s + mlen + ((empty || gap >= 5) ? 1 : gap)
                    end_leaf(); close_to(matched)
                    ctype[++nc] = "l"; cwidth[nc] = w; cempty[nc] = empty; matched = nc; started = 1; para_open = 0
                    p += w
                    continue
                }
            }
            break
        }
        s = indent(p); rest_blank = (substr(X, p) ~ /^ *$/)
        # 컨테이너가 다 이어지지 않아도 문단 이어 쓰기(게으른 줄)면 컨테이너와 문단을 그대로 둔다.
        if (!all && !started && kind == "" && leaf == "para" && !rest_blank) {
            para_line(l, p)
            continue
        }
        close_to(matched)
        if (kind == "fence") { end_leaf(); leaf = "fence"; print l; continue }
        if (kind == "atx") { end_leaf(); print head(l, p) render(body(l, p), 0); continue }
        if (kind == "setext" || kind == "hr") { end_leaf(); print l; continue }
        if (kind == "table") {
            hh = phead[np]; hb = pbody[np]; np--
            end_leaf()
            # GitHub 는 칸을 나누기 전에 \| 를 따로 읽어 \\| 같은 겹 백슬래시에서 칸 경계를 다르게 본다.
            # 머리 행부터 다음 빈 줄까지 백슬래시가 있으면 표 전체를 애매로 보고 칸 안 스팬까지 엔터티로 바꾼다.
            tall = index(hb, "\\") > 0
            for (k = i; k <= last && !blank(line[k]); k++) if (index(line[k], "\\")) tall = 1
            print hh row(hb)
            print l
            leaf = "table"
            continue
        }
        if (rest_blank) { end_leaf(); tcons = 0; print l; continue }
        # 표는 빈 줄이나 다른 블록의 시작에서 끝난다. 들여쓴 줄도 행으로 보고 엔터티로 바꾼다.
        if (leaf == "table") { print head(l, p) row(body(l, p)); continue }
        if (s >= 4 && leaf != "para") { end_leaf(); leaf = "code"; print l; continue }
        para_line(l, p)
    }
    flush()
}
'
