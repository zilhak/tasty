//! 액션 문(`dispatch_action_by_id`)에 누가 들어오는가를 명부로 붙든다.
//!
//! ## 무엇을 지키는가
//!
//! `src/adapters/ui/input/shortcuts/dispatch.rs` 의 `dispatch_action_by_id` 는 액션 id
//! (`KeybindingSettings` 의 필드 이름 그대로)를 받아 단축키와 같은 효과를 내는 **평평한
//! 문**이다. 문이 하나라 새 호출자가 붙기 쉽고, 붙는 순간 그 호출자는 사용자 액션 전부에
//! 한 번에 닿는다.
//!
//! 그래서 이 가드는 **호출자 집합**을 명부와 집합 동등으로 묶는다. 새 문이 생기면 빨개지고,
//! 그때 사람이 `docs/identity.md` 의 원칙 1(사용자 행동 ↔ 에이전트 행동 분리)·원칙 3
//! (포커스 독립성)과 대조한다. **그 대조는 이 가드가 안 한다.**
//!
//! ## 두 홉을 다 본다
//!
//! 직접 호출자만 보면 새는 자리가 있다. 오늘의 유일한 호출자는 팔레트의 지연 실행
//! (`dispatch_pending_command_palette`)이고, 그것은 `state.command_palette.pending_run`
//! 에서 꺼낸다. 즉 **`pending_run` 에 쓰는 자리도 이 문의 입구다.** 앞 홉만 붙들면 새
//! 기입자가 조용히 들어온다.
//!
//! ## 이 가드가 **안 묻는 것** — 원칙 2.2 부채
//!
//! 원칙 2.2 는 "에이전트 기능인데 GUI 에만 있는 것" 을 금지한다. 그 물음은 **액션마다
//! 답이 다르고**(`focus_pane_next` 는 원칙 3 이 에이전트 쪽에 없기를 **요구**한다),
//! 어떤 액션이 에이전트 기능인지는 측정이 아니라 **소유자의 판정**이다. 그래서 여기서는
//! 안 묻는다 — 모수만 도출해 두고 판정 칸은 비운다(아래 `the_action_set_is_derived_not_written`).
//!
//! ⇒ 2026-09-07 현재: **모수 71, 판정 0.** 이 수를 "부채 0" 으로 읽지 마라. 안 센 것이다.
//!
//! ## 문이 액션 전부를 열지는 않는다
//!
//! "단일 진입점" 이라는 말이 "모든 액션이 이 문을 지난다" 로 읽히기 쉬운데 아니다.
//! 실측 2026-09-07: 필드 71 · 문의 arm 42. 나머지는 다른 자리에서 처리된다(확대/축소는
//! `zoom.rs`, 복사/붙여넣기는 `copy_paste.rs`, modifier·슬롯 키 필드는 애초에 액션이 아니다).
//! 그래서 이 가드는 `arms ⊆ fields` 만 단정한다 — 반대 방향은 참이 아니고, 참이어야 할
//! 이유도 없다.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

const DOOR: &str = "dispatch_action_by_id";
const DOOR_FILE: &str = "src/adapters/ui/input/shortcuts/dispatch.rs";
/// 슬롯에 **쓰는** 모양만 센다. `pending_run.take()` 는 읽기이고, 읽는 자리는 이미
/// 문의 호출자로 붙들려 있다 — 같은 자리를 두 명부에 넣으면 한쪽을 고칠 때 다른 쪽이
/// 조용히 낡는다.
const PALETTE_SLOT: &str = "pending_run =";
const KEYBINDINGS: &str = "crates/tasty-settings/src/keybindings.rs";

/// 문을 직접 부르는 자리 — (파일, 사유).
const DOOR_CALLERS: &[(&str, &str)] = &[(
    "src/view/main/redraw.rs",
    "Command Palette 의 지연 실행. 팝업이 닫힌 뒤에 액션을 쏘려고 한 프레임 미룬 자리이고, \
     들어오는 값은 `state.command_palette.pending_run` 에서 꺼낸 것뿐이다",
)];

/// 팔레트 슬롯에 **쓰는** 자리 — 문의 둘째 입구. (파일, 사유).
const SLOT_WRITERS: &[(&str, &str)] = &[(
    "src/adapters/ui/popup/command_palette.rs",
    "팝업에서 Enter 로 고른 항목을 슬롯에 넣는다. 사람이 팝업을 열어 고른 것이라 \
     사용자 입력 경로이고, release 에서 에이전트는 팝업을 강제로 못 연다(원칙 1)",
)];

/// 걷기가 죽으면 아래 집합 동등이 양쪽 빈 채로 성립한다.
const MIN_SOURCES: usize = 300;
/// 액션 필드 하한. 2026-09-07 실측 71.
const MIN_FIELDS: usize = 50;
/// 문의 arm 하한. 2026-09-07 실측 42.
const MIN_ARMS: usize = 30;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn sources() -> Vec<(PathBuf, String)> {
    let out = rust_sources(&repo_root(), &["src"]);
    assert!(
        out.len() >= MIN_SOURCES,
        "`src` 에서 .rs 를 {} 개만 걷었다 — 걷기가 깨졌다. 모수가 비면 아래 집합 동등은 \
         양쪽이 비어 공짜로 성립한다",
        out.len()
    );
    out
}

/// `needle` 을 **코드에서** 쓰는 파일들 — 자기 정의 파일은 뺀다.
///
/// ★ **주석·문자열을 덮은 사본에 대고 묻는다.** 원문에 걸면 그 이름을 **설명하는 주석**이
/// 호출자로 세어진다. 실측으로 밟았다(2026-09-07): 원문 술어는 `src/view/main/keyboard.rs`
/// 를 문의 호출자로, `src/view/main/redraw.rs` 를 슬롯의 기입자로 셌는데 **둘 다 doc
/// 주석**이었다. 이 저장소는 그 답을 이미 갖고 있다 — [`mask_non_code`].
fn code_uses(needle: &str, own: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (rel, text) in sources() {
        let rel = rel.to_string_lossy().into_owned();
        if rel == own {
            continue;
        }
        if mask_non_code(&text).contains(needle) {
            out.insert(rel);
        }
    }
    out
}

fn check(found: BTreeSet<String>, roster: &[(&str, &str)], what: &str, why_it_matters: &str) {
    let listed: BTreeSet<String> = roster.iter().map(|(p, _)| (*p).to_string()).collect();
    let extra: Vec<&String> = found.difference(&listed).collect();
    let stale: Vec<&String> = listed.difference(&found).collect();
    assert!(
        extra.is_empty(),
        "{what} 에 명부에 없는 자리가 생겼다: {extra:?}\n{why_it_matters}\n\
         ★ 명부에 줄을 더해서 통과시키기 전에 `docs/identity.md` 원칙 1(사용자 행동 ↔ \
         에이전트 행동 분리)과 원칙 3(포커스 독립성)에 대고 읽어라. 그 대조는 이 가드가 \
         안 한다 — 여기서 멈추는 것이 그 대조를 하라는 뜻이다."
    );
    assert!(
        stale.is_empty(),
        "명부에 있는데 트리에 없는 {what} 자리다: {stale:?}\n\
         사라진 자리의 잔재는 다음 사람이 계속 검토하게 만든다 — 지워라."
    );
}

#[test]
fn the_action_door_has_only_the_registered_callers() {
    check(
        code_uses(DOOR, DOOR_FILE),
        DOOR_CALLERS,
        "액션 문의 호출자",
        "이 문은 액션 id 하나로 사용자 액션 전부에 닿는다. 새 호출자는 그 전부를 한 번에 \
         얻는다 — 그것이 에이전트 경로면 원칙 1 이 금지하는 '사용자 입력 재현' 이다.",
    );
}

#[test]
fn the_palette_slot_has_only_the_registered_writers() {
    check(
        code_uses(PALETTE_SLOT, "src/state/command_palette.rs"),
        SLOT_WRITERS,
        "팔레트 슬롯의 기입자",
        "슬롯에 쓰면 다음 프레임에 문이 열린다. 직접 호출자만 붙들면 이 둘째 입구로 \
         조용히 들어온다.",
    );
}

/// 액션 이름은 **도출한다 — 손으로 안 쓴다.**
///
/// 명부에 71 을 적으면 필드가 늘 때마다 낡는다(ADR-0139). 이름은 `KeybindingSettings`
/// 에서 읽고, 여기서 단정하는 것은 **관계**뿐이다: 문의 arm 은 전부 실재하는 액션 필드다.
///
/// 반대 방향(`fields ⊆ arms`)은 **단정하지 않는다.** 29 개 필드가 다른 자리에서 처리되고
/// (`zoom.rs` · `copy_paste.rs`), modifier·슬롯 키 필드는 애초에 액션이 아니다.
#[test]
fn the_action_set_is_derived_not_written() {
    let root = repo_root();
    let fields = keybinding_fields(&root);
    let arms = door_arms(&root);
    assert!(
        fields.len() >= MIN_FIELDS,
        "`KeybindingSettings` 필드를 {} 개만 뽑았다 (2026-09-07 실측 71) — 추출이 깨졌다",
        fields.len()
    );
    assert!(
        arms.len() >= MIN_ARMS,
        "액션 문의 arm 을 {} 개만 뽑았다 (2026-09-07 실측 42) — 추출이 깨졌다",
        arms.len()
    );
    let unknown: Vec<&String> = arms.difference(&fields).collect();
    assert!(
        unknown.is_empty(),
        "액션 문에 `KeybindingSettings` 에 없는 id 가 있다: {unknown:?}\n\
         그 arm 은 어떤 단축키로도 안 불리고 설정에도 안 나온다 — 오타이거나, 필드를 \
         지우면서 arm 을 안 지운 것이다."
    );
}

fn keybinding_fields(root: &std::path::Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(KEYBINDINGS))
        .unwrap_or_else(|e| panic!("{KEYBINDINGS} 를 읽지 못했다 — {e}"));
    let body = brace_body(&text, "pub struct KeybindingSettings")
        .expect("`KeybindingSettings` 선언을 못 찾았다 — 구조가 바뀌었으면 이 가드를 옮겨라");
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("pub ")
            && let Some(name) = rest.split(':').next()
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.insert(name.to_string());
        }
    }
    out
}

fn door_arms(root: &std::path::Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(DOOR_FILE))
        .unwrap_or_else(|e| panic!("{DOOR_FILE} 를 읽지 못했다 — {e}"));
    let at = text
        .find("fn dispatch_action_by_id")
        .expect("액션 문을 못 찾았다 — 이름이 바뀌었으면 이 가드도 함께 옮긴다");
    let body = brace_body(&text[at..], "match action_id")
        .expect("`match action_id` 를 못 찾았다 — 디스패치 모양이 바뀌었다");
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let t = line.trim();
        if !t.contains("=>") {
            continue;
        }
        let head = t.split("=>").next().unwrap_or("");
        for piece in head.split('|') {
            let p = piece.trim();
            if let Some(inner) = p.strip_prefix('"').and_then(|x| x.strip_suffix('"'))
                && !inner.is_empty()
                && inner
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                out.insert(inner.to_string());
            }
        }
    }
    out
}

/// `marker` 뒤 첫 `{` 부터 짝이 맞는 `}` 까지.
fn brace_body<'a>(src: &'a str, marker: &str) -> Option<&'a str> {
    let at = src.find(marker)?;
    let open = at + src[at..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn every_registered_place_carries_a_reason() {
    let thin: Vec<&str> = DOOR_CALLERS
        .iter()
        .chain(SLOT_WRITERS.iter())
        .filter(|(_, why)| why.split_whitespace().count() < 8)
        .map(|(p, _)| *p)
        .collect();
    assert!(
        thin.is_empty(),
        "사유가 너무 짧다 — 다음 사람이 그 자리가 왜 정당한지 **재현**할 수 있어야 한다: {thin:?}"
    );
}
