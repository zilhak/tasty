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
//! > 앞에서 **그 append 와 같은 수신자 위의** `begin_frame` 을 부르고, 그 호출이 그 함수
//! > 안의 **어떤 루프에도 들어 있지 않다.**
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
//! - **한 프레임에 이 함수가 한 번만 불린다는 것이 아니다 — 다만 루프 형태는 닫혔다.**
//!   모수 함수가 **루프 안에서** 불리는 것은
//!   [`no_frame_entry_is_called_from_inside_a_loop`] 이 본다(그 시험이 이 파일에서
//!   유일하게 함수 하나를 넘는다). 안 닫힌 것은 나머지다: 호출자가 둘 이상이라 한
//!   프레임에 두 경로가 다 지나가는 형태, 조건 분기로 두 번 부르는 형태.
//! - **호출이 헬퍼 뒤로 숨으면 갈라지는데, 갈리는 자리가 짐작과 다르다.**
//!   `begin_frame` 만 헬퍼로 옮기면 **빨개진다**(안전한 방향 — 사람이 상수를 고치러
//!   온다). 둘 다 옮기면 그 함수가 모수에서 *빠지는* 것이 아니라 **헬퍼가 모수가
//!   된다** — 좌변이 사라지는 게 아니라 한 칸 내려간다. 그 하강 자체는
//!   [`the_population_is_not_empty_and_names_itself`] 의 이름 집합 동등이 잡는다(헬퍼
//!   이름은 `FRAME_ENTRY_FNS` 에 없다). **하지만 잡는 것은 그 시험 하나뿐이다** —
//!   [`every_frame_entry_arms_the_atlas_clock_before_it_appends`] 는 하강한 헬퍼를 그냥
//!   새 모수로 받아 초록이고, 그 헬퍼를 **루프 안에서 부르는 호출자**는 여전히 안 보인다:
//!   프레임당 1 회여야 할 bump 가 반복당 1 회가 됐는데 그 루프는 호출자에 있고 이 술어의
//!   시야는 함수 **하나**다. 잃는 것은 좌변이 아니라 **문맥**이고, 명부는 좌변이
//!   내려간 사실만 알린다.
//! - **수신자는 보지만 텍스트로만 본다.** 무장 호출은 append 앞의 수신자
//!   (오늘 두 자리 다 `self.renderer`)에 묶여 있어, 본체의 동명이인
//!   (`src/view/base.rs` 의 뷰 계층 `begin_frame` — 호출 5 곳, 전부 `src/view/`)이
//!   진입점 파일에 들어와도 무장으로 세어지지 않는다. 남는 한계는 텍스트 동일성이다:
//!   같은 경로 문자열이 실행 시 같은 객체라는 보장은 이 판정기가 못 준다. 반대 방향의
//!   오탐도 있다 — `let r = &mut self.renderer;` 로 묶어 쓰는 리팩터는 수신자 문자열이
//!   달라져 **빨개진다.** 결함이 아닌데 빨간 것이지만 안전한 방향이라 그대로 둔다.
//! - **atlas 로 가는 새 길은 모수 밖이다.** append 를 안 거치고 `get_or_insert` 에 닿는
//!   경로가 생기면 이 가드는 그 경로를 아예 안 본다. 그 고장만 아래 명부 시험이 잡는다 —
//!   그것도 "다시 폐포를 손으로 재라" 고 말할 뿐, 폐포 자체를 계산하지는 않는다.
//!
//! # 시야가 함수 하나다 — 그리고 무엇을 재면 그 벽이 무너지나
//!
//! 위 마지막 두 항목이 같은 한 가지에서 나온다. `enclosing_fn` 이 최소 span 을 잡으므로
//! **판정 단위가 함수 하나**이고, 그래서 호출자에 있는 것(루프·조건·호출 횟수)은 원리적
//! 으로 안 보인다. 이건 이 파일만의 성질이 아니다 — `src/source_guards/` 의 판정기 40 개
//! 중 호출자 방향으로 한 단계라도 올라가는 것은 **0 건**이다(실측). 그러니 다음 사람이
//! 이 형태의 가드를 하나 더 지어도 같은 벽에 부딪힌다.
//!
//! **재서 절반을 깼다.** 역방향 한 단계를 도구로 지었다 — `mod.rs` 의 [`callers_of`] 가
//! 이름으로 호출처를 전수 스캔하고 각 자리에서 품는 함수와 루프 여부를 값으로 준다.
//! [`no_frame_entry_is_called_from_inside_a_loop`] 이 그것을 `FRAME_ENTRY_FNS` 에 적용해,
//! **모수 함수가 루프 안에서 불리는가**를 본다(좌변 2 건 — `src/gfx/gpu.rs` 의 두 자리).
//!
//! 안 깬 절반은 **하강한 헬퍼**의 호출자다. 물음과 도구는 같고 적용 대상만 다르다:
//! 하강이 일어나면 헬퍼 이름을 [`callers_of`] 에 넣으면 된다. 지금 안 지은 근거는
//! 그대로다 — 오늘 좌변에 헬퍼 하강이 **0 건**이고(`FRAME_ENTRY_FNS` 의 두 함수가
//! append 를 직접 부른다), 실현되지 않은 형태에 좌변을 만들면 빈 좌변의 초록이 된다.
//! **재검토 조건은 값으로 배선돼 있다**: 하강이 일어나면
//! [`the_population_is_not_empty_and_names_itself`] 가 먼저 빨개져 사람을 부른다(헬퍼
//! 이름은 명부에 없다). 그 실패문을 들고 온 사람이 이 문단을 읽는다 — 면제가 아니라
//! **예약**이고, 도구가 생긴 만큼 그 예약은 더 싸졌다.
//!
//! # 어기면 어느 쪽이 틀렸나
//!
//! 소스 쪽이다. 계약 본문은 `crates/tasty-font/src/lib.rs` 의 `FrameClock` 과
//! `docs/dev-guide/gpu-rendering.md` §① 에 있고, 이 판정기는 그 계약의 집행자지
//! 출처가 아니다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{
    CallSite, callers_of, enclosing_fn, fn_spans, line_of, loop_blocks, mask_non_code, rust_sources,
};

/// 프레임 시계를 감는 호출. 정의(`pub fn begin_frame(`)와 갈라내려고 점을 붙여 찾는다.
const ARM_CALL: &str = ".begin_frame(";
/// 그 시계가 stamp 로 찍히는 자리로 들어가는 호출 — 모수를 정하는 것도 이 이름이다.
const APPEND_CALL: &str = ".append_terminal_viewport(";
/// atlas 에 닿는 유일한 관문.
const ATLAS_CALL: &str = ".atlas.get_or_insert(";
/// 가드가 자기 상수를 호출로 세지 않도록 빼는 디렉토리. 이웃
/// (`dispatch_name_literals`)이 쓰는 것과 같은 형태다.
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];
/// append 를 부르는 함수 전부 — **좌변의 이름 명부**다. 파일 접두사가 아니라 이름으로
/// 못 박는 이유는 아래 모수 시험 주석에 있다.
const FRAME_ENTRY_FNS: &[&str] = &["capture_surface_to_png", "render_terminals"];
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

/// 호출 앞에 붙은 수신자 경로. `self.renderer.append_terminal_viewport(` 의 `.` 위치를
/// 주면 `self.renderer` 를 돌려준다.
///
/// 식별자 문자와 점만 뒤로 훑는다. 그래서 수신자가 호출 체인(`foo().bar.begin_frame()`)
/// 이면 뒷조각(`bar`)만 잡히고, 그것은 append 쪽 수신자와 안 맞아 **빨개진다** — 안전한
/// 방향이다(사람이 와서 본다).
fn receiver_before(masked: &str, call_at: usize) -> &str {
    let head = &masked[..call_at];
    let start = head
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .map_or(0, |i| i + 1);
    &head[start..]
}

/// `{recv}{call}` 을 `[from, to)` 안에서 찾되, 매치 왼쪽이 식별자 문자면 버린다
/// (`myself.renderer.` 가 `self.renderer.` 로 읽히지 않게).
fn find_call_on(masked: &str, from: usize, to: usize, recv: &str, call: &str) -> Option<usize> {
    let needle = format!("{recv}{call}");
    masked[from..to]
        .match_indices(&needle)
        .map(|(at, _)| at + from)
        .find(|at| {
            masked[..*at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '.'))
        })
}

/// append 를 **부르는** 파일들 — 곧 프레임 진입점이 사는 곳.
fn frame_entry_files() -> Vec<(PathBuf, String)> {
    rust_sources()
        .into_iter()
        .filter(|(rel, _)| !is_guard_file(&rel_str(rel)))
        .map(|(rel, src)| (rel, mask_non_code(&src)))
        .filter(|(_, masked)| masked.contains(APPEND_CALL))
        .collect()
}

/// append 를 **품는 함수**들 — (파일, 함수 이름). 위 파일 목록이 좌변의 *어디*라면
/// 이쪽이 좌변의 *무엇*이다.
fn frame_entry_fns() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (rel, masked) in frame_entry_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (append, _) in masked.match_indices(APPEND_CALL) {
            let (name, ..) = enclosing_fn(&spans, append).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 `{APPEND_CALL}` 를 품는 함수를 못 찾았다 — 못 자른 \
                     상태의 초록은 통과가 아니니 이 가드의 함수 절단을 함께 고쳐라",
                    line_of(&masked, append)
                )
            });
            out.push((rel.clone(), name.clone()));
        }
    }
    out
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
    for rel in &files {
        assert!(
            rel.starts_with("src/gfx/"),
            "프레임 진입점이 `src/gfx/` 밖에서 나왔다: {rel}. 진짜 새 진입점이면 이 \
             단정을 넓혀라. 아니면 `{APPEND_CALL}` 이 다른 뜻으로 쓰인 것이다"
        );
    }
    // 좌변을 **이름으로** 못 박는다. 접두사만 보면 모수가 갈아치워져도 초록이다 —
    // `render_terminals` 가 사라지고 `draw_one` 이 그 자리에 서도 둘 다 `src/gfx/`
    // 라서 접두사 단정은 통과한다. 그것이 이 모듈 위쪽에 적힌 **하강**(호출을 헬퍼
    // 뒤로 옮기면 헬퍼가 모수가 된다)의 조용한 갈래고, 이름 집합 동등은 그 갈래를
    // 소리나게 만든다: 헬퍼 이름은 이 명부에 없으니 여기서 빨개진다.
    let found: BTreeSet<String> = frame_entry_fns().into_iter().map(|(_, f)| f).collect();
    let expected: BTreeSet<String> = FRAME_ENTRY_FNS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        found, expected,
        "프레임 진입점 함수 명부가 달라졌다. 진짜 새 진입점이면 `FRAME_ENTRY_FNS` 에 \
         더하고 그 함수도 이 계약을 지키게 해라. 이름만 바뀐 것이면 상수를 고쳐라. \
         append 를 헬퍼 뒤로 옮긴 것이면 **모수가 그 헬퍼로 내려간 것**이고, 그때 이 \
         판정기의 시야는 호출자의 루프를 잃는다 — 옮기기 전에 그 루프를 확인해라"
    );
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
            // 무장 호출을 **append 와 같은 수신자에** 묶는다. `.begin_frame(` 만
            // 보면 본체의 동명이인(`src/view/base.rs` 의 뷰 계층 `begin_frame`)이
            // 이 자리에 들어와도 통과한다 — atlas 시계는 안 감긴 채로.
            let recv = receiver_before(&masked, append);
            let arm = find_call_on(&masked, *open, *close, recv, ARM_CALL).unwrap_or_else(|| {
                panic!(
                    "{rel} 의 `fn {name}` 이 `{recv}{APPEND_CALL}` 를 부르면서 \
                     `{recv}{ARM_CALL}` 을 안 부른다. atlas 프레임 시계가 안 감기면 \
                     페이지를 한 번 evict 한 뒤 이후의 모든 eviction 을 영구히 \
                     거절하고, 증상은 glyph 가 조용히 안 그려지는 것뿐이다. \
                     `{ARM_CALL}` 이 다른 수신자 위에 있으면 그것은 이 계약이 \
                     말하는 시계가 아니다. 호출을 헬퍼로 옮긴 것이면 이 가드의 \
                     상수를 같이 고쳐라"
                )
            });
            assert!(
                arm < append,
                "{rel} 의 `fn {name}`: `{recv}{ARM_CALL}`(줄 {}) 이 `{APPEND_CALL}`(줄 {}) \
                 보다 뒤에 있다. 이번 프레임 glyph 가 지난 프레임 stamp 로 찍힌다",
                line_of(&masked, arm),
                line_of(&masked, append),
            );
            for (lo, hi) in loop_blocks(&masked, *open, *close) {
                assert!(
                    !(lo < arm && arm < hi),
                    "{rel} 의 `fn {name}`: `{recv}{ARM_CALL}`(줄 {}) 이 줄 {} 에서 시작하는 \
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

/// 모수 함수가 **루프 안에서 불리지 않는가.** 이 판정기가 처음으로 함수 하나를 넘는다.
///
/// 위 두 시험은 모수 함수의 *안*을 본다. 이 시험은 그 함수를 **부르는 자리**를 본다 —
/// 모수 함수가 루프 안에서 불리면 한 프레임에 시계가 여러 번 감기고, 그러면 프레임당
/// 1 회여야 할 eviction 스로틀이 반복마다 풀리며 LRU stamp 가 프레임을 세는 의미를
/// 잃는다. 모듈 위쪽 "초록이 뜻하지 않는 것" 의 셋째 항목이 말하던 갈래이고, 그중
/// **루프 형태만** 이 시험이 닫는다.
#[test]
fn no_frame_entry_is_called_from_inside_a_loop() {
    let mut seen = 0usize;
    for name in FRAME_ENTRY_FNS {
        let sites = callers_of(name, GUARD_DIRS);
        assert!(
            !sites.is_empty(),
            "`{name}` 을 부르는 자리를 하나도 못 찾았다. 이름이 바뀌었거나 유일한 \
             호출자가 사라진 것이다 — 빈 좌변의 초록은 통과가 아니라 미측정이다"
        );
        for CallSite {
            rel,
            caller,
            in_loop,
            line,
            ..
        } in &sites
        {
            let caller = caller.as_deref().unwrap_or_else(|| {
                panic!(
                    "{rel}:{line} 의 `{name}` 호출을 품는 함수를 못 잘랐다. 못 자른 \
                     상태에서는 루프 여부가 측정값이 아니니 통과로 접지 않는다"
                )
            });
            assert!(
                !in_loop,
                "{rel}:{line}: `fn {caller}` 가 `{name}` 을 **루프 안에서** 부른다. \
                 그 함수는 첫머리에서 atlas 프레임 시계를 감으므로, 한 프레임에 시계가 \
                 반복 횟수만큼 감긴다 — 프레임당 1 회여야 할 eviction 스로틀이 반복마다 \
                 풀려 페이지가 서로를 몰아내고(thrashing), LRU stamp 는 프레임을 세는 \
                 의미를 잃는다. 루프 밖에서 한 번만 불러라"
            );
            seen += 1;
        }
    }
    assert!(
        seen > 0,
        "호출 자리를 하나도 안 봤다 — 위 단정이 초록인데 여기서 0 이다"
    );
}
