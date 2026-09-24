//! 갤러리의 구역·예제·설명·토큰 표를 배치하는 공용 헬퍼.

use tasty_type_appearance::theme::Theme;

/// 예제 영역의 레이아웃 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StageVariant {
    /// flex wrap, padding 26 — 기본 무대.
    #[default]
    Wrap,
    /// padding 0 — 풀블리드 데모 (Table / Tab strip 등).
    Tight,
    /// 세로 적층.
    Column,
    /// 가로 중앙 정렬.
    Center,
    /// 단독 큰 데모 (모달 프레임 등) — radius 전체.
    Solo,
}

/// "Tokens used" 칩 한 개 — 색 스와치 + 토큰명 + 용도.
#[derive(Clone, Copy)]
pub struct TokenChip {
    pub tok: &'static str,
    pub use_: &'static str,
    pub color: egui::Color32,
}

impl TokenChip {
    pub fn new(tok: &'static str, use_: &'static str, color: egui::Color32) -> Self {
        Self { tok, use_, color }
    }
}

#[inline]
fn col(h: impl Into<egui::Color32>) -> egui::Color32 {
    h.into()
}

/// 구역 제목과 아래 구분선을 그린다.
pub fn section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    // margin-top 46 ≈ spacing_xl(24) + spacing_lg(16).
    ui.add_space(theme.spacing_xl.value() + theme.spacing_lg.value());
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(theme.font_size_term_sm.value())
            .color(col(theme.text_muted())),
    );
    ui.add_space(theme.spacing_sm.value());
    hline(ui, theme, col(theme.separator));
    ui.add_space(theme.spacing_sm.value());
}

/// 예제 제목과 사용 상황 설명을 그린다.
pub fn spec(ui: &mut egui::Ui, theme: &Theme, title: &str, when: Option<&str>) {
    // margin-top 26 ≈ spacing_xl(24).
    ui.add_space(theme.spacing_xl.value());
    ui.label(
        egui::RichText::new(title)
            .size(theme.font_size_term_lg.value())
            .strong()
            .color(col(theme.text_primary())),
    );
    if let Some(w) = when {
        ui.add_space(theme.spacing_xs.value());
        ui.label(
            egui::RichText::new(w)
                .size(theme.font_size_body.value())
                .color(col(theme.text_secondary())),
        );
    }
    ui.add_space(theme.spacing_md.value());
}

/// 예제 영역의 테두리·배경과 종류별 여백·레이아웃을 적용한다.
pub fn stage(
    ui: &mut egui::Ui,
    theme: &Theme,
    variant: StageVariant,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    // padding 26 ≈ spacing_xl(24); tight 은 0.
    let pad = match variant {
        StageVariant::Tight => 0.0,
        _ => theme.spacing_xl.value(),
    };
    egui::Frame::new()
        .fill(col(theme.bg_panel()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            col(theme.border_default()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(pad as i8))
        .show(ui, |ui| match variant {
            StageVariant::Column => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
                    add_contents(ui);
                });
            }
            StageVariant::Center => {
                ui.vertical_centered(add_contents);
            }
            // 큰 단독 예제와 여백 없는 예제는 세로 배치를 사용한다.
            StageVariant::Solo | StageVariant::Tight => {
                ui.vertical(add_contents);
            }
            StageVariant::Wrap => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
                    add_contents(ui);
                });
            }
        });
}

/// 라벨과 예제들을 한 묶음으로 배치한다.
pub fn cluster(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .size(theme.font_size_micro.value())
                .color(col(theme.text_muted())),
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            add_contents(ui);
        });
    });
}

/// 왼쪽에는 치수 설명을, 오른쪽에는 사용한 토큰을 표시한다.
/// `tokens` 가 비면 1컬럼(Layout spec)만 그린다.
pub fn meta(ui: &mut egui::Ui, theme: &Theme, specs: &[(&str, &str)], tokens: &[TokenChip]) {
    body_column(ui, |ui| {
        egui::Frame::new()
            .fill(col(theme.bg_panel()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                col(theme.separator),
            ))
            .corner_radius(theme.corner_radius_sm.value())
            .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
            .show(ui, |ui| {
                let n = if tokens.is_empty() { 1 } else { 2 };
                ui.columns(n, |cols| {
                    meta_head(&mut cols[0], theme, "Layout spec");
                    for (k, v) in specs {
                        cols[0].horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                            ui.label(
                                egui::RichText::new(*k)
                                    .size(theme.font_size_term_sm.value())
                                    .color(col(theme.text_muted())),
                            );
                            ui.label(
                                egui::RichText::new(*v)
                                    .size(theme.font_size_term_sm.value())
                                    .color(col(theme.text_primary())),
                            );
                        });
                    }
                    if !tokens.is_empty() {
                        meta_head(&mut cols[1], theme, "Tokens used");
                        for t in tokens {
                            cols[1].horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                                let sz = theme.font_size_caption.value();
                                let (r, _) = ui
                                    .allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                                ui.painter().rect_filled(
                                    r,
                                    theme.corner_radius_sm.value(),
                                    t.color,
                                );
                                ui.label(
                                    egui::RichText::new(t.tok)
                                        .size(theme.font_size_caption.value())
                                        .color(col(theme.text_secondary())),
                                );
                                ui.label(
                                    egui::RichText::new(t.use_)
                                        .size(theme.font_size_caption.value())
                                        .color(col(theme.text_muted())),
                                );
                            });
                        }
                    }
                });
            });
    });
}

fn meta_head(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(theme.font_size_micro.value())
            .color(col(theme.text_muted())),
    );
    ui.add_space(theme.spacing_sm.value());
}

/// `note`/`meta`/`do_`·`dont` 와 host_shell 이 공유하는 본문 컬럼 폭 temp-data 키.
pub(crate) fn body_column_width_id() -> egui::Id {
    egui::Id::new("g_body_column_width")
}

/// 예제가 가용 폭을 늘려도 설명이 창 밖으로 잘리지 않도록 본문 컬럼 폭에 맞춘다.
/// 저장된 컬럼 폭이 없으면 현재 가용 폭을 사용한다.
fn body_column<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let wrap_w = ui
        .data(|d| d.get_temp::<f32>(body_column_width_id()))
        .filter(|w| *w > 0.0)
        .unwrap_or_else(|| ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(wrap_w, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| add(ui),
    )
    .inner
}

/// 보조 설명을 작은 글씨로 표시한다.
pub fn note(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(theme.spacing_md.value());
    body_column(ui, |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(theme.font_size_term_sm.value())
                    .color(col(theme.text_muted())),
            )
            .wrap(),
        );
    });
}

/// 권장 사항을 success 색으로 표시한다.
pub fn do_(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    accent_bar(ui, theme, text, col(theme.accent_success()));
}

/// 피할 사항을 danger 색으로 표시한다.
pub fn dont(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    accent_bar(ui, theme, text, col(theme.accent_danger()));
}

fn accent_bar(ui: &mut egui::Ui, theme: &Theme, text: &str, accent: egui::Color32) {
    ui.add_space(theme.spacing_sm.value());
    body_column(ui, |ui| {
        // 배경은 강조색의 낮은 알파로 만든다.
        const ACCENT_TINT_OPACITY: f32 = 0.12;
        let tint = accent.gamma_multiply(ACCENT_TINT_OPACITY);
        let resp = egui::Frame::new()
            .fill(tint)
            .corner_radius(theme.corner_radius_sm.value())
            .inner_margin(egui::Margin {
                left: theme.spacing_md.value() as i8,
                right: theme.spacing_md.value() as i8,
                top: theme.spacing_sm.value() as i8,
                bottom: theme.spacing_sm.value() as i8,
            })
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .size(theme.font_size_term_sm.value())
                        .color(col(theme.text_secondary())),
                );
            });
        let r = resp.response.rect;
        let bar = egui::Rect::from_min_size(
            r.min,
            egui::vec2(theme.tab_indicator_width.value(), r.height()),
        );
        ui.painter().rect_filled(bar, 0.0, accent);
    });
}

/// 현재 ui 폭 전체에 1px separator 라인을 그린다 (세로 공간도 예약).
fn hline(ui: &mut egui::Ui, theme: &Theme, color: egui::Color32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), color),
    );
}
