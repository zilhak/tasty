//! 팝오버 Area 를 Tab 이동 대상에서 뺀 `popup_below_widget` 판. 팝오버 키(Escape)를 읽는 이 크레이트의 유일한
//! 자리라 따로 둔다. 루트의 팝업 본문 키 가드(`popup_body_key_gate`)가 이 파일을 공용 위젯처럼 보고, 이 함수를 부르는
//! 팝업 층 파일을 짝 명부로 관리한다.

/// `egui::popup_below_widget` 과 같은 자리·틀·닫기 규칙으로 팝오버를 열되, 팝오버 Area 자체는 Tab 이동 대상에서
/// 뺀다(egui 는 interactable Area 에 `Sense::click()` 을 주어 Area 가 한 칸의 포커스를 받는다). 안의 위젯은 버튼 바로
/// 뒤의 Tab 순서에 그대로 든다. egui 판과 달리 부모 레이어의 열린 팝업 목록에는 올리지 않는다(egui 내부 상태라
/// 바깥에서 쓸 수 없다). 그 목록은 같은 레이어의 tooltip 을 숨기는 데만 쓰이므로, 열린 동안 tooltip 을 숨기려면
/// 호출부가 그 레이어 위젯에 hover text 를 붙이지 않는다(포트 창 머리). Esc 로 닫으면 포커스를 버튼으로 돌린다(egui
/// 는 Esc 에 포커스를 비운다). 메뉴 틀은 [`crate::with_popover_frame`] 클로저 안에서 불러야 입는다.
pub fn popup_below_widget_unfocused<R>(
    parent_ui: &egui::Ui,
    popup_id: egui::Id,
    widget_response: &egui::Response,
    close_behavior: egui::PopupCloseBehavior,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    if !parent_ui.memory(|mem| mem.is_popup_open(popup_id)) {
        return None;
    }
    let mut pos = widget_response.rect.left_bottom();
    if let Some(to_global) = parent_ui
        .ctx()
        .layer_transform_to_global(parent_ui.layer_id())
    {
        pos = to_global * pos;
    }
    let frame = egui::Frame::popup(parent_ui.style());
    let inner_width = (widget_response.rect.width() - frame.total_margin().sum().x).max(0.0);
    let response = egui::Area::new(popup_id)
        .kind(egui::UiKind::Popup)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .default_width(inner_width)
        .pivot(egui::Align2::LEFT_TOP)
        // 누름은 받아 앞으로 올리되 포커스는 받지 않는다.
        .sense(egui::Sense::CLICK)
        .show(parent_ui.ctx(), |ui| {
            frame
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_min_width(inner_width);
                        add_contents(ui)
                    })
                    .inner
                })
                .inner
        });
    let should_close = match close_behavior {
        egui::PopupCloseBehavior::CloseOnClick => widget_response.clicked_elsewhere(),
        egui::PopupCloseBehavior::CloseOnClickOutside => {
            widget_response.clicked_elsewhere() && response.response.clicked_elsewhere()
        }
        egui::PopupCloseBehavior::IgnoreClicks => false,
    };
    let escape = parent_ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || should_close {
        parent_ui.memory_mut(|mem| mem.close_popup());
    }
    if escape {
        widget_response.request_focus();
    }
    Some(response.inner)
}
