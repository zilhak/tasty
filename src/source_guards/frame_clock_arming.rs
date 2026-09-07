//! atlas 프레임 시계를 **무장하는 호출이 프레임 진입점 안에, append 보다 앞에, 루프
//! 밖에** 있는지 못 박는다.
//!
//! # 계약
//!
//! `GlyphAtlas::begin_frame` 은 프레임당 한 번 불려야 한다. 그 bump 가 per-page LRU
//! stamp 를 정하고, **프레임당 1회 eviction 스로틀을 다시 무장한다.** 스로틀을 푸는
//! 것은 그 bump 뿐이라, 안 부르면 atlas 는 한 번 evict 한 뒤 이후의 모든 eviction 을
//! 영구히 거절한다 — 증상은 glyph 가 조용히 rasterize 되지 않는 것뿐이다. 상태 기계
//! 자체(`FrameClock`)는 `crates/tasty-font` 에서 device 없이 검증되지만, **그 시계를
//! 실제로 감는가**는 이쪽 크레이트의 소스에 대한 물음이라 거기서 물을 수 없다.
//!
//! # 왜 이 자리가 판정 가능한가
//!
//! "프레임마다 부르는가" 는 그대로는 정적으로 판정할 수 없다 — 프레임은 런타임
//! 개념이다. 판정 가능한 형태로 좁힌 것이 이것이다:
//!
//! > `Renderer::append_terminal_viewport` 를 **부르는 모든 함수**는, 그 첫 append 보다
//! > 앞에서 `Renderer::begin_frame` 을 부르고, 그 호출이 그 함수 안의 **어떤 루프에도
//! > 들어 있지 않다.**
//!
//! 이 좁히기가 약한 술어가 아닌 이유는 **도달 폐포를 값으로 재서** 확인했기 때문이다.
//! `GlyphAtlas::get_or_insert` 를 부르는 자리는 본체에 넷이고
//! (`fill_surface` · `push_overlay_glyph` · `append_preedit_overlay` · `render_cell`),
//! 넷 다 비공개이며, 그 호출자를 따라 올라가면 전부
//! `Renderer::append_terminal_viewport` 의 본문 안에서 끝난다. 즉 atlas 에 닿는 길은
//! append 하나뿐이라, append 의 호출자 집합이 곧 프레임 진입점 집합이다. 그 전제가
//! 낡으면 조용히 낡지 않도록 [`the_only_paths_into_the_atlas_are_the_four_known_ones`]
//! 가 그 넷을 명부로 들고 있다.
//!
//! # 초록이 뜻하는 것
//!
//! **모수 안의 각 함수에서, `begin_frame` 호출이 텍스트로 존재하고, 첫 append 보다
//! 앞에 있고, 루프 블록 안에 있지 않다.** 그것뿐이다.
//!
//! 초록이 뜻하지 **않는** 것 여섯을 적는다. 정적 판정이라 못 보는 갈래는 반드시 있고,
//! 안 적으면 다음 사람이 이 초록을 "프레임 규율이 지켜진다" 로 읽는다. 아래는 짐작이
//! 아니라 술어를 합성 텍스트 다섯 형태에 직접 돌려 **잰** 결과다.
//!
//! - **그 호출이 실제로 실행된다는 것이 아니다.** `if cond { …begin_frame(); }` 는
//!   텍스트로 존재하고 루프 밖이라 통과한다. 조건 분기는 안 본다.
//! - **프레임당 정확히 한 번이라는 것이 아니다.** append 앞에 `begin_frame` 이 둘 있어도
//!   통과한다. 이 판정기가 세는 것은 위치지 횟수가 아니다.
//! - **한 프레임에 이 함수가 한 번만 불린다는 것이 아니다.** 모수 안의 함수를 한 프레임에
//!   두 번 부르면 시계가 두 번 감기고, 그것은 이 파일에서 안 보인다.
//! - **호출이 헬퍼 뒤로 숨으면 갈라지는데, 갈리는 자리가 짐작과 다르다.**
//!   `begin_frame` 만 헬퍼로 옮기면 **빨개진다**(안전한 방향 — 사람이 상수를 고치러
//!   온다). 둘 다 옮기면 그 함수가 모수에서 *빠지는* 것이 아니라 **헬퍼가 모수가
//!   된다** — 좌변이 사라지는 게 아니라 한 칸 내려간다. 헬퍼를 한 번만 부르면 초록이고
//!   그건 **옳은** 초록이다. 그런데 그 헬퍼를 **루프 안에서 부르면 역시 초록이고 그건
//!   틀린 초록이다**: 프레임당 1 회여야 할 bump 가 반복당 1 회가 됐는데, 그 루프는
//!   호출자에 있고 이 술어의 시야는 함수 **하나**다. 잃는 것은 좌변이 아니라 **문맥**이다.
//! - **`.begin_frame(` 은 수신자를 안 본다.** 본체에는 같은 이름의 다른 메서드가 있다
//!   (`src/view/base.rs` 의 뷰 계층 `begin_frame` — 호출 5 곳, 전부 `src/view/`).
//!   그 호출이 프레임 진입점 파일에 하나라도 들어오면 atlas 시계를 안 감고도 통과한다.
//!   오늘 모수 두 파일에는 0 건이라 실현되지 않았을 뿐이다.
//! - **atlas 로 가는 새 길은 모수 밖이다.** append 를 안 거치고 `get_or_insert` 에 닿는
//!   경로가 생기면 이 가드는 그 경로를 아예 안 본다. 그 고장만 아래 명부 시험이 잡는다 —
//!   그것도 "다시 폐포를 손으로 재라" 고 말할 뿐, 폐포 자체를 계산하지는 않는다.
//!
//! # 어기면 어느 쪽이 틀렸나
//!
//! 소스 쪽이다. 계약 본문은 `crates/tasty-font/src/lib.rs` 의 `FrameClock` 과
//! `docs/dev-guide/gpu-rendering.md` §① 에 있고, 이 판정기는 그 계약의 집행자지
//! 출처가 아니다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{line_of, mask_non_code, matching_delim, rust_sources, word_positions};

/// 프레임 시계를 감는 호출. 정의(`pub fn begin_frame(`)와 갈라내려고 점을 붙여 찾는다.
const ARM_CALL: &str = ".begin_frame(";
/// 그 시계가 stamp 로 찍히는 자리로 들어가는 호출 — 모수를 정하는 것도 이 이름이다.
const APPEND_CALL: &str = ".append_terminal_viewport(";
/// atlas 에 닿는 유일한 관문.
const ATLAS_CALL: &str = ".atlas.get_or_insert(";
/// `Renderer` 자신이 사는 파일. 여기 있는 두 이름은 정의라 호출이 아니다.
const RENDERER_DEF: &str = "src/gfx/renderer.rs";
/// 가드가 자기 상수를 호출로 세지 않도록 빼는 디렉토리. 이웃
/// (`dispatch_name_literals`)이 쓰는 것과 같은 형태다.
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];
/// `get_or_insert` 로 atlas 에 닿는 함수 전부. 이 넷이 곧 위 도달 폐포의 밑변이다.
const ATLAS_TOUCHING_FNS: &[&str] = &[
    "append_preedit_overlay",
    "fill_surface",
    "push_overlay_glyph",
    "render_cell",
];

fn rel_str(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

fn is_guard_file(rel: &str) -> bool {
    GUARD_DIRS.iter().any(|dir| rel.starts_with(dir))
}

/// `from` 이후 **같은 괄호 깊이**의 첫 `{` 블록 범위. `(`·`[` 그룹은 통째로 건너뛴다 —
/// `for (a, b) in xs {` 처럼 헤더에 괄호가 먼저 오는 형태를 위해서다. `;` 를 먼저
/// 만나면 블록이 없는 것이다.
fn block_after(masked: &str, from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < masked.len() {
        let c = masked[i..].chars().next()?;
        match c {
            '{' => return matching_delim(masked, i).map(|end| (i, end)),
            '(' | '[' => i = matching_delim(masked, i)? + 1,
            ';' => return None,
            _ => i += c.len_utf8(),
        }
    }
    None
}

/// 파일 안의 모든 `fn` 항목을 (이름, 본문 시작, 본문 끝, `fn` 키워드 위치)로.
/// 본문 없는 선언(`fn f(&self);`)은 건너뛴다.
fn fn_spans(masked: &str) -> Vec<(String, usize, usize, usize)> {
    let mut out = Vec::new();
    for kw in word_positions(masked, "fn") {
        let after = kw + "fn".len();
        let name: String = masked[after..]
            .chars()
            .skip_while(|c| c.is_whitespace())
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue; // `fn(u32) -> u32` 같은 타입 자리.
        }
        let Some((open, close)) = block_after(masked, after) else {
            continue;
        };
        out.push((name, open, close, kw));
    }
    out
}

/// `pos` 를 품는 가장 안쪽 `fn`.
fn enclosing_fn(
    spans: &[(String, usize, usize, usize)],
    pos: usize,
) -> Option<&(String, usize, usize, usize)> {
    spans
        .iter()
        .filter(|(_, open, close, _)| *open < pos && pos < *close)
        .min_by_key(|(_, open, close, _)| close - open)
}

/// `body` 범위 안의 루프 블록 범위들.
fn loop_blocks(masked: &str, open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for kw in ["for", "while", "loop"] {
        for pos in word_positions(masked, kw) {
            if pos <= open || pos >= close {
                continue;
            }
            if let Some(span) = block_after(masked, pos + kw.len())
                && span.1 <= close
            {
                out.push(span);
            }
        }
    }
    out
}

/// append 를 **부르는** 파일들 — 곧 프레임 진입점이 사는 곳.
fn frame_entry_files() -> Vec<(PathBuf, String)> {
    rust_sources()
        .into_iter()
        .filter(|(rel, _)| {
            let rel = rel_str(rel);
            rel != RENDERER_DEF && !is_guard_file(&rel)
        })
        .map(|(rel, src)| (rel, mask_non_code(&src)))
        .filter(|(_, masked)| masked.contains(APPEND_CALL))
        .collect()
}

#[test]
fn the_population_is_not_empty_and_names_itself() {
    let files: Vec<String> = frame_entry_files()
        .iter()
        .map(|(rel, _)| rel_str(rel))
        .collect();
    assert!(
        !files.is_empty(),
        "프레임 진입점을 하나도 못 찾았다 — `{APPEND_CALL}` 이 이름을 바꿨거나 스캔 \
         루트가 깨졌다. 빈 모수의 초록은 통과가 아니라 미측정이다"
    );
    // 모수가 어디인지 실패문에 남긴다. 수를 못 박지는 않는다 — surface 를 그리는
    // 새 진입점이 생기면 그것도 이 계약을 지켜야 하지 이 시험이 막을 일이 아니다.
    for rel in &files {
        assert!(
            rel.starts_with("src/gfx/"),
            "프레임 진입점이 `src/gfx/` 밖에서 나왔다: {rel}. 진짜 새 진입점이면 이 \
             단정을 넓혀라. 아니면 `{APPEND_CALL}` 이 다른 뜻으로 쓰인 것이다"
        );
    }
}

#[test]
fn every_frame_entry_arms_the_atlas_clock_before_it_appends() {
    let mut checked = 0usize;
    for (rel, masked) in frame_entry_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (append, _) in masked.match_indices(APPEND_CALL) {
            let (name, open, close, _) = enclosing_fn(&spans, append).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 `{APPEND_CALL}` 를 품는 함수를 못 찾았다 — 못 자른 \
                     상태의 초록은 통과가 아니니 이 가드의 함수 절단을 함께 고쳐라",
                    line_of(&masked, append)
                )
            });
            let arm = masked[*open..*close]
                .find(ARM_CALL)
                .map(|rel_pos| rel_pos + *open)
                .unwrap_or_else(|| {
                    panic!(
                        "{rel} 의 `fn {name}` 이 `{APPEND_CALL}` 를 부르면서 \
                         `{ARM_CALL}` 을 안 부른다. atlas 프레임 시계가 안 감기면 \
                         페이지를 한 번 evict 한 뒤 이후의 모든 eviction 을 영구히 \
                         거절하고, 증상은 glyph 가 조용히 안 그려지는 것뿐이다. \
                         호출을 헬퍼로 옮긴 것이면 이 가드의 상수를 같이 고쳐라"
                    )
                });
            assert!(
                arm < append,
                "{rel} 의 `fn {name}`: `{ARM_CALL}`(줄 {}) 이 `{APPEND_CALL}`(줄 {}) \
                 보다 뒤에 있다. 이번 프레임 glyph 가 지난 프레임 stamp 로 찍힌다",
                line_of(&masked, arm),
                line_of(&masked, append),
            );
            for (lo, hi) in loop_blocks(&masked, *open, *close) {
                assert!(
                    !(lo < arm && arm < hi),
                    "{rel} 의 `fn {name}`: `{ARM_CALL}`(줄 {}) 이 줄 {} 에서 시작하는 \
                     루프 안에 있다. 프레임당 1회여야 할 bump 가 반복당 1회가 되면 \
                     같은 프레임 안에서 스로틀이 계속 풀려 페이지가 서로를 몰아낸다 \
                     (thrashing) — 그리고 LRU stamp 가 프레임을 세는 의미를 잃는다",
                    line_of(&masked, arm),
                    line_of(&masked, lo),
                );
            }
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "append 호출을 하나도 안 봤다 — 위 모수 시험이 초록인데 여기서 0 이면 함수 \
         절단이 깨진 것이다"
    );
}

/// 위 모수의 숨은 전제 — **atlas 로 가는 길이 append 하나뿐**이라는 것 — 이 조용히
/// 낡지 않게 한다. 이 시험은 폐포를 계산하지 않는다. 밑변이 달라지면 빨개져서 사람에게
/// 폐포를 다시 재라고 말할 뿐이다.
#[test]
fn the_only_paths_into_the_atlas_are_the_four_known_ones() {
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut public: Vec<String> = Vec::new();
    for (rel, src) in rust_sources() {
        let rel = rel_str(&rel);
        if is_guard_file(&rel) {
            continue;
        }
        let masked = mask_non_code(&src);
        if !masked.contains(ATLAS_CALL) {
            continue;
        }
        let spans = fn_spans(&masked);
        for (pos, _) in masked.match_indices(ATLAS_CALL) {
            let (name, _, _, kw) = enclosing_fn(&spans, pos).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 `{ATLAS_CALL}` 를 품는 함수를 못 찾았다",
                    line_of(&masked, pos)
                )
            });
            let line_start = masked[..*kw].rfind('\n').map_or(0, |i| i + 1);
            if masked[line_start..*kw].contains("pub") {
                public.push(format!("{rel}: fn {name}"));
            }
            found.insert(name.clone());
        }
    }
    let expected: BTreeSet<String> = ATLAS_TOUCHING_FNS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        found, expected,
        "atlas 에 닿는 함수 명부가 달라졌다. 이 모듈 위쪽 문서가 \
         `Renderer::append_terminal_viewport` 를 유일한 관문이라고 재 놓았는데, 그 \
         측정이 낡았다. 새 함수의 호출자를 따라 올라가 append 본문 안에서 끝나는지 \
         손으로 다시 재고, 안 끝나면 `frame_entry_files` 의 모수를 넓혀라"
    );
    assert!(
        public.is_empty(),
        "atlas 에 닿는 함수가 공개로 바뀌었다: {public:?}. 공개면 append 를 안 거치는 \
         호출자가 생길 수 있고, 그러면 위 모수가 프레임 진입점 전체를 안 덮는다"
    );
}
