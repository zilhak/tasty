//! `modifier-hint` specimen — Modifier hint 오버레이 (디자인 `gallery/overlays-popups.jsx`
//! "Modifier hints" 섹션, `overlays-shared.jsx:1435-1545` 의 `ModifierHintPanelG`).
//!
//! modifier(Ctrl/Alt/Shift, macOS 는 Cmd/Option 추가)를 **누르고 있는 동안** 500ms 홀드
//! 뒤 사이드바 하단에 뜨는 안내 패널. 4분류(Popup/Toast/Banner/Modal) 밖의 신규 요소 —
//! **키보드 포커스 없음 + 마우스 인터랙티브(드래그/리사이즈/X) + 홀드 수명**. 릴리즈 즉시
//! 소멸. 본체는 `src/adapters/ui/modifier_hint_overlay.rs`.
//!
//! specimen 은 갤러리가 본체 binary 에 의존할 수 없어 시각 layout 을 **복제**하되, 색·치수는
//! 전부 `Theme` 의 `modhint_*()` 접근자(=본체와 동일 토큰)에서 가져온다. 홀드 라이프사이클은
//! 정적으로 재현할 수 없으므로 "revealed(표시 완료)" 상태 한 장을 그리고 수명주기는 note 로
//! 설명한다. 섹션 정렬 = 조합 크기 오름차순 → `Ctrl < Alt < Shift`(본체 modifier-hint-02).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, kbd};

use crate::catalog::icons::{MOUSE, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 한 조합 섹션의 mock 데이터.
struct Section {
    /// 조합 헤더 키캡 (예: `"Ctrl"`, `"Ctrl+Shift"`).
    chord: &'static str,
    /// (액션 라벨, 바인딩 키캡, plugin 여부) 행.
    rows: &'static [(&'static str, &'static str, bool)],
    /// (역할 설명, leading 글리프) 특수 역할 행.
    roles: &'static [(&'static str, RoleGlyph)],
}

/// 역할 행 leading 글리프 — 탭/워크스페이스 숫자(#) 또는 마우스 캡처(mouse icon).
#[derive(Clone, Copy)]
enum RoleGlyph {
    /// `mhIc.hash` — 탭/워크스페이스 전환(숫자 오버레이).
    Hash,
    /// `mhIc.mouse` — TUI 마우스 캡처 우회.
    Mouse,
}

/// Ctrl 홀드 예시 — normal 행 · plugin 행(agent dot) · role 행(# / mouse) 을 모두 노출.
const SECTIONS: &[Section] = &[
    Section {
        chord: "Ctrl",
        rows: &[("Copy", "Ctrl+C", false)],
        roles: &[("Switch tabs (number keys show over tabs)", RoleGlyph::Hash)],
    },
    Section {
        chord: "Ctrl+Alt",
        rows: &[("git-helper: Stage hunk", "Ctrl+Alt+G", true)],
        roles: &[],
    },
    Section {
        chord: "Ctrl+Shift",
        rows: &[("Reopen closed tab", "Ctrl+Shift+T", false)],
        roles: &[(
            "Bypass TUI mouse capture (Shift+drag to select)",
            RoleGlyph::Mouse,
        )],
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::spec(
        ui,
        theme,
        "Modifier hint panel",
        Some("Ctrl held · sections ordered by combo size then Ctrl < Alt < Shift"),
    );
    spec::stage(ui, theme, StageVariant::Center, |ui| {
        panel(ui, theme);
    });

    let held = theme.motion_hold_reveal_ms() as i64;
    let fade = theme.motion_ui_fade_ms() as i64;
    spec::meta(
        ui,
        theme,
        &[
            ("width", "modhint-width 220 (min 200)"),
            ("height", "modhint-height 400 (min 240)"),
            ("strip", "modhint-strip-height 28 · bg-sidebar"),
            ("reveal", "hold 500ms → fade 200ms (opacity 0.2→1.0)"),
            ("release", "0ms — vanishes immediately"),
        ],
        &[
            TokenChip::new(
                "modhint-bg",
                "panel fill (opaque)",
                theme.modhint_bg().to_egui(),
            ),
            TokenChip::new(
                "modhint-border",
                "1px shell",
                theme.modhint_border().to_egui(),
            ),
            TokenChip::new(
                "modhint-strip-bg",
                "drag strip",
                theme.modhint_strip_bg().to_egui(),
            ),
            TokenChip::new(
                "modhint-role-bg",
                "role row washed",
                theme.modhint_role_bg().to_egui(),
            ),
            TokenChip::new(
                "modhint-role-fg",
                "role glyph",
                theme.modhint_role_fg().to_egui(),
            ),
            TokenChip::new(
                "modhint-agent-dot",
                "plugin row dot",
                theme.modhint_agent_dot().to_egui(),
            ),
        ],
    );

    let _ = (held, fade);
    spec::note(
        ui,
        theme,
        "Keyboard focus is never taken — typing flows to the terminal while the panel is up. \
         Only the mouse is consumed (drag to move, borders/corner grip to resize, X to dismiss).",
    );
    spec::do_(
        ui,
        theme,
        "Empty combos (no binding and no role) omit the whole section — never render an \"empty\" row.",
    );
    spec::dont(
        ui,
        theme,
        "Don't gate the 500ms hold delay on reduced-motion — the delay is an intent gate, not motion. \
         Only the 200ms fade is dropped under reduced-motion.",
    );
}

/// 220×400 패널 셸 + 드래그 스트립 + 섹션 리스트 + 코너 그립.
fn panel(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.modhint_width().value();
    let h = theme.modhint_height().value();
    let bw = theme.border_width.value();

    let frame = egui::Frame::new()
        .fill(theme.modhint_bg().to_egui())
        .stroke(egui::Stroke::new(bw, theme.modhint_border().to_egui()))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui());

    let resp = frame.show(ui, |ui| {
        ui.set_width(w);
        ui.set_height(h);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        drag_strip(ui, theme, w);
        // 스크롤 리스트 (남은 높이). specimen 은 정적이라 세로 오버플로가 없지만
        // 본체와 동일하게 ScrollArea 로 감싸 스크롤 어포던스를 재현한다.
        let list_h = h - theme.modhint_strip_height().value() - bw;
        egui::ScrollArea::vertical()
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                section_list(ui, theme, w);
            });
    });

    // 우하단 코너 그립 — 대각선 2획 (nwse-resize). right:2 bottom:2, 12×12.
    let rect = resp.response.rect;
    let g = theme.modhint_grip_size().value();
    let pad = bw * 2.0;
    let br = egui::pos2(rect.right() - pad, rect.bottom() - pad);
    let col: egui::Color32 = theme.text_muted().to_egui();
    let stroke = egui::Stroke::new(bw, col);
    // 두 개의 짧은 대각선 (바깥 긴 획 + 안쪽 짧은 획).
    ui.painter().line_segment(
        [egui::pos2(br.x - g, br.y), egui::pos2(br.x, br.y - g)],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(br.x - g * 0.5, br.y),
            egui::pos2(br.x, br.y - g * 0.5),
        ],
        stroke,
    );
}

/// 드래그 스트립 — held 조합 Kbd + "held" 라벨 + 우측 X. bg-sidebar, 하단 separator, cursor:move.
fn drag_strip(ui: &mut egui::Ui, theme: &Theme, w: f32) {
    let strip_h = theme.modhint_strip_height().value();
    let bw = theme.border_width.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, strip_h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.modhint_strip_bg().to_egui());
    // 하단 1px separator.
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - bw * 0.5,
        egui::Stroke::new(bw, theme.modhint_separator().to_egui()),
    );

    let pad_l = theme.modhint_pad().value();
    let pad_r = theme.spacing_xs.value();
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_l, rect.top()),
        egui::pos2(rect.right() - pad_r, rect.bottom()),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        kbd(ui, theme, "Ctrl");
        ui.label(
            egui::RichText::new("held")
                .size(theme.font_size_caption.value())
                .color(theme.modhint_held_fg().to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .show(ui, theme, &|ui, r, c| {
                    crate::catalog::icons::CLOSE
                        .image(r.height(), c)
                        .paint_at(ui, r);
                });
        });
    });
}

/// 섹션 리스트 — 각 섹션 = ChordHead + HintRow* + RoleRow*.
fn section_list(ui: &mut egui::Ui, theme: &Theme, w: f32) {
    let pad = theme.modhint_pad().value();
    let inner_w = w - pad * 2.0;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
        ui.min_rect().min + egui::vec2(pad, pad),
        egui::vec2(inner_w, ui.available_height()),
    )));
    child.spacing_mut().item_spacing.y = theme.modhint_section_gap().value();
    child.vertical(|ui| {
        for sec in SECTIONS {
            chord_head(ui, theme, sec.chord);
            ui.spacing_mut().item_spacing.y = theme.modhint_row_gap().value();
            for (label, binding, plugin) in sec.rows {
                hint_row(ui, theme, label, binding, *plugin);
            }
            for (desc, glyph) in sec.roles {
                role_row(ui, theme, desc, *glyph);
            }
            ui.spacing_mut().item_spacing.y = theme.modhint_section_gap().value();
        }
    });
}

/// 조합 헤더 — Kbd 키캡 + 하단 separator.
fn chord_head(ui: &mut egui::Ui, theme: &Theme, chord: &str) {
    ui.add_space(theme.modhint_row_gap().value());
    kbd(ui, theme, chord);
    ui.add_space(theme.modhint_row_gap().value());
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.modhint_separator().to_egui(),
        ),
    );
}

/// 액션 행 — (plugin 이면 agent dot) + 라벨(ellipsis) + 우측 Kbd.
fn hint_row(ui: &mut egui::Ui, theme: &Theme, label: &str, binding: &str, plugin: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        if plugin {
            // agent dot — status-dot-size, accent-agent (modhint-agent-dot).
            let d = theme.status_dot_size().value();
            let (r, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), d * 0.5, theme.modhint_agent_dot().to_egui());
        }
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_body.value())
                .color(theme.modhint_row_fg().to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            kbd(ui, theme, binding);
        });
    });
}

/// 특수 역할 행 — washed 배경 + leading 글리프(role-fg) + 설명.
fn role_row(ui: &mut egui::Ui, theme: &Theme, desc: &str, glyph: RoleGlyph) {
    egui::Frame::new()
        .fill(theme.modhint_role_bg().to_egui())
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_sm.value() as i8,
            theme.modhint_row_gap().value() as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                let gsz = theme.icon_glyph_size_xs.value();
                let (r, _) = ui.allocate_exact_size(egui::vec2(gsz, gsz), egui::Sense::hover());
                let col: egui::Color32 = theme.modhint_role_fg().to_egui();
                match glyph {
                    RoleGlyph::Hash => {
                        // mhIc.hash — 숫자/# 글리프. mock glyph 부재라 monospace "#".
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "#",
                            egui::FontId::monospace(gsz),
                            col,
                        );
                    }
                    RoleGlyph::Mouse => {
                        paint_glyph(ui, MOUSE, r, col);
                    }
                }
                ui.label(
                    egui::RichText::new(desc)
                        .size(theme.font_size_body.value())
                        .color(theme.modhint_row_fg().to_egui()),
                );
            });
        });
}

fn paint_glyph(ui: &mut egui::Ui, glyph: MockGlyph, rect: egui::Rect, color: egui::Color32) {
    glyph.image(rect.height(), color).paint_at(ui, rect);
}
