"""Core::apply 밖의 구조 상태 writer를 (지표, 파일, 함수) 단위로 센다.

입력은 test 전용 줄을 비우고 주석·문자열을 가린 사본이다. 줄 번호는 원본과 같다.
정규식은 필드·메서드 이름 중심이며 공백·줄바꿈을 허용해 rustfmt가 나눈 체인도 잡는다.
수신자 타입은 해석하지 않으므로 같은 이름의 다른 필드도 센다. 한계는
docs/dev-guide/ci-gates.md의 구조 writer 래칫 절에 있다.

지표는 field·terminal·subop·appstate 네 가지다.
출력: 한 줄에 hit 하나, 탭 구분 `metric bucket file fn line kind`.
"""

import os
import re
import sys

# Core::apply가 호출하는 구현 파일. 이 파일 안의 쓰기는 입구 안으로 보고 세지 않는다.
APPLY_PATH_FILES = [
    "src/core/impl_attach.rs",
    "src/core/impl_category.rs",
    "src/core/impl_close.rs",
    "src/core/impl_convert.rs",
    "src/core/impl_mirror.rs",
    "src/core/impl_move.rs",
    "src/core/impl_move_container.rs",
    "src/core/impl_split.rs",
    "src/core/impl_tab.rs",
    "src/core/impl_workspace.rs",
    "src/core/restore_rebuild.rs",
]

# CoreState 구조가 아닌 같은 이름의 데이터를 다루는 곳. 파일은 접두 일치, 함수는 정확 일치다.
EXCLUDED_PREFIXES = [
    # TerminalStore 자체 구현. 호출자의 insert를 센다.
    "src/core/terminal_store.rs",
    # 저널 importer의 중간 구조를 채운다.
    "src/core/layout_persistence/import.rs",
    # 프리셋 저장소와 미리보기 모델을 편집한다.
    "src/adapters/ui/preset.rs",
    "src/adapters/ui/preset/",
]
EXCLUDED_FNS = {
    # 저장할 프리셋 사본의 이름을 바꾼다.
    ("src/intent/preset.rs", "store_preset"),
    # Explorer 패널 내부 탭을 닫는다. surface 콘텐츠이며 구조가 아니다.
    ("src/adapters/ui/egui_panels.rs", "apply_to_explorer_panel"),
}

APPSTATE_PREFIXES = ["src/state.rs", "src/state/"]

ASSIGN_FIELDS = (
    "name|subtitle|description|attach_mapping|mirror|category"
    "|explicit_name|layout_opt|tabs|workspaces|categories"
)
VEC_FIELDS = "tabs|workspaces|categories"
VEC_METHODS = (
    "push|insert|remove|swap_remove|clear|swap|retain|retain_mut|drain|truncate|pop"
    "|extend|append|splice|sort\\w*|dedup\\w*|reverse|rotate_left|rotate_right|resize"
)

FIELD_WRITES = [
    ("assign", re.compile(r"\.\s*(?:" + ASSIGN_FIELDS + r")\s*=(?![=>])")),
    ("index", re.compile(r"\.\s*(?:" + VEC_FIELDS + r")\s*\[[^\]]*\]\s*=(?![=>])")),
    ("vec", re.compile(r"\.\s*(?:" + VEC_FIELDS + r")\s*\.\s*(?:" + VEC_METHODS + r")\s*\(")),
    ("mem", re.compile(
        r"\b(?:replace|swap|take)\s*\(\s*&\s*mut\s+[^,;(){}]*?\.\s*(?:" + VEC_FIELDS + r")\b")),
    ("setter", re.compile(r"\.\s*(?:set_attach_mapping|set_category)\s*\(")),
]
TERMINAL_INSERT = re.compile(r"\.\s*terminals\s*\.\s*insert\s*\(")

# AppState 함수가 구조를 바꾸는지 판정할 때 쓰는 하위 연산 이름.
STRUCTURAL_CALLS = [
    "close_pane_preserving_focus", "detach_pane_preserving_focus", "split_pane_in_place",
    "close_pane", "detach_pane", "replace_pane", "insert_pane_beside",
    "add_terminal_marker_tab", "add_terminal_marker_tab_background",
    "split_active_surface_marker", "split_surface_by_id_marker",
    "split_surface_by_id_with_surface", "remove_tab_preserving_active",
    "take_tab_preserving_active", "close_tab", "close_active_tab", "close_tab_by_id",
    "add_surface_tab", "add_surface_tab_background", "move_tab", "take_layout",
    "put_layout", "close_surface", "put_surface", "split_focused_surface",
    "split_surface_by_id", "split_surface_by_id_generic", "split_with_surface",
    "split_with_node", "extract_surface", "replace_surface", "set_attach_mapping",
    "set_category", "push_closed_item", "set_workspace_category", "create_category",
    "rename_category", "delete_category", "reorder_category",
    "apply_create_workspace_inner", "create_default_workspace",
]
STRUCTURAL_CALL_RE = re.compile(
    r"(?<!fn )(?<![\w])(?:" + "|".join(STRUCTURAL_CALLS) + r")\s*\(")
# src/state 밖에서 하위 연산을 직접 부르는 곳. setter는 field 지표가 이미 센다.
SUBOP_RE = re.compile(
    r"\.\s*(" + "|".join(n for n in STRUCTURAL_CALLS
                         if n not in ("set_attach_mapping", "set_category")) + r")\s*\(")
# 수신자가 `state`이면 AppState 메서드 호출이다. 그 함수는 appstate 지표가 센다.
APPSTATE_RECEIVER_RE = re.compile(r"\bstate\s*$")
DIRECT_STRUCTURAL_EXTRA = [
    re.compile(r"\.\s*terminals\s*\.\s*(?:insert|remove|replace)\s*\("),
    re.compile(r"\bnext_ids\s*\.\s*next_\w+\s*\("),
]

FN_RE = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
PUB_RE = re.compile(r"\bpub\s*(?:\(\s*crate\s*\)\s*)?(?:const\s+|async\s+|unsafe\s+)*$")


def rust_files(root):
    out = []
    base = os.path.join(root, "src")
    for dirpath, _dirs, files in os.walk(base):
        for f in files:
            if f.endswith(".rs"):
                full = os.path.join(dirpath, f)
                out.append(os.path.relpath(full, root).replace(os.sep, "/"))
    return sorted(out)


def match_close(text, i, open_ch, close_ch):
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == open_ch:
            depth += 1
        elif c == close_ch:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def functions(text):
    """(name, is_pub, body_start, body_end) 목록. 본문이 없는 선언은 뺀다."""
    out = []
    for m in FN_RE.finditer(text):
        p = text.find("(", m.end())
        if p < 0:
            continue
        pe = match_close(text, p, "(", ")")
        if pe < 0:
            continue
        j = pe + 1
        while j < len(text) and text[j] not in "{;":
            j += 1
        if j >= len(text) or text[j] == ";":
            continue
        be = match_close(text, j, "{", "}")
        if be < 0:
            continue
        k = m.start()
        while k > 0 and text[k - 1] not in "{};":
            k -= 1
        prefix = re.sub(r"#\[[^\]]*\]", " ", text[k:m.start()])
        is_pub = PUB_RE.search(prefix.rstrip() + " ") is not None
        out.append((m.group(1), is_pub, j, be))
    return out


def enclosing(fns, pos):
    best = None
    for f in fns:
        if f[2] <= pos <= f[3] and (best is None or f[2] > best[2]):
            best = f
    return best[0] if best else "-"


def excluded(rel, fn):
    if any(rel.startswith(p) for p in EXCLUDED_PREFIXES):
        return True
    return (rel, fn) in EXCLUDED_FNS


def bucket_of(rel):
    if rel in APPLY_PATH_FILES:
        return None
    return "core" if rel.startswith("src/core/") else "outside"


def line_of(text, pos):
    return text.count("\n", 0, pos) + 1


def main():
    if len(sys.argv) != 2:
        print("usage: core_writer_scan.py <masked-root>", file=sys.stderr)
        return 2
    root = sys.argv[1]
    missing = [p for p in APPLY_PATH_FILES + EXCLUDED_PREFIXES
               if not os.path.exists(os.path.join(root, p.rstrip("/")))]
    missing += [f for f, _ in EXCLUDED_FNS if not os.path.exists(os.path.join(root, f))]
    if missing:
        for p in missing:
            print("[core-writer] 목록의 경로가 없다: " + p, file=sys.stderr)
        print("[core-writer] 입구 파일·제외 목록을 현재 트리에 맞춘 뒤 다시 돌려라.",
              file=sys.stderr)
        return 3
    files = rust_files(root)
    if not files:
        print("[core-writer] src 아래 .rs를 찾지 못했다.", file=sys.stderr)
        return 3

    rows = []
    appstate_fns = {}
    for rel in files:
        with open(os.path.join(root, rel), encoding="utf-8") as fh:
            text = fh.read()
        fns = functions(text)
        bucket = bucket_of(rel)
        if bucket is not None:
            for kind, rx in FIELD_WRITES:
                for m in rx.finditer(text):
                    fn = enclosing(fns, m.start())
                    if not excluded(rel, fn):
                        rows.append(("field", bucket, rel, fn, line_of(text, m.start()), kind))
            for m in TERMINAL_INSERT.finditer(text):
                fn = enclosing(fns, m.start())
                if not excluded(rel, fn):
                    rows.append(("terminal", bucket, rel, fn, line_of(text, m.start()), "insert"))
            if not any(rel.startswith(p) for p in APPSTATE_PREFIXES):
                for m in SUBOP_RE.finditer(text):
                    fn = enclosing(fns, m.start())
                    if excluded(rel, fn) or APPSTATE_RECEIVER_RE.search(text[:m.start()]):
                        continue
                    rows.append(("subop", bucket, rel, fn, line_of(text, m.start()), m.group(1)))
        if any(rel.startswith(p) for p in APPSTATE_PREFIXES):
            for name, is_pub, bs, be in fns:
                body = text[bs:be + 1]
                direct = (any(rx.search(body) for _k, rx in FIELD_WRITES)
                          or STRUCTURAL_CALL_RE.search(body) is not None
                          or any(rx.search(body) for rx in DIRECT_STRUCTURAL_EXTRA))
                appstate_fns.setdefault(name, []).append(
                    {"rel": rel, "pub": is_pub, "body": body, "direct": direct,
                     "line": line_of(text, bs)})

    # 같은 AppState 모듈의 구조 함수를 이름으로 부르는 함수도 구조 함수로 본다.
    defs = [dict(d, name=n) for n, ds in appstate_fns.items() for d in ds]
    names = {d["name"] for d in defs if d["direct"]}
    changed = True
    while changed:
        changed = False
        call_re = re.compile(r"(?:\.|::)\s*(?:" + "|".join(map(re.escape, sorted(names)))
                             + r")\s*\(") if names else None
        for d in defs:
            if d["direct"] or call_re is None:
                continue
            if call_re.search(d["body"]):
                d["direct"] = True
                if d["name"] not in names:
                    names.add(d["name"])
                    changed = True
    for d in sorted(defs, key=lambda d: (d["rel"], d["line"])):
        if d["pub"] and d["direct"]:
            rows.append(("appstate", "state", d["rel"], d["name"], d["line"], "fn"))

    for r in rows:
        print("\t".join(str(x) for x in r))
    return 0


if __name__ == "__main__":
    sys.exit(main())
