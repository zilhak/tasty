//! 작업영역(작업 컬럼) 하단 StatusBar — 디자인 `ui_kits/terminal/work.jsx` 의
//! `StatusBar` 컴포넌트 대응. 위치·크기·구조(하단 24px 바, 좌/우 클러스터)는 확정이나
//! 표시 항목은 잠정이다 — 상세 `docs/features/workspace-status-bar/index.md`.
//!
//! ## 구성 (디자인 canonical)
//! - 높이 `theme.status_bar_height`(24), `bg_app` 배경 + 상단 1px `separator`.
//! - 좌측: 브랜치 점(`accent_success`)+이름 / surfaceId / `<shell> · <cols>×<rows>`.
//! - 우측(clickable): `Cmd+K palette` 칩(팔레트 오픈) + 테마 토글(점+테마명).
//!
//! ## focus 의존성 (원칙 3)
//! surfaceId·셸·그리드·브랜치는 **현재 focus surface 를 read** 해서 표시한다. 이는
//! "활성 상태 정보를 조회로 제공"하는 허용된 read 용도이며, 표시 *대상 결정* 이
//! focus 에 의존한다(동작이 아니라 표시이므로 focus 독립성 원칙에 위배되지 않음).
//!
//! ## view / wrapper 분리
//! 순수 [`draw_status_bar_view`] 는 [`StatusBarData`] + `Theme` 만 받고
//! [`StatusBarAction`] 리스트만 반환한다(AppState/CoreState 비의존). wrapper
//! [`draw_status_bar`] 가 state/engine 에서 데이터를 추출하고 action 을 적용한다.

use egui::emath::GuiRounding as _;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::PhysicalPx;
use tasty_type_geometry::rect::PhysicalRect;

use crate::state::AppState;
use crate::theme;

/// StatusBar 가 작업 컬럼 하단에 차지하는 inset (physical px) —
/// `compute_terminal_rect` 의 `bottom_inset` 인자의 단일 진실원.
/// 항상 그려지므로 항상 실제 높이를 반환한다(titlebar `top_inset` 과 대칭).
pub fn status_bar_bottom_inset(scale_factor: f32) -> PhysicalPx {
    theme::theme().status_bar_height.to_physical(scale_factor)
}

/// view 입력 — 한 프레임 분의 StatusBar 표시 데이터.
#[derive(Clone, Debug, Default)]
pub struct StatusBarData {
    /// git 브랜치명(focus surface 의 cwd 기준). repo 가 아니면 `None` → 미표시.
    pub branch: Option<String>,
    /// focus surface id(숫자). "Copy Terminal ID" 가 복사하는 값과 동일.
    pub surface_id: Option<u32>,
    /// 셸/포그라운드 프로세스명(terminal 한정).
    pub shell: Option<String>,
    /// 그리드 크기 (cols, rows) (terminal 한정).
    pub grid: Option<(usize, usize)>,
    /// 현재 테마명(capitalize 표시용 원본 id).
    pub theme_id: String,
    /// 현재 테마가 light 인지(테마 토글 점 색 결정: light=yellow, dark=mauve).
    pub theme_is_light: bool,
    /// 팔레트 단축키 표시 문자열(KeybindingSettings 연동, 빈 문자열이면 칩만 표시).
    pub palette_binding: String,
}

/// view 가 보고하는 사용자 클릭 액션.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarAction {
    /// 팔레트 칩 클릭 → 커맨드 팔레트 토글.
    OpenPalette,
    /// 테마 토글 클릭 → latte ↔ mocha 전환.
    ToggleTheme,
}

// 디자인 inline 레이아웃 값(work.jsx `StatusBar`: cell padding 0 10px, gap 6,
// dot 7×7). tab_bar.rs 와 동일하게 bar view 의 로컬 레이아웃 상수로 둔다.
const CELL_PAD_X: f32 = 10.0;
const CELL_GAP: f32 = 6.0;
const DOT_SIZE: f32 = 7.0;

/// 텍스트 너비 측정(logical px).
fn measure(ui: &egui::Ui, text: &str, font: &egui::FontId, color: egui::Color32) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), color)
            .size()
            .x
    })
}

/// 순수 시각 — `Theme` + [`StatusBarData`] 로 작업 컬럼 하단 24px 바를 그리고,
/// 사용자 클릭을 [`StatusBarAction`] 으로 수집해 반환한다.
///
/// `rect`는 바의 *logical* 사각형(작업 컬럼 폭 × status_bar_height).
pub fn draw_status_bar_view(
    ctx: &egui::Context,
    th: &Theme,
    rect: egui::Rect,
    data: &StatusBarData,
) -> Vec<StatusBarAction> {
    let mut actions = Vec::new();
    let font = egui::FontId::monospace(th.font_size_caption.value());
    let muted: egui::Color32 = th.text_muted().into();
    let hover: egui::Color32 = th.text_secondary().into();
    let success: egui::Color32 = th.accent_success().into();
    // divergence: light/dark 테마 표시 도트. warning/agent role 이 아니라 테마 종류 표시용이나
    // 전용 토큰이 없어(§4-9) 값-보존 위해 accent_warning()/accent_agent() 사용(픽셀 동일).
    let theme_dot: egui::Color32 = if data.theme_is_light {
        th.accent_warning().into()
    } else {
        th.accent_agent().into()
    };
    let bar_h = rect.height();

    egui::Area::new(egui::Id::new("workspace_status_bar"))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(th.bg_app().into())
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());

                    // 상단 1px separator (디자인: borderTop 1px separator).
                    ui.painter().hline(
                        rect.x_range(),
                        rect.top(),
                        egui::Stroke::new(1.0, th.separator),
                    );

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        // ── 좌측 클러스터 ──
                        // 브랜치 점 + 이름 (repo 일 때만).
                        if let Some(branch) = &data.branch {
                            dot_text_cell(ui, bar_h, &font, success, success, branch, DOT_SIZE);
                        }
                        // surfaceId.
                        if let Some(sid) = data.surface_id {
                            text_cell(ui, bar_h, &font, muted, &sid.to_string());
                        }
                        // shell · cols×rows.
                        if let (Some(shell), Some((cols, rows))) = (&data.shell, data.grid) {
                            text_cell(ui, bar_h, &font, muted, &format!("{shell} · {cols}×{rows}"));
                        }

                        // flex spacer.
                        let used = ui.min_rect().width();
                        let right_w = right_cluster_width(ui, &font, data);
                        let spacer = (rect.width() - used - right_w).max(0.0);
                        ui.add_space(spacer);

                        // ── 우측 클러스터 (clickable) ──
                        // 팔레트 칩: "<Cmd+K> palette".
                        let palette_label = if data.palette_binding.is_empty() {
                            crate::i18n::t("status_bar.palette").to_owned()
                        } else {
                            format!(
                                "{} {}",
                                data.palette_binding,
                                crate::i18n::t("status_bar.palette")
                            )
                        };
                        if button_cell(ui, bar_h, &font, muted, hover, &palette_label)
                            .on_hover_text(crate::i18n::t("status_bar.palette_tooltip"))
                            .clicked()
                        {
                            actions.push(StatusBarAction::OpenPalette);
                        }
                        // 테마 토글: 점 + 테마명(capitalize).
                        if dot_button_cell(
                            ui,
                            bar_h,
                            &font,
                            muted,
                            hover,
                            theme_dot,
                            &capitalize(&data.theme_id),
                            DOT_SIZE,
                        )
                        .on_hover_text(crate::i18n::t("status_bar.theme_tooltip"))
                        .clicked()
                        {
                            actions.push(StatusBarAction::ToggleTheme);
                        }
                    });
                });
        });

    actions
}

/// 우측 클러스터(팔레트 칩 + 테마 토글)의 총 너비를 미리 계산(spacer 산정용).
fn right_cluster_width(ui: &egui::Ui, font: &egui::FontId, data: &StatusBarData) -> f32 {
    let muted = egui::Color32::PLACEHOLDER;
    let palette_label = if data.palette_binding.is_empty() {
        crate::i18n::t("status_bar.palette").to_owned()
    } else {
        format!(
            "{} {}",
            data.palette_binding,
            crate::i18n::t("status_bar.palette")
        )
    };
    let palette_w = measure(ui, &palette_label, font, muted) + CELL_PAD_X * 2.0;
    let theme_w = DOT_SIZE
        + CELL_GAP
        + measure(ui, &capitalize(&data.theme_id), font, muted)
        + CELL_PAD_X * 2.0;
    palette_w + theme_w
}

/// 텍스트만 있는 셀(비클릭).
fn text_cell(ui: &mut egui::Ui, h: f32, font: &egui::FontId, color: egui::Color32, text: &str) {
    let w = measure(ui, text, font, color) + CELL_PAD_X * 2.0;
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        color,
    );
}

/// 점 + 텍스트 셀(비클릭).
fn dot_text_cell(
    ui: &mut egui::Ui,
    h: f32,
    font: &egui::FontId,
    dot: egui::Color32,
    text_color: egui::Color32,
    text: &str,
    dot_size: f32,
) {
    let w = dot_size + CELL_GAP + measure(ui, text, font, text_color) + CELL_PAD_X * 2.0;
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let dot_center = egui::pos2(r.left() + CELL_PAD_X + dot_size / 2.0, r.center().y);
    ui.painter().circle_filled(dot_center, dot_size / 2.0, dot);
    ui.painter().text(
        egui::pos2(r.left() + CELL_PAD_X + dot_size + CELL_GAP, r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font.clone(),
        text_color,
    );
}

/// 텍스트 버튼 셀(클릭 + hover 색 전환).
fn button_cell(
    ui: &mut egui::Ui,
    h: f32,
    font: &egui::FontId,
    color: egui::Color32,
    hover: egui::Color32,
    text: &str,
) -> egui::Response {
    let w = measure(ui, text, font, color) + CELL_PAD_X * 2.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let c = if resp.hovered() { hover } else { color };
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        c,
    );
    resp
}

/// 점 + 텍스트 버튼 셀(클릭 + hover 색 전환). 점 색은 hover 와 무관하게 고정.
#[allow(clippy::too_many_arguments)] // reason: 점/텍스트/hover 색을 모두 받는 셀
fn dot_button_cell(
    ui: &mut egui::Ui,
    h: f32,
    font: &egui::FontId,
    color: egui::Color32,
    hover: egui::Color32,
    dot: egui::Color32,
    text: &str,
    dot_size: f32,
) -> egui::Response {
    let w = dot_size + CELL_GAP + measure(ui, text, font, color) + CELL_PAD_X * 2.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let c = if resp.hovered() { hover } else { color };
    let dot_center = egui::pos2(r.left() + CELL_PAD_X + dot_size / 2.0, r.center().y);
    ui.painter().circle_filled(dot_center, dot_size / 2.0, dot);
    ui.painter().text(
        egui::pos2(r.left() + CELL_PAD_X + dot_size + CELL_GAP, r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font.clone(),
        c,
    );
    resp
}

/// 첫 글자 대문자화(디자인: 테마명 capitalize).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// focus surface 의 cwd 결정(terminal 은 store 의 `get_cwd()`, 그 외는 trait
/// `source_cwd()`). state.rs 의 `cwd_from_surface` 와 동일 규칙.
fn focused_cwd(engine: &crate::core::CoreState, sid: u32) -> Option<std::path::PathBuf> {
    let surface = engine.find_surface_by_id(sid)?;
    if surface.kind() == "terminal" {
        engine.terminals.get(sid).and_then(|t| t.get_cwd())
    } else {
        surface.source_cwd()
    }
}

/// cwd 기준 git 브랜치명. `.git/HEAD` 를 cwd 부터 상위로 올라가며 찾아 파싱한다
/// (git 바이너리/libgit2 비의존, std::fs 만 — 크로스플랫폼). repo 가 아니거나
/// detached HEAD 면 `None`.
fn git_branch(cwd: &std::path::Path) -> Option<String> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let head = d.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head) {
            let line = content.trim();
            // "ref: refs/heads/<branch>" → branch. detached(직접 SHA)면 None.
            return line
                .strip_prefix("ref: refs/heads/")
                .map(|b| b.trim().to_owned());
        }
        dir = d.parent();
    }
    None
}

/// wrapper — state/engine 에서 데이터를 추출해 view 를 그리고, view 가 보고한
/// 클릭 액션을 state mutation(팔레트 오픈 / 테마 토글)으로 변환한다.
///
/// `terminal_rect` 는 `bottom_inset` 이 이미 반영된 작업 컬럼 사각형(physical).
/// StatusBar 는 그 바로 아래 strip 을 차지한다.
pub fn draw_status_bar(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let th = theme::theme();
    let bar_h_logical = th.status_bar_height.value();

    // 작업 컬럼 하단 strip 의 logical 사각형.
    let x = (terminal_rect.x.value() / scale_factor).round_ui();
    let y = ((terminal_rect.y.value() + terminal_rect.height.value()) / scale_factor).round_ui();
    let w = (terminal_rect.width.value() / scale_factor).round_ui();
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, bar_h_logical));

    // ── 데이터 추출 (immutable read) ──
    let surface_id = state.focused_surface_id(engine);
    // Grid (cols/rows) is a lock-free handle-cache read. The foreground process
    // name comes from the 1Hz BusyPoll cache (`foreground_name`) rather than a
    // per-frame system snapshot — re-snapshotting every frame both cost ≈6ms on
    // the main thread and made the name flicker while agents churned helpers.
    let grid = surface_id
        .and_then(|sid| engine.terminals.get(sid))
        .map(|term| (term.cols(), term.rows()));
    let shell = surface_id.and_then(|sid| engine.foreground_name(sid).map(str::to_owned));
    let branch = surface_id
        .and_then(|sid| focused_cwd(engine, sid))
        .and_then(|cwd| git_branch(&cwd));
    let palette_binding = engine
        .settings
        .keybindings
        .toggle_command_palette
        .first()
        .map(|b| tasty_settings::KeybindingSettings::format_display(b, &engine.settings.general))
        .unwrap_or_default();

    let data = StatusBarData {
        branch,
        surface_id,
        shell,
        grid,
        theme_id: engine.settings.appearance.theme.clone(),
        theme_is_light: th.is_light,
        palette_binding,
    };

    // ── view ──
    let actions = draw_status_bar_view(ctx, &th, rect, &data);

    // ── action 적용 ──
    for action in actions {
        match action {
            StatusBarAction::OpenPalette => {
                use crate::intent::{OpenPopupMode, UiIntent};
                state.command_palette.reset();
                state.dispatch_intent(
                    UiIntent::TogglePopup {
                        id: crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID,
                        mode: OpenPopupMode::CenteredFocused,
                    }
                    .from_user_menu("status_bar.palette"),
                );
            }
            StatusBarAction::ToggleTheme => {
                // 디자인 onTheme: latte ↔ mocha. 그 외 테마에서 누르면 latte 로.
                let target = if engine.settings.appearance.theme == tasty_themes::BUILTIN_LATTE_ID {
                    tasty_themes::BUILTIN_MOCHA_ID
                } else {
                    tasty_themes::BUILTIN_LATTE_ID
                };
                let mut new_settings = engine.settings.clone();
                tasty_themes::apply_theme(&mut new_settings.appearance, target);
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::UpdateSettings(new_settings)
                        .from_user_menu("status_bar.theme_toggle"),
                );
            }
        }
    }
}
