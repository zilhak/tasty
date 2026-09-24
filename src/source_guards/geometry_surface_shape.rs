//! debug popup·banner 응답의 rect 키와 banner 좌표계 표기를 원문에서 찾는다.
//! 셸 rect는 논리 좌표, plugin mesh 콘텐츠는 물리 좌표이므로 응답에서 구별해야 한다.
//! 헤드리스에서도 검사하려고 소스를 읽으며, 실제 JSON 응답이나 문자열의 함수 소속까지 검증하지 않는다.

const DEBUG_HANDLER: &str = "src/adapters/ipc/handler/debug.rs";

fn handler_src() -> String {
    let path = super::repo_root().join(DEBUG_HANDLER);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{DEBUG_HANDLER} 읽기 실패: {e}"))
}

#[test]
fn both_geometry_surfaces_emit_the_same_rect_keys() {
    let src = handler_src();
    let shape = r#""x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height()"#;
    let hits = src.matches(shape).count();
    assert!(
        hits >= 2,
        "popup·banner의 공통 rect 키 형식을 {hits}곳만 찾았다. x/y/w/h 형식이 유지되는지 확인한다."
    );
}

#[test]
fn the_banner_surface_states_its_coordinate_systems() {
    let src = handler_src();
    assert!(
        src.contains(r#""coords": { "rect": "logical", "content_rect": "physical" }"#),
        "banner 응답의 좌표계 표기를 찾지 못했다. 셸 rect는 논리 좌표, 콘텐츠 rect는 물리 좌표임을 표시해야 한다."
    );
}

#[test]
fn the_banner_surface_exposes_geometry_at_all() {
    let src = handler_src();
    for key in [r#""rect": rect"#, r#""content_rect": content_rect"#] {
        assert!(
            src.contains(key),
            "banner 응답에서 {key}를 찾지 못했다. 좌표로 배치를 검사할 수 있도록 rect와 content_rect를 유지한다."
        );
    }
}
