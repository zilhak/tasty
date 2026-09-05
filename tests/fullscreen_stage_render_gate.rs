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
//! 5. **레이아웃 영속화는 무대를 모른다.** 재시작이 전체화면 상태로 부팅되면 사용자가
//!    창을 조작할 수 없다. 이것만 순회를 끼므로 하한과 대조군이 함께 붙는다.
//!
//! 5 의 범위는 **레이아웃 영속화 모듈 하나**다. `src/app/persistence.rs` 와
//! `src/intent/preset_capture.rs` 는 여기서 안 본다 — 물음이 다르기 때문이다. preset 은
//! 사용자가 명시적으로 캡처하고 명시적으로 되살리는 것이라 "부팅이 조작 불가 상태로
//! 시작된다" 는 논거가 그대로 안 옮겨 간다. 그쪽을 재는 채널은 여기 **없다.**

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _` 무시는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

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
fn window_resize_does_not_touch_the_grid_during_a_stage() {
    let src = read("src/view/main.rs");
    let arm_head = "WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {";
    let start = only_at(&src, arm_head, "resize 이벤트 arm");
    let rest = &src[start..];
    let end = rest
        .find("\n            WindowEvent::")
        .unwrap_or(rest.len());
    let arm = &rest[..end];

    let gate = arm
        .find("if self.state.fullscreen_stage_active() {")
        .expect(
            "resize 이벤트 arm 에 무대 게이트가 없다 — 무대 중 창 크기가 바뀌면 grid 가 \
             즉시 따라가 '진입 시점 값 유지' 계약이 깨진다.",
        );
    let gpu_resize = arm
        .find("self.base.gpu.resize(new_size);")
        .expect("resize 이벤트 arm 에 gpu.resize 가 없다");
    assert!(
        gpu_resize < gate,
        "gpu.resize 가 무대 게이트 안으로 들어갔다 — GPU 서페이스 크기는 무대 여부와 \
         무관하게 창을 따라가야 한다."
    );
    // 줄바꿈 위치는 rustfmt 소관이라 needle 에 넣지 않는다.
    for after_gate in ["self.core_state.update_grid_size(", ".resize_all("] {
        let at = arm
            .find(after_gate)
            .unwrap_or_else(|| panic!("resize 이벤트 arm 에 `{after_gate}` 가 없다"));
        assert!(
            at > gate,
            "`{after_gate}` 가 무대 게이트 밖에 있다 — 무대 중에도 grid 가 재계산된다."
        );
    }
}

/// 무대는 휘발성이다 — 재시작이 전체화면 상태로 부팅되면 사용자가 창을 조작할 수 없는
/// 상태가 된다. 그래서 레이아웃 영속화 코드는 무대를 알면 안 된다.
///
/// **이 단정은 부정형이고, 부정 단정은 혼자 서면 안 된다.** "위반이 0" 과 "아무 파일도
/// 안 읽었다" 가 같은 초록이기 때문이다 — 그리고 뒤쪽이 나는 순간은 하필 모듈이 옮겨
/// 가거나 이름이 바뀐 때, 즉 위반이 새로 들어오기 가장 쉬운 때다. 그래서 인구를 먼저
/// 세고, 그 하한은 공용 순회가 자기 실패문과 함께 강제한다.
///
/// 모수는 **모듈 디렉토리와 그 모듈 파일을 합친 것**이다. 한때 디렉토리만 훑었는데,
/// 그러면 `layout_persistence.rs` 자신이 인구 밖이라 거기 들어온 참조는 영영 안 보인다.
const PERSISTENCE_FLOOR: Floor = Floor {
    min: 3,
    measured: 6,
    measured_on: "2026-09-06",
    why_this_gap: "이 모수는 레이아웃 영속화 모듈의 `.rs` 개수다. 한 모듈 안이라 크레이트 \
                   분해처럼 한꺼번에 움직이지 않고 capture/restore 를 쪼개거나 합칠 때 \
                   하나씩 움직인다 — 그래서 여유를 좁게 잡는다. 넓게 잡으면 모듈이 반쯤 \
                   사라져도 통과하고, 그 절반이 하필 참조를 품은 쪽일 수 있다.",
};

/// 영속화 모듈의 소스. 순회 루트를 `src/core` 로 잡고 접두사로 좁히는 이유는, 모듈
/// 디렉토리와 같은 이름의 모듈 파일이 **형제**라 한 루트로는 둘을 같이 못 담기 때문이다.
/// 모듈이 통째로 이름을 바꾸면 이 접두사가 아무것도 안 고르고, 그때는 하한이 빨개진다 —
/// 그것이 노리는 바다.
fn persistence_sources(root: &Path, floor: &Floor) -> Result<Vec<Walked>, String> {
    walk_with_floor(
        &root.join("src/core"),
        root,
        floor,
        Descend::SkipBuildCaches,
        &|found| found.rel.starts_with("src/core/layout_persistence") && found.rel.ends_with(".rs"),
    )
}

/// 무대를 참조하는지 판정하는 유일한 자리 — 대조군도 이것을 부른다.
fn mentions_stage(text: &str) -> bool {
    text.contains("fullscreen_stage")
}

/// 순회가 모은 파일 중 무대를 참조하는 것.
fn stage_referencing(files: &[Walked]) -> Vec<String> {
    let mut hits = Vec::new();
    for found in files {
        let text = std::fs::read_to_string(&found.path)
            .unwrap_or_else(|e| panic!("read {}: {e}", found.path.display()));
        if mentions_stage(&text) {
            hits.push(found.rel.clone());
        }
    }
    hits
}

#[test]
fn stage_state_is_not_persisted() {
    let root = repo_root();
    // 순회 실패를 삼키지 않는다. 삼키면 이 아래의 `is_empty()` 가 "위반이 없다" 가
    // 아니라 "아무것도 안 봤다" 를 뜻하게 되고, 둘은 같은 초록으로 나온다.
    let files =
        persistence_sources(&root, &PERSISTENCE_FLOOR).unwrap_or_else(|why| panic!("{why}"));
    let hits = stage_referencing(&files);
    assert!(
        hits.is_empty(),
        "레이아웃 영속화가 무대를 참조한다: {hits:?} — 무대는 영속화 대상이 아니다. \
         재시작이 전체화면 상태로 부팅되면 사용자가 창을 조작할 수 없다."
    );
}

/// 대조군: 위 판정이 **심어 둔 참조를 실제로 집는가.**
///
/// 갈래를 둘 둔다. 하나만 두면 못 가른다 — 심은 것을 집었다는 것만으로는 그 판정이
/// 아무거나 집는 것인지 알 수 없고, 안 집었다는 것만으로는 순회가 죽은 것인지 알 수
/// 없다. 두 갈래가 서로의 대조다.
#[test]
fn the_persistence_scan_reacts_to_a_planted_reference() {
    let base = std::env::temp_dir().join(format!(
        "tasty-stage-persist-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let module_dir = base.join("src/core/layout_persistence");
    std::fs::create_dir_all(&module_dir).expect("픽스처 디렉토리");

    // 실제 모수와 같은 모양: 모듈 파일 하나 + 디렉토리 안 셋.
    let plain = [
        (
            base.join("src/core/layout_persistence.rs"),
            "pub mod schema;\n",
        ),
        (module_dir.join("capture.rs"), "fn capture() {}\n"),
        (module_dir.join("restore.rs"), "fn restore() {}\n"),
        (module_dir.join("schema.rs"), "struct Snapshot;\n"),
    ];
    for (path, text) in &plain {
        std::fs::write(path, text).expect("픽스처 파일");
    }
    // 순회가 이 접두사 밖까지 긁는지도 함께 본다 — 긁으면 아래 두 수가 어긋난다.
    std::fs::write(
        base.join("src/core/state.rs"),
        "let x = fullscreen_stage_active();\n",
    )
    .expect("접두사 밖 파일");

    let probe = Floor {
        min: 2,
        measured: 4,
        measured_on: "2026-09-06",
        why_this_gap: "픽스처는 이 시험이 방금 만든 것이라 모수가 코드와 함께만 움직인다 — \
                       그래도 하한을 실측보다 낮게 두는 것은 이 자리의 물음이 파일 수가 \
                       아니라 순회가 살아 있는가이기 때문이다.",
    };

    // --- 갈래 1: 참조가 없으면 안 집는다 ---
    let clean = persistence_sources(&base, &probe).expect("픽스처 순회가 하한에 걸렸다");
    assert_eq!(
        clean.len(),
        4,
        "픽스처에서 모은 수가 다르다 — 접두사가 모듈 밖까지 집었거나 못 미쳤다: {:?}",
        clean.iter().map(|f| f.rel.clone()).collect::<Vec<_>>()
    );
    assert!(
        stage_referencing(&clean).is_empty(),
        "무대를 안 쓰는 픽스처에서 위반이 나왔다 — 판정이 아무거나 집는다"
    );

    // --- 갈래 2: 심으면 집는다 ---
    std::fs::write(
        module_dir.join("restore.rs"),
        "fn restore(s: &Snapshot) { s.fullscreen_stage; }\n",
    )
    .expect("참조 심기");
    let planted = persistence_sources(&base, &probe).expect("픽스처 순회가 하한에 걸렸다");
    assert_eq!(
        stage_referencing(&planted),
        vec!["src/core/layout_persistence/restore.rs".to_string()],
        "심어 둔 무대 참조를 판정이 못 집는다 — 그러면 본 시험의 초록은 '위반이 없다' 가 \
         아니라 '무엇도 못 집는다' 를 뜻한다"
    );

    // 판정은 이미 끝났다. `unwrap` 을 쓰면 임시 디렉토리 삭제 실패가 이 가드의 빨강으로
    // 둔갑한다.
    let _ = std::fs::remove_dir_all(&base);
}
