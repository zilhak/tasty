//! 전체화면 무대 렌더 분기의 **위치 계약**을 소스 구조로 고정하는 가드.
//!
//! 무대를 "뒤 렌더 스킵" 으로 나이브하게 구현하면 조용히 죽는 기능이 있다. 이 세
//! 제약은 어느 것도 GPU 없이 런타임으로 단정할 수 없어(스크린샷·attach relay·
//! swapchain 이 전부 실제 어댑터를 요구한다) 소스 구조로 고정한다. 선례:
//! `tests/design_token_adherence.rs` / `tests/no_emoji_in_source.rs`.
//!
//! 1. **분기는 offscreen surface 스크린샷 뒤에 온다.** 앞으로 옮기면
//!    `ui.screenshot --surface <id>` 요청이 큐에 남아 영구 대기한다 — release
//!    에이전트 기능이라 무대 때문에 죽으면 안 된다.
//! 2. **분기는 레이아웃/`resize_all` 앞에 온다.** 뒤로 밀면 무대 중에도 PTY grid 가
//!    재계산돼 "원본은 진입 시점 그대로" 계약이 깨진다.
//! 3. **무대 경로도 window 캡처 + `present` 를 수행한다.** 건너뛰면 `ui.screenshot`
//!    (window)이 영구 대기하고, 그러면 무대가 제대로 그려졌는지 자동 검증할 수단이
//!    사라진다.
//! 4. **`render_if_dirty` 는 무대를 이유로 조기 반환하지 않는다.** attach mesh relay 가
//!    그 앞에 있어, 끊으면 로컬 전체화면이 원격 사용자 화면을 멈춘다.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let p: PathBuf = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// `needle` 이 정확히 한 번 나오는 바이트 오프셋.
fn only_at(hay: &str, needle: &str, what: &str) -> usize {
    let n = hay.matches(needle).count();
    assert_eq!(
        n, 1,
        "{what}: `{needle}` 이 {n} 번 나온다 — 이 가드는 유일 출현을 전제한다. \
         구조가 바뀌었으면 가드도 함께 갱신하라."
    );
    hay.find(needle).expect("checked above")
}

/// `fn <name>` 부터 다음 최상위 `\n    fn ` / `\n    pub` 직전까지의 대략적 본문.
fn fn_body<'a>(src: &'a str, header: &str) -> &'a str {
    let start = src
        .find(header)
        .unwrap_or_else(|| panic!("no fn: {header}"));
    let rest = &src[start + header.len()..];
    // 같은 impl 안의 다음 함수 선언(4칸 들여쓰기)까지를 본문으로 본다.
    let end = rest
        .find("\n    fn ")
        .into_iter()
        .chain(rest.find("\n    pub fn "))
        .chain(rest.find("\n    ///"))
        .min()
        .unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn stage_branch_sits_between_offscreen_capture_and_layout() {
    let src = read("src/gfx/gpu.rs");
    let screenshot = only_at(
        &src,
        "self.handle_pending_surface_screenshot(engine);",
        "offscreen 캡처",
    );
    let branch = only_at(&src, "if state.fullscreen_stage_active() {", "무대 분기");
    let resize = only_at(&src, "state.resize_all(", "레이아웃 resize_all");
    assert!(
        screenshot < branch,
        "무대 분기가 offscreen surface 스크린샷보다 앞에 있다 — \
         `ui.screenshot --surface <id>` 가 무대 중 영구 대기하게 된다."
    );
    assert!(
        branch < resize,
        "무대 분기가 `state.resize_all` 뒤로 밀렸다 — 무대 중 PTY grid 가 재계산돼 \
         '원본은 진입 시점 그대로' 계약이 깨진다."
    );
}

#[test]
fn stage_frame_keeps_window_capture_and_present() {
    let src = read("src/gfx/gpu.rs");
    let body = fn_body(&src, "fn render_fullscreen_stage(");
    assert!(
        body.contains("self.pending_screenshot.take()"),
        "무대 프레임이 window 스크린샷 캡처를 건너뛴다 — `ui.screenshot` 요청이 \
         영구 대기하고, 무대 검증 수단이 사라진다."
    );
    assert!(
        body.contains("output.present()"),
        "무대 프레임이 present 를 건너뛴다 — 화면에 아무것도 올라가지 않는다."
    );
}

#[test]
fn render_if_dirty_has_no_stage_early_return() {
    let src = read("src/view/main/redraw.rs");
    let body = fn_body(&src, "fn render_if_dirty(");
    assert!(
        !body.contains("fullscreen_stage"),
        "`render_if_dirty` 가 무대 조건을 참조한다 — 이 함수는 attach mesh relay 를 \
         품고 있어 무대를 이유로 끊으면 원격 사용자 화면이 멈춘다(주체 간 비침범). \
         무대 분기는 `Gpu::render` 안에 둔다."
    );
}

#[test]
fn terminal_resize_is_the_gated_one_in_handle_redraw() {
    let src = read("src/view/main/redraw.rs");
    let body = fn_body(&src, "fn handle_redraw(");
    assert!(
        body.contains("if !self.state.fullscreen_stage_active() {")
            && body.contains("resize_all_terminals("),
        "`handle_redraw` 의 `resize_all_terminals` 가 무대 게이트를 잃었다 — 무대 중 \
         창 크기가 바뀌면 원본 grid 가 따라가 리플로우된다."
    );
}

#[test]
fn stage_state_is_not_persisted() {
    // 무대는 휘발성이다 — 재시작이 전체화면 상태로 부팅되면 사용자가 창을 조작할 수
    // 없는 상태가 된다. 레이아웃 영속화 코드가 무대를 알면 안 된다.
    let dir: PathBuf = repo_root().join("src/core/layout_persistence");
    let mut hits = Vec::new();
    visit(&dir, &mut |p, text| {
        if text.contains("fullscreen_stage") {
            hits.push(p.display().to_string());
        }
    });
    assert!(
        hits.is_empty(),
        "레이아웃 영속화가 무대를 참조한다: {hits:?} — 무대는 영속화 대상이 아니다."
    );
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            visit(&p, f);
        } else if p.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&p) {
                f(&p, &text);
            }
        }
    }
}
