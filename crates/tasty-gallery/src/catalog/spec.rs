//! Gallery specimen 빌딩 블록 — research §1.3 primitive 의 egui 1:1 대응.
//!
//! 문서형 셸(`host_shell`)이 페이지를 `Section > Spec` 트리로 렌더할 때,
//! 그리고 Round 2 의 각 페이지 specimen 본문(`Spec::draw`)이 무대/클러스터/메타를
//! 조립할 때 호출하는 공용 헬퍼.
//!
//! 모든 색·치수·폰트는 `Theme` 토큰에서만 가져온다 (raw px / `from_rgb` 금지).
//! Theme 에 정확히 대응하는 토큰이 없는 디자인 치수(예: section margin 46)는
//! 의미가 가장 가까운 spacing 토큰의 합/근사로 도출한다.

use tasty_type_appearance::theme::Theme;

/// Stage 레이아웃 변형 (research §1.3 `.stage` variants).
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

/// 카탈로그 구역 헤딩 — mono 12 uppercase muted + 하단 separator (research `.g-section > h2`).
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

/// 카탈로그 한 항목의 헤딩 — h3 16 600 + when 13 secondary (research `.spec-head`).
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

/// 라이브 데모 무대 — border + radius + bg-panel, 변형별 패딩/레이아웃 (research `.stage`).
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
            // Solo("단독 큰 데모")·Tight("풀블리드 padding 0") 의 디자인 의도는 둘 다
            // 세로 단일 컬럼. catch-all horizontal_wrapped 로 떨어지면 콘텐츠가 가로로
            // 흘러 배치가 무너지고(markdown 은 columns 음수폭 panic) → 세로 적층으로 명시.
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

/// 라벨 붙은 데모 묶음 — mono 10 uppercase muted 라벨 + 가로 행 (research `.cluster`).
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

/// 치수표 + 토큰칩 — 좌 "Layout spec" dl / 우 "Tokens used" 칩 (research `.meta`).
/// `tokens` 가 비면 1컬럼(Layout spec)만 그린다.
pub fn meta(ui: &mut egui::Ui, theme: &Theme, specs: &[(&str, &str)], tokens: &[TokenChip]) {
    // 본문 컬럼 폭 제약 — 무대 팽창 시 상자가 컬럼 밖으로 넘치지 않게(note 와 같은 근본).
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

/// 본문 컬럼 폭으로 제약된 하위 ui 를 할당해 `add` 를 그린다.
///
/// specimen 무대(`stage`)가 컬럼보다 넓게 그리면 그 뒤 본문 top_down ui 의 max_rect 가
/// 그만큼 늘어나(available_width 팽창), **본문 컬럼 레벨 헬퍼가 줄바꿈/배치되지 않고
/// 창 우측에서 잘린다** — `.wrap()` 만으로는 팽창한 폭에 맞춰져 소용이 없다. host_shell 이
/// 매 프레임 심어두는 본문 컬럼 폭으로 폭을 고정해 그 안에서 확실히 배치한다(없으면
/// available_width 폴백). 본문 산문 헬퍼는 전부 이걸 통과하므로 새 헬퍼가 생겨도 같은
/// 팽창에 자동으로 안전하다 — 소비자마다 폭을 다시 심지 않는다.
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

/// 보조 산문 — muted 작은 문단 (research `.note`).
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

/// 권장(Do) — success 좌측바 + tint 배경 (research `.do`).
pub fn do_(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    accent_bar(ui, theme, text, col(theme.accent_success()));
}

/// 금지(Dont) — danger 좌측바 + tint 배경 (research `.dont`).
pub fn dont(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    accent_bar(ui, theme, text, col(theme.accent_danger()));
}

fn accent_bar(ui: &mut egui::Ui, theme: &Theme, text: &str, accent: egui::Color32) {
    ui.add_space(theme.spacing_sm.value());
    // 본문 컬럼 폭 제약 — 무대 팽창 시 do_/dont tint 상자가 컬럼 밖으로 넘쳐 텍스트가
    // 창 우측에서 잘리지 않게(note/meta 와 같은 근본).
    body_column(ui, |ui| {
        // accent 12% tint (research: success/danger 12% bg). 기존 theme accent 에서
        // gamma_multiply 로 도출 — git_viewer specimen 의 9~10% tint 와 동일 관행
        // (raw from_rgba_* 는 disallowed-methods 로 금지).
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
