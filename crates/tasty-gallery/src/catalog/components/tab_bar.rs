//! Pane tab bar 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/tab_bar.rs::draw_pane_tab_bars_view` 의 시각을 mock props
//! 로 재현. 갤러리가 본체 binary 에 의존할 수 없어 view/struct 를 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 대표 상태:
//! 1. 단일 pane, 단일 탭 — 기본 외형
//! 2. 단일 pane, 5 탭 — 활성 탭 강조 + dirty/busy 혼합
//! 3. 매우 긴 탭 이름 — 말줄임 처리
//! 4. 단일 pane, 12 탭 — 스크롤 화살표
//! 5. 다수 pane (4 분할) — pane focus 강조 차이
//!
//! Drag overlay 는 본체 view 에서 `ctx.layer_painter(Tooltip)` 로 그리므로 데모
//! 안에서는 별도 표현 없이 정적 상태만 보여준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::PhysicalPx;
use tasty_type_geometry::rect::PhysicalRect;

const BAR_H: f32 = 24.0;
const PLUS_W: f32 = 28.0;
const ARROW_W: f32 = 20.0;
const SEPARATOR_W: f32 = 1.0;
const H_PADDING: f32 = 8.0;
const DOT_RADIUS: f32 = 3.0;
const DOT_PAD: f32 = 6.0;
const ACTIVE_INDICATOR_H: f32 = 2.0;

#[derive(Clone, Debug)]
struct TabEntryView {
    name: String,
    has_notification: bool,
    is_busy: bool,
    is_agent_created: bool,
}

#[derive(Clone, Debug)]
struct PaneTabBarView {
    pane_id: u32,
    /// 데모는 logical 좌표만 사용 (scale_factor=1.0). x/y/width 는 ui 내부 local 좌표.
    rect: PhysicalRect,
    tabs: Vec<TabEntryView>,
    active_tab: usize,
    is_focused: bool,
    scroll_offset: f32,
}

struct PaneTabBarsProps<'a> {
    theme: &'a Theme,
    panes: &'a [PaneTabBarView],
    tab_width: f32,
    tab_font_size: f32,
}

/// 본체 `draw_pane_tab_bars_view` 와 동등한 시각. 데모는 좌표를 ui-local 로
/// 그리므로 `egui::Area` 대신 현재 `ui` 에 직접 알로케이트.
fn draw_pane_tab_bar_mock(
    ui: &mut egui::Ui,
    theme: &Theme,
    info: &PaneTabBarView,
    tab_w: f32,
    label_font_size: f32,
) {
    let th = theme;
    let plus_font_size = th.font_size_body.value();
    let arrow_font_size = th.font_size_caption.value();
    let logical_w = info.rect.width.value();

    let n = info.tabs.len();
    let content_w = n as f32 * tab_w + (n.max(1) - 1) as f32 * SEPARATOR_W + SEPARATOR_W + PLUS_W;
    let needs_scroll = content_w > logical_w;
    let viewport_w = if needs_scroll {
        (logical_w - ARROW_W * 2.0).max(0.0)
    } else {
        logical_w.max(0.0)
    };
    let max_scroll = (content_w - viewport_w).max(0.0);
    let scroll = info.scroll_offset.clamp(0.0, max_scroll);

    let bg: egui::Color32 = if info.is_focused {
        th.surface0.into()
    } else {
        th.mantle.into()
    };

    let (frame_rect, _) =
        ui.allocate_exact_size(egui::vec2(logical_w, BAR_H), egui::Sense::hover());
    ui.painter().rect_filled(frame_rect, 0.0, bg);

    let mut x = frame_rect.min.x;
    let painter = ui.painter().with_clip_rect(frame_rect);

    // Left arrow
    if needs_scroll {
        let arrow_rect =
            egui::Rect::from_min_size(egui::pos2(x, frame_rect.min.y), egui::vec2(ARROW_W, BAR_H));
        let arrow_color: egui::Color32 = if scroll > 0.0 {
            th.subtext0.into()
        } else {
            th.surface1.into()
        };
        painter.text(
            arrow_rect.center(),
            egui::Align2::CENTER_CENTER,
            "<",
            egui::FontId::proportional(arrow_font_size),
            arrow_color,
        );
        x += ARROW_W;
    }

    let clip_rect = egui::Rect::from_min_size(
        egui::pos2(x, frame_rect.min.y),
        egui::vec2(viewport_w, BAR_H),
    );
    let tab_painter = painter.with_clip_rect(clip_rect);
    let mut tx = clip_rect.min.x - scroll;

    for (i, tab) in info.tabs.iter().enumerate() {
        if i > 0 {
            let sep = egui::Rect::from_min_size(
                egui::pos2(tx, clip_rect.min.y),
                egui::vec2(SEPARATOR_W, BAR_H),
            );
            tab_painter.rect_filled(sep, 0.0, th.surface1);
            tx += SEPARATOR_W;
        }

        let is_active = i == info.active_tab;
        let tab_bg: egui::Color32 = if is_active { th.base.into() } else { bg };
        let text_color: egui::Color32 = if is_active {
            th.text.into()
        } else if tab.has_notification {
            th.yellow.into()
        } else {
            th.subtext0.into()
        };

        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(tx, clip_rect.min.y), egui::vec2(tab_w, BAR_H));

        tab_painter.rect_filled(tab_rect, 0.0, tab_bg);

        if is_active {
            let line_rect = egui::Rect::from_min_size(
                egui::pos2(tab_rect.min.x, tab_rect.min.y),
                egui::vec2(tab_w, ACTIVE_INDICATOR_H),
            );
            tab_painter.rect_filled(line_rect, 0.0, th.blue);
        }

        if tab.is_busy {
            let dot_center = egui::pos2(tab_rect.max.x - DOT_PAD - DOT_RADIUS, tab_rect.center().y);
            let dot_color: egui::Color32 = if is_active {
                th.green.into()
            } else {
                th.green.with_alpha(180).to_egui()
            };
            tab_painter.circle_filled(dot_center, DOT_RADIUS, dot_color);
        }

        // agent(IPC/CLI) 생성 surface → mauve dot. busy 와 겹치면 그 왼쪽 슬롯.
        if tab.is_agent_created {
            let base_x = tab_rect.max.x - DOT_PAD - DOT_RADIUS;
            let agent_x = if tab.is_busy {
                base_x - DOT_RADIUS * 2.0 - DOT_PAD
            } else {
                base_x
            };
            let dot_color: egui::Color32 = if is_active {
                th.mauve.into()
            } else {
                th.mauve.with_alpha(180).to_egui()
            };
            tab_painter.circle_filled(
                egui::pos2(agent_x, tab_rect.center().y),
                DOT_RADIUS,
                dot_color,
            );
        }

        // 라벨 (잘림 처리 — 본체와 동일 알고리즘)
        let font_id = egui::FontId::proportional(label_font_size);
        let available_w = tab_w - H_PADDING * 2.0;
        let galley = tab_painter.layout_no_wrap(tab.name.clone(), font_id.clone(), text_color);
        if galley.size().x > available_w {
            let mut truncated = tab.name.clone();
            while !truncated.is_empty() {
                truncated.pop();
                let candidate = format!("{truncated}…");
                let g = tab_painter.layout_no_wrap(candidate.clone(), font_id.clone(), text_color);
                if g.size().x <= available_w {
                    let text_x = tab_rect.min.x + H_PADDING;
                    let text_y = tab_rect.center().y - g.size().y / 2.0;
                    tab_painter.galley(egui::pos2(text_x, text_y), g, text_color);
                    break;
                }
            }
        } else {
            let text_pos = tab_rect.center() - galley.size() / 2.0;
            tab_painter.galley(text_pos, galley, text_color);
        }

        tx += tab_w;
    }

    // Separator + "+"
    let sep = egui::Rect::from_min_size(
        egui::pos2(tx, clip_rect.min.y),
        egui::vec2(SEPARATOR_W, BAR_H),
    );
    tab_painter.rect_filled(sep, 0.0, th.surface1);
    tx += SEPARATOR_W;
    let plus_rect =
        egui::Rect::from_min_size(egui::pos2(tx, clip_rect.min.y), egui::vec2(PLUS_W, BAR_H));
    tab_painter.text(
        plus_rect.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        egui::FontId::proportional(plus_font_size),
        egui::Color32::from(th.subtext0),
    );

    // Right arrow
    if needs_scroll {
        let arrow_rect = egui::Rect::from_min_size(
            egui::pos2(frame_rect.max.x - ARROW_W, frame_rect.min.y),
            egui::vec2(ARROW_W, BAR_H),
        );
        let arrow_color: egui::Color32 = if scroll < max_scroll {
            th.subtext0.into()
        } else {
            th.surface1.into()
        };
        painter.text(
            arrow_rect.center(),
            egui::Align2::CENTER_CENTER,
            ">",
            egui::FontId::proportional(arrow_font_size),
            arrow_color,
        );
    }

    // pane_id 라벨 (디버그용 — 갤러리에서 다중 pane 구분).
    let label_pos = egui::pos2(frame_rect.min.x + 2.0, frame_rect.max.y + 2.0);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        format!("pane {}", info.pane_id),
        egui::FontId::proportional(10.0),
        egui::Color32::from(th.overlay0),
    );
}

fn draw_pane_tab_bars_mock(ui: &mut egui::Ui, props: &PaneTabBarsProps<'_>) {
    for info in props.panes {
        draw_pane_tab_bar_mock(ui, props.theme, info, props.tab_width, props.tab_font_size);
        ui.add_space(20.0);
    }
}

fn mk_pane(
    pane_id: u32,
    rect_w: f32,
    tabs: Vec<TabEntryView>,
    active: usize,
    focused: bool,
    scroll: f32,
) -> PaneTabBarView {
    PaneTabBarView {
        pane_id,
        rect: PhysicalRect {
            x: PhysicalPx(0.0),
            y: PhysicalPx(0.0),
            width: PhysicalPx(rect_w),
            height: PhysicalPx(BAR_H),
        },
        tabs,
        active_tab: active,
        is_focused: focused,
        scroll_offset: scroll,
    }
}

fn tab(name: &str) -> TabEntryView {
    TabEntryView {
        name: name.to_string(),
        has_notification: false,
        is_busy: false,
        is_agent_created: false,
    }
}

fn tab_notif(name: &str) -> TabEntryView {
    TabEntryView {
        name: name.to_string(),
        has_notification: true,
        is_busy: false,
        is_agent_created: false,
    }
}

fn tab_busy(name: &str) -> TabEntryView {
    TabEntryView {
        name: name.to_string(),
        has_notification: false,
        is_busy: true,
        is_agent_created: false,
    }
}

fn tab_agent(name: &str) -> TabEntryView {
    TabEntryView {
        name: name.to_string(),
        has_notification: false,
        is_busy: false,
        is_agent_created: true,
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "PaneTabBarsProps + draw_pane_tab_bars_view — AppState/CoreState 비의존.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Wrapper: src/adapters/ui/tab_bar.rs::draw_pane_tab_bars")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let tab_width: f32 = 160.0;
    let tab_font_size: f32 = 12.0;

    egui::ScrollArea::vertical()
        .id_salt("tab_bar_demo_scroll")
        .show(ui, |ui| {
            // Case 1 — single pane, single tab
            ui.label(
                egui::RichText::new("Case 1 — 단일 pane, 단일 탭 (focused)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let panes = vec![mk_pane(1, 600.0, vec![tab("main")], 0, true, 0.0)];
            let props = PaneTabBarsProps {
                theme,
                panes: &panes,
                tab_width,
                tab_font_size,
            };
            draw_pane_tab_bars_mock(ui, &props);
            ui.add_space(16.0);

            // Case 2 — 5 tabs, notif + busy + agent 혼합
            ui.label(
                egui::RichText::new(
                    "Case 2 — 5 탭, 활성 강조 + notification (노란) + busy (녹색 점) + agent (mauve 점)",
                )
                .strong()
                .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let panes = vec![mk_pane(
                2,
                900.0,
                vec![
                    tab("README.md"),
                    tab_busy("build.rs"),
                    tab_agent("agent/run.rs"),
                    tab_notif("Cargo.toml"),
                    tab("docs/index.md"),
                ],
                2,
                true,
                0.0,
            )];
            let props = PaneTabBarsProps {
                theme,
                panes: &panes,
                tab_width,
                tab_font_size,
            };
            draw_pane_tab_bars_mock(ui, &props);
            ui.add_space(16.0);

            // Case 3 — 매우 긴 탭 이름 (말줄임 검증)
            ui.label(
                egui::RichText::new("Case 3 — 매우 긴 탭 이름 (말줄임 …)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let panes = vec![mk_pane(
                3,
                700.0,
                vec![
                    tab("very-long-filename-that-overflows-the-tab-width.tsx"),
                    tab("another/deep/nested/path/component-file.rs"),
                    tab("short.md"),
                ],
                0,
                true,
                0.0,
            )];
            let props = PaneTabBarsProps {
                theme,
                panes: &panes,
                tab_width,
                tab_font_size,
            };
            draw_pane_tab_bars_mock(ui, &props);
            ui.add_space(16.0);

            // Case 4 — 12 탭, 스크롤 화살표
            ui.label(
                egui::RichText::new("Case 4 — 12 탭 + 좁은 pane → 좌/우 스크롤 화살표")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let many_tabs: Vec<TabEntryView> = (0..12)
                .map(|i| {
                    if i == 3 {
                        tab_busy(&format!("tab-{i:02}"))
                    } else if i == 7 {
                        tab_notif(&format!("tab-{i:02}"))
                    } else {
                        tab(&format!("tab-{i:02}"))
                    }
                })
                .collect();
            let panes = vec![mk_pane(4, 600.0, many_tabs, 5, true, 60.0)];
            let props = PaneTabBarsProps {
                theme,
                panes: &panes,
                tab_width,
                tab_font_size,
            };
            draw_pane_tab_bars_mock(ui, &props);
            ui.add_space(16.0);

            // Case 5 — 4-pane (focus 강조 차이)
            ui.label(
                egui::RichText::new(
                    "Case 5 — 4 pane (focused vs non-focused — 배경 surface0 vs mantle)",
                )
                .strong()
                .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let panes = vec![
                mk_pane(
                    11,
                    400.0,
                    vec![tab("a.txt"), tab_notif("b.txt")],
                    0,
                    true,
                    0.0,
                ),
                mk_pane(12, 400.0, vec![tab("c.rs"), tab("d.rs")], 1, false, 0.0),
                mk_pane(13, 400.0, vec![tab_busy("server.log")], 0, false, 0.0),
                mk_pane(
                    14,
                    400.0,
                    vec![tab("notes.md"), tab("todo.md"), tab("draft.md")],
                    2,
                    false,
                    0.0,
                ),
            ];
            let props = PaneTabBarsProps {
                theme,
                panes: &panes,
                tab_width,
                tab_font_size,
            };
            draw_pane_tab_bars_mock(ui, &props);

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(
                    "⚠ Drag overlay (ghost tab + 파란 insert marker) 는 본체 view 가 \
                     `ctx.layer_painter(Tooltip)` 로 그림 — 갤러리는 정적 상태만.",
                )
                .small()
                .color(egui::Color32::from(theme.subtext0)),
            );
        });
}
