//! 기하를 내주는 debug 관측면 둘이 **같은 모양**으로 내주는가.
//!
//! `debug.host_popup.list` 와 `debug.banner.list` 는 같은 파일에 나란히 있고 둘 다
//! 사각형을 낸다. 그 둘의 키 이름이 갈리면 잃는 것은 미관이 아니라 **검증 스크립트가
//! 두 벌이 되는 것**이다 — 배너를 재는 절차와 popup 을 재는 절차가 각각 다른 파서를
//! 갖게 되고, 그 둘은 따로 늙는다.
//!
//! 배너 쪽은 실제로 그 갈림의 극단이었다: 좌표를 **하나도** 안 내줘서 배너의 배치·정렬·
//! 간격을 재는 모든 검증이 스크린샷 픽셀 판정으로 시작해야 했다.
//!
//! ## 무엇을 보는가
//!
//! 1. 두 핸들러가 사각형을 낼 때 쓰는 키가 `x`/`y`/`w`/`h` 로 같은가.
//! 2. 배너 응답이 **좌표계를 스스로 말하는가**(`coords`). 두 rect 의 좌표계가 서로
//!    다르므로(셸은 논리, plugin mesh 콘텐츠는 물리) 이건 주석에만 두면 반드시 틀린다 —
//!    이 레포의 길이 타입 정책이 있는 이유와 같다(`docs/concepts/typed-length.md`).
//!
//! 텍스트로 읽는다. 두 값 모두 함수 본문 속 `json!` 리터럴이라 값으로 존재하지 않고,
//! 핸들러는 `#[cfg(all(debug_assertions, feature = "gui"))]` 로 게이트돼 있어 링크로는
//! 헤드리스 조합에서 사라진다.

const DEBUG_HANDLER: &str = "src/adapters/ipc/handler/debug.rs";

fn handler_src() -> String {
    let path = super::repo_root().join(DEBUG_HANDLER);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{DEBUG_HANDLER} 읽기 실패: {e}"))
}

/// 두 관측면이 사각형을 같은 키로 낸다.
#[test]
fn both_geometry_surfaces_emit_the_same_rect_keys() {
    let src = handler_src();
    let shape = r#""x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height()"#;
    let hits = src.matches(shape).count();
    assert!(
        hits >= 2,
        "popup 과 banner 가 같은 rect 키 모양을 쓰는 자리를 {hits} 곳만 찾았다 — \
         모양이 갈리면 두 표면을 재는 스크립트가 두 벌이 된다"
    );
}

/// 배너 응답은 자기 좌표계를 스스로 말한다.
#[test]
fn the_banner_surface_states_its_coordinate_systems() {
    let src = handler_src();
    assert!(
        src.contains(r#""coords": { "rect": "logical", "content_rect": "physical" }"#),
        "banner.list 응답에 좌표계 선언이 없다. 두 rect 의 좌표계가 서로 다르므로\n\
         (셸=논리, plugin mesh 콘텐츠=물리) 선언이 빠지면 호출부가 둘을 같은 자로 읽는다"
    );
}

/// 배너 shown 항목이 기하를 낸다 — 이 축이 닫은 구멍 자체.
#[test]
fn the_banner_surface_exposes_geometry_at_all() {
    let src = handler_src();
    for key in [r#""rect": rect"#, r#""content_rect": content_rect"#] {
        assert!(
            src.contains(key),
            "banner.list 의 shown 항목에서 {key} 가 사라졌다 — 배너를 재는 채널이 \
             다시 스크린샷 픽셀 하나뿐이 된다"
        );
    }
}
