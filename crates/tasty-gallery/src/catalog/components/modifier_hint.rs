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

/// **Ctrl 홀드** 패널 — normal 행 · plugin 행(agent dot) · hash role(탭 전환) 을 노출.
/// 마우스 캡처 우회 role 은 **Shift 단독** 에만 붙으므로 여기엔 없다(→ `SHIFT_SECTIONS`).
const CTRL_SECTIONS: &[Section] = &[
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
        roles: &[],
    },
];

/// **Shift 홀드** 패널 — mouse role 은 Shift **단독** 섹션에만. Ctrl+Shift 는 우회 role 없이
/// 바인딩 행만(우회 실 동작은 조합에도 걸리지만 안내 행은 단독 섹션에만 표시).
const SHIFT_SECTIONS: &[Section] = &[
    Section {
        chord: "Shift",
        rows: &[],
        roles: &[(
            "Bypass TUI mouse capture (Shift+drag to select)",
            RoleGlyph::Mouse,
        )],
    },
    Section {
        chord: "Ctrl+Shift",
        rows: &[("Reopen closed tab", "Ctrl+Shift+T", false)],
        roles: &[],
    },
];

/// **혼재** — Ctrl 홀드 시 채워진 섹션과 빈(플레이스홀더) 섹션이 한 리스트에 공존
/// (디자인 §2·3). Ctrl(role+rows) · Ctrl+Alt(EMPTY) · Ctrl+Shift(rows) · Ctrl+Alt+Shift(EMPTY).
/// 빈 섹션 = `rows`·`roles` 모두 빈 배열 → `section_list` 가 플레이스홀더로 렌더.
const MIXED_SECTIONS: &[Section] = &[
    Section {
        chord: "Ctrl",
        rows: &[
            ("Command palette", "Ctrl+K", false),
            ("New tab", "Ctrl+T", false),
            ("Close tab", "Ctrl+W", false),
        ],
        roles: &[("Switch tabs (number keys show over tabs)", RoleGlyph::Hash)],
    },
    Section {
        chord: "Ctrl+Alt",
        rows: &[],
        roles: &[],
    },
    Section {
        chord: "Ctrl+Shift",
        rows: &[
            ("New workspace", "Ctrl+Shift+N", false),
            ("Split horizontal", "Ctrl+Shift+D", false),
            ("git-helper: Stage hunk", "Ctrl+Shift+G", true),
        ],
        roles: &[],
    },
    Section {
        chord: "Ctrl+Alt+Shift",
        rows: &[],
        roles: &[],
    },
];

/// **전체 빈 패널** — Ctrl+Alt 홀드 시 상위 조합이 모두 미할당(디자인 §2·2). 두 섹션 모두
/// 플레이스홀더만. 미할당 조합 홀드 시 패널이 뜨고 "바인딩 없음"으로 부재를 명시(ADR-0038).
const EMPTY_SECTIONS: &[Section] = &[
    Section {
        chord: "Ctrl+Alt",
        rows: &[],
        roles: &[],
    },
    Section {
        chord: "Ctrl+Alt+Shift",
        rows: &[],
        roles: &[],
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::spec(
        ui,
        theme,
        "Modifier hint panel",
        Some(
            "Two holds shown · only combos containing the held keys · ordered by combo size then Ctrl < Alt < Shift",
        ),
    );
    spec::stage(ui, theme, StageVariant::Center, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            // 각 패널을 고유 id scope 로 감싼다 — panel() 내부 ScrollArea 가 auto-id 를
            // 쓰는데 4 개가 같은 소스 위치라 held 가 같은 두 "Ctrl" 패널을 포함해 id 가
            // 충돌했다(egui 가 "ScrollArea ID … 중복" 마커를 그리고, 공유된 스크롤 상태로
            // 패널들이 겹쳐 그려졌다). push_id 로 내부 auto-id 전체를 인스턴스별로 분기한다.
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                ui.push_id("mh_ctrl", |ui| panel(ui, theme, "Ctrl", CTRL_SECTIONS));
                ui.push_id("mh_shift", |ui| panel(ui, theme, "Shift", SHIFT_SECTIONS));
            });
            // 빈 조합 플레이스홀더(ADR-0038) — 혼재(채워진+빈) · 전체 빈 패널.
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                ui.push_id("mh_ctrl_mixed", |ui| {
                    panel(ui, theme, "Ctrl", MIXED_SECTIONS)
                });
                ui.push_id("mh_ctrl_alt", |ui| {
                    panel(ui, theme, "Ctrl+Alt", EMPTY_SECTIONS)
                });
            });
        });
    });

    let held = theme.modhint_hold_delay().to_millis_f32() as i64;
    let fade = theme.modhint_fade().to_millis_f32() as i64;
    spec::meta(
        ui,
        theme,
        &[
            ("width", "modhint-width 180 (min 180)"),
            ("height", "modhint-height 400 (min 240)"),
            ("strip", "modhint-strip-height 28 · bg-sidebar"),
            (
                "reveal",
                "hold 500ms (Shift-only 1200ms) → fade 200ms (opacity 0.2→1.0)",
            ),
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
                "modhint-empty-fg",
                "empty placeholder text",
                theme.modhint_empty_fg().to_egui(),
            ),
            TokenChip::new(
                "modhint-agent-dot",
                "plugin row dot",
                theme.modhint_agent_dot().to_egui(),
            ),
        ],
    );

    // 아래 설명 문구가 아직 값을 문자열에 박아 두고 있어 theme 에서 읽은 두
    // 타이밍 값은 이 specimen 에서 쓰이지 않는다 — 미사용 경고만 억제한다.
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
        "Empty combos (no binding and no role) keep their section and show one muted \"No shortcuts bound\" \
         placeholder — so holding an all-empty combo surfaces the panel instead of dead silence (ADR-0038).",
    );
    spec::dont(
        ui,
        theme,
        "Don't dress the placeholder like a real row: no keycap (implies a binding), no washed background \
         (reads as a role-row), no leading glyph (reads as a bullet). It's text-muted only — quieter than \
         every real row so it never competes with an actual binding.",
    );
    spec::note(
        ui,
        theme,
        "Row keycaps show the leaf key only (C / G / T) — the chord head above owns the modifier, so \
         Ctrl+C would repeat it. Section headers keep the full combo (Ctrl / Ctrl+Alt / Ctrl+Shift).",
    );
    spec::note(
        ui,
        theme,
        "The list narrows to the held combo: pressing Ctrl then adding Shift drops the bare-Ctrl \
         section and shows only combos containing Ctrl+Shift. Shift-only holds wait 1200ms (not 500ms) \
         to avoid popping up mid-typing. The mouse-capture-bypass role is listed under the bare Shift \
         section only — it never repeats under Ctrl+Shift and other Shift combos.",
    );
    spec::dont(
        ui,
        theme,
        "Don't gate the 500ms hold delay on reduced-motion — the delay is an intent gate, not motion. \
         Only the 200ms fade is dropped under reduced-motion.",
    );
}

/// 180×400 패널 셸 + 드래그 스트립 + 섹션 리스트 + 코너 그립.
fn panel(ui: &mut egui::Ui, theme: &Theme, held: &str, sections: &[Section]) {
    let w = theme.modhint_width().value();
    let h = theme.modhint_height().value();
    let bw = theme.border_width.value();

    let frame = egui::Frame::new()
        .fill(theme.modhint_bg().to_egui())
        .stroke(egui::Stroke::new(bw, theme.modhint_border().to_egui()))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui());

    let resp = frame.show(ui, |ui| {
        // frame 이 horizontal_top 안에 있어 inner ui 가 horizontal 레이아웃을 상속한다 —
        // 명시적으로 세로 적층으로 바꾸지 않으면 drag_strip 과 아래 ScrollArea 가 가로로
        // 배치돼(ScrollArea 가 strip 오른쪽 x+w 에서 시작) 리스트 본문이 패널 밖에 그려졌다.
        ui.vertical(|ui| {
            ui.set_width(w);
            ui.set_height(h);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            drag_strip(ui, theme, w, held);
            // 스크롤 리스트 (남은 높이). specimen 은 정적이라 세로 오버플로가 없지만
            // 본체와 동일하게 ScrollArea 로 감싸 스크롤 어포던스를 재현한다.
            let list_h = h - theme.modhint_strip_height().value() - bw;
            egui::ScrollArea::vertical()
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section_list(ui, theme, w, sections);
                });
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
fn drag_strip(ui: &mut egui::Ui, theme: &Theme, w: f32, held: &str) {
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
        kbd(ui, theme, held);
        ui.label(
            egui::RichText::new("held")
                .size(theme.font_size_caption.value())
                .color(theme.modhint_held_fg().to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // specimen 은 상태가 없다 — 클릭 응답을 받아 처리할 곳이 없다.
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
fn section_list(ui: &mut egui::Ui, theme: &Theme, w: f32, sections: &[Section]) {
    let pad = theme.modhint_pad().value();
    let inner_w = w - pad * 2.0;
    // 패딩(pad)만큼 안쪽에서 시작하는 inner_w 폭 컬럼. 예전엔 `ui.min_rect().min` 기준
    // 절대 좌표로 child 를 잡았는데, panel 이 horizontal_top 안의 auto_shrink 무한폭
    // ScrollArea 라 그 기준점이 상자 밖(우측)으로 어긋나 리스트 본문이 패널 밖에 그려졌다.
    // 좌/상 패딩은 add_space 로, 폭은 allocate_ui 로 고정해 흐름 기준으로 배치한다.
    ui.add_space(pad);
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.allocate_ui_with_layout(
            egui::vec2(inner_w, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = theme.modhint_section_gap().value();
                section_body(ui, theme, sections);
            },
        );
    });
}

fn section_body(ui: &mut egui::Ui, theme: &Theme, sections: &[Section]) {
    ui.vertical(|ui| {
        for sec in sections {
            chord_head(ui, theme, sec.chord);
            // 빈 조합(바인딩·역할 모두 없음)은 내부 간격을 3px(채워진 6px)로 좁혀 "바인딩
            // 없음" 플레이스홀더 한 줄만 그린다(ADR-0038, 본체 draw_section 전사).
            let is_empty = sec.rows.is_empty() && sec.roles.is_empty();
            let content_gap = if is_empty {
                theme.modhint_empty_row_gap().value()
            } else {
                theme.modhint_row_gap().value()
            };
            ui.spacing_mut().item_spacing.y = content_gap;
            if is_empty {
                empty_row(ui, theme);
            } else {
                for (label, binding, plugin) in sec.rows {
                    // 행 키캡은 leaf 키만 — chord head 가 이미 modifier 를 담당한다. 디자인 SoT
                    // `overlays-shared.jsx` 의 `r.keys.startsWith(s.keys+"+") ? r.keys.slice(...)`
                    // 를 1:1 전사: `SECTIONS` mock 은 full chord 를 유지하고 렌더에서만 접두를 뗀다.
                    let prefix = format!("{}+", sec.chord);
                    let leaf = binding.strip_prefix(&prefix).unwrap_or(binding);
                    hint_row(ui, theme, label, leaf, *plugin);
                }
                for (desc, glyph) in sec.roles {
                    role_row(ui, theme, desc, *glyph);
                }
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

/// 액션 행 — (plugin 이면 agent dot) + 라벨(wrap) + 우측 Kbd.
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            kbd(ui, theme, binding);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(theme.font_size_body.value())
                            .color(theme.modhint_row_fg().to_egui()),
                    )
                    .wrap(),
                );
            });
        });
    });
}

/// 빈 조합 플레이스홀더 행 — muted 텍스트("바인딩 없음"). 부재 신호라 리스트에서 가장
/// 조용하다: 키캡 없음 · wash 없음 · leading 글리프 없음 · 정적(호버/포커스 없음). 본체
/// `modifier_hint_overlay.rs draw_empty_row` 전사. 갤러리는 mock 이라 문구를 하드코딩
/// (본체는 `t("modifier_hint.empty")`).
fn empty_row(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        // 키캡 행(24px)보다 타이트한 20px 최소 높이(디자인 §6-5).
        ui.set_min_height(theme.modhint_empty_row_min_height().value());
        ui.add(
            egui::Label::new(
                egui::RichText::new("No shortcuts bound")
                    .size(theme.font_size_body.value())
                    .color(theme.modhint_empty_fg().to_egui()),
            )
            .selectable(false),
        );
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
            ui.set_width(ui.available_width());
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
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(desc)
                            .size(theme.font_size_body.value())
                            .color(theme.modhint_row_fg().to_egui()),
                    )
                    .wrap(),
                );
            });
        });
}

fn paint_glyph(ui: &mut egui::Ui, glyph: MockGlyph, rect: egui::Rect, color: egui::Color32) {
    glyph.image(rect.height(), color).paint_at(ui, rect);
}
