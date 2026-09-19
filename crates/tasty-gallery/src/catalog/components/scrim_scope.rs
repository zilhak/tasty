//! `scrim-scope` specimen — scrim 은 popup 이 소속된 **scope 의 rect** 를 덮는다.
//!
//! 셸 목업(`shell_mock`) 하나 위에 props 를 바꿔 가며 같은 장면을 그린다 —
//! titlebar · 접힌 사이드바 레일 · pane 탭바 · surface 둘 · 상태바. props 는
//! `scope`(Window/Surface) · `split`(좌우/상하) · `child`(자식 picker 동반) ·
//! `narrow`(소속 surface 를 좁게) · `clamp`(popup 이 scope 보다 커서 눌림).
//!
//! 결정표가 정한 것만 그린다:
//! - **Window scope** — 커맨드 팔레트 · 설정 · 모든 가운데 confirm 은 **창 전체** scrim 을
//!   그대로 쓴다. target 바인딩이 없는 호환 경로도 여기에 남는다.
//! - **Surface scope** — scrim 이 소속 surface rect 만 덮는다. **보더는 포함하고 인접
//!   surface · 사이드바 · 탭바 · 상태바는 제외**한다.
//! - **radius** — scope 대상 자신의 radius 를 따른다. 오늘의 셸에서 surface 는 0 이라
//!   surface scope 의 scrim 은 직각이고, 창은 `corner_radius` 를 따른다.
//! - **알파** — `--tasty-scrim-bg` 한 벌(`Theme::scrim`). scope 가 둘이라고 알파를 두
//!   벌로 만들지 않는다.
//! - **scope 당 scrim 한 번** — 부모 popup 과 자식 file picker 가 같은 scope 를 공유하면
//!   scrim 은 **한 번만** 깔린다. 자식이 두 번째를 덧그리면 알파가 겹쳐 같은 토큰이 두
//!   배로 어두워진다. z 는 scrim 0 · 부모 1 · 자식 2.
//! - **8px inset** — popup 은 scope rect 에서 `spacing_sm` 만큼 안쪽으로 눌린다.
//!
//! 이 Spec 이 **바꾸지 않는 것**: 입력 차단(dim ≠ block) · Esc/바깥클릭 순서 · draft
//! 수명과 부모/자식 숨김-복원 정책 · 그림자 두 값과 그 3-갈래 규칙(ADR-0254).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::spec::{self, StageVariant, TokenChip};

// ── specimen 무대 치수 ────────────────────────────────────────────────────
// 전부 Theme 토큰의 배수로 적는다. 수를 그대로 적으면 그 값이 `size-*` 스케일 안일 때
// 토큰 값을 손으로 다시 쓴 자리가 되고(그 자리는 토큰이 움직여도 안 따라간다),
// `src/source_guards/on_scale_length_literal.rs` 가 그것을 센다.
/// 셸 목업의 바깥 치수. 폭은 measure-md, 높이는 크롬 셋에 body 를 얹은 값이다.
fn shell_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(
        theme.measure_md.value(),
        theme.titlebar_height.value()
            + theme.tab_bar_height.value()
            + theme.status_bar_height.value()
            + theme.spacing_xl.value() * 8.0,
    )
}

/// 부모 popup 카드 크기 — surface 한 칸에 8px inset 까지 두고 들어가는 크기.
fn parent_card_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(
        theme.spacing_xl.value() * 6.0,
        theme.spacing_xl.value() * 2.0,
    )
}

/// 자식 picker 카드 크기 — 부모보다 작아야 겹쳤을 때 z 순서가 눈에 남는다.
fn child_card_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(
        theme.spacing_xl.value() * 4.0,
        theme.spacing_lg.value() * 2.0,
    )
}

/// `clamp` 변형이 요구하는 폭 — 좁은 칸보다 커서 inset 까지 눌리는 값.
fn clamped_card_width(theme: &Theme) -> f32 {
    theme.spacing_xl.value() * 13.0
}

/// 오늘의 셸에서 surface 자신의 radius. scope rect 의 radius 가 이것을 따른다.
const SURFACE_RADIUS: LogicalPx = LogicalPx(0.0);
/// `narrow` 일 때 소속 surface 가 차지하는 body 비율.
const NARROW_FRACTION: f32 = 0.32;
/// surface 안에 그리는 가짜 터미널 줄. dim 이 눈에 남게 하는 데만 쓴다.
const FAKE_LINES: [&str; 4] = ["$ cargo test", "  running 12 tests", "  ok", "$ _"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// 창 전체 — 커맨드 팔레트 · 설정 · target 바인딩 없는 호환 경로.
    Window,
    /// 소속 surface 만 — convert_surface 처럼 surface 에 묶인 popup.
    Surface,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    /// 좌우 분할 — surface 둘이 가로로 눕는다.
    Horizontal,
    /// 상하 분할 — surface 둘이 세로로 쌓인다.
    Vertical,
}

#[derive(Clone, Copy)]
struct ShellProps {
    scope: Scope,
    split: Split,
    child: bool,
    narrow: bool,
    clamp: bool,
}

impl ShellProps {
    fn new(scope: Scope) -> Self {
        Self {
            scope,
            split: Split::Horizontal,
            child: false,
            narrow: false,
            clamp: false,
        }
    }
    fn split(mut self, split: Split) -> Self {
        self.split = split;
        self
    }
    fn child(mut self) -> Self {
        self.child = true;
        self
    }
    fn narrow_clamp(mut self) -> Self {
        self.narrow = true;
        self.clamp = true;
        self
    }
}

/// body 를 두 surface rect 로 가른다. 첫째가 popup 의 소속 surface다.
/// 분할선은 `border_width` 한 줄 — 두 rect 는 그 줄을 나눠 갖지 않는다(인접 제외).
fn split_body(body: egui::Rect, props: ShellProps, divider: f32) -> (egui::Rect, egui::Rect) {
    let fraction = if props.narrow { NARROW_FRACTION } else { 0.5 };
    match props.split {
        Split::Horizontal => {
            let cut = body.left() + (body.width() - divider) * fraction;
            (
                egui::Rect::from_min_max(body.min, egui::pos2(cut, body.bottom())),
                egui::Rect::from_min_max(egui::pos2(cut + divider, body.top()), body.max),
            )
        }
        Split::Vertical => {
            let cut = body.top() + (body.height() - divider) * fraction;
            (
                egui::Rect::from_min_max(body.min, egui::pos2(body.right(), cut)),
                egui::Rect::from_min_max(egui::pos2(body.left(), cut + divider), body.max),
            )
        }
    }
}

/// surface 한 칸 — 터미널 배경 + 1px 보더 + 가짜 프롬프트 줄. 보더는 surface rect
/// 안쪽에 그려지므로 scope rect 가 보더를 **포함**한다.
///
/// 콘텐츠 줄이 있어야 dim 이 눈에 남는다 — 빈 검정 칸은 scrim 을 씌워도 검정이라,
/// "인접 surface 는 안 어두워진다" 가 그림으로 안 읽힌다.
///
/// 두 칸을 **같은 채움·같은 글자색**(focused)으로 그린다. 포커스 색차를 같이 얹으면
/// 두 칸이 달라 보이는 원인이 둘이 되어, 이 specimen 이 재려는 것(scrim 이 한 칸에만
/// 닿는다)을 그림만 보고는 가릴 수 없다.
fn paint_surface(p: &egui::Painter, theme: &Theme, rect: egui::Rect) {
    let term = theme.surface("terminal");
    let (fill, fg) = (term.focused_bg, term.focused_fg);
    p.rect_filled(rect, SURFACE_RADIUS.value(), egui::Color32::from(fill));
    p.rect_stroke(
        rect,
        SURFACE_RADIUS.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );

    let pad = theme.spacing_sm.value();
    let line_h = theme.font_size_micro.value() + theme.spacing_xs.value();
    let font = egui::FontId::monospace(theme.font_size_micro.value());
    let inner = p.with_clip_rect(rect.shrink(theme.border_width.value()));
    for (i, line) in FAKE_LINES.iter().enumerate() {
        let y = rect.top() + pad + line_h * i as f32;
        if y + line_h > rect.bottom() {
            break;
        }
        inner.text(
            egui::pos2(rect.left() + pad, y),
            egui::Align2::LEFT_TOP,
            line,
            font.clone(),
            egui::Color32::from(fg),
        );
    }
}

/// 크롬 띠 한 줄 — 채움 + micro 라벨. 타이틀바 · 탭바 · 상태바가 공유한다.
fn paint_chrome(
    p: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    radius: f32,
    fill: egui::Color32,
    label: &str,
) {
    p.rect_filled(rect, radius, fill);
    p.text(
        egui::pos2(rect.left() + theme.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
}

/// popup 카드 한 장 — fill + 1px border-strong + modal shadow + 가운데 라벨.
/// 그림자 갈래는 이 Spec 이 안 바꾼다(ADR-0254) — 뷰포트를 점유하는 표면이라 modal.
fn paint_card(
    p: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    fill: egui::Color32,
    label: &str,
) {
    let radius = theme.corner_radius.value();
    p.add(theme.shadow_modal().to_egui().as_shape(rect, radius));
    p.rect_filled(rect, radius, fill);
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    // 제목은 좌상단이다 — 가운데 두면 자식 카드가 부모 위에 얹혔을 때 부모의 정체가
    // 가려져, "자식이 부모 위에 있다" 와 "부모가 없다" 가 그림에서 구분되지 않는다.
    p.text(
        rect.min + egui::vec2(theme.spacing_sm.value(), theme.spacing_sm.value()),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(theme.font_size_micro.value()),
        theme.text_primary().to_egui(),
    );
}

/// 셸 목업 — 크롬 · surface 둘 · scope scrim · popup. scrim 은 **한 번만** 깔린다.
fn shell_mock(ui: &mut egui::Ui, theme: &Theme, props: ShellProps) {
    let (rect, _) = ui.allocate_exact_size(shell_size(theme), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let radius = theme.corner_radius.value();
    let divider = theme.border_width.value();

    // 창 배경 + 1px 프레임.
    p.rect_filled(rect, radius, theme.bg_app().to_egui());
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_frame().to_egui()),
        egui::StrokeKind::Inside,
    );

    // titlebar · status bar · 접힌 사이드바 레일 · pane 탭바 — 전부 토큰 치수.
    let titlebar = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), theme.titlebar_height.value()),
    );
    paint_chrome(
        &p,
        theme,
        titlebar,
        radius,
        theme.bg_panel().to_egui(),
        "tasty",
    );
    let status = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - theme.status_bar_height.value()),
        rect.max,
    );
    paint_chrome(
        &p,
        theme,
        status,
        radius,
        theme.bg_panel().to_egui(),
        "status bar",
    );
    let rail = egui::Rect::from_min_max(
        egui::pos2(rect.left(), titlebar.bottom()),
        egui::pos2(
            rect.left() + theme.sidebar_collapsed_slot_width.value(),
            status.top(),
        ),
    );
    p.rect_filled(rail, 0.0, theme.bg_sidebar().to_egui());
    let tabs = egui::Rect::from_min_max(
        egui::pos2(rail.right(), titlebar.bottom()),
        egui::pos2(
            rect.right(),
            titlebar.bottom() + theme.tab_bar_height.value(),
        ),
    );
    paint_chrome(
        &p,
        theme,
        tabs,
        0.0,
        theme.bg_panel().to_egui(),
        "pane tabs",
    );

    // surface 둘. 첫째가 popup 의 소속 surface다.
    let body = egui::Rect::from_min_max(
        egui::pos2(rail.right(), tabs.bottom()),
        egui::pos2(rect.right(), status.top()),
    );
    let (owner, adjacent) = split_body(body, props, divider);
    paint_surface(&p, theme, owner);
    paint_surface(&p, theme, adjacent);

    // ── scrim: scope 당 한 번 ─────────────────────────────────────────────
    // rect 는 scope 대상의 rect, radius 는 그 대상 자신의 radius.
    let (scrim_rect, scrim_radius) = match props.scope {
        Scope::Window => (rect, radius),
        Scope::Surface => (owner, SURFACE_RADIUS.value()),
    };
    p.rect_filled(scrim_rect, scrim_radius, theme.scrim().to_egui());

    // popup 레이어는 scope rect 로 클립되고, 배치는 8px inset 안쪽에 눌린다.
    let clipped = ui.painter_at(scrim_rect);
    let bounds = scrim_rect.shrink(theme.spacing_sm.value());
    let want = parent_card_size(theme);
    let want_w = if props.clamp {
        clamped_card_width(theme)
    } else {
        want.x
    };
    let parent = egui::Rect::from_center_size(
        bounds.center(),
        egui::vec2(want_w.min(bounds.width()), want.y.min(bounds.height())),
    );
    paint_card(
        &clipped,
        theme,
        parent,
        theme.bg_panel().to_egui(),
        "Convert surface",
    );
    if props.child {
        // 자식은 같은 scope 를 상속한다 — scrim 을 **덧그리지 않는다**. z 만 부모 위.
        let offset = theme.spacing_lg.value();
        let want = child_card_size(theme);
        let child = egui::Rect::from_center_size(
            bounds.center() + egui::vec2(offset, offset),
            egui::vec2(want.x.min(bounds.width()), want.y.min(bounds.height())),
        );
        paint_card(
            &clipped,
            theme,
            child,
            theme.surface_raised().to_egui(),
            "Choose file",
        );
    }
}

// ── Spec 1 — window scope vs surface scope ────────────────────────────────

pub fn draw_scope(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "window scope · full-window scrim", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Window))
        });
        spec::cluster(ui, theme, "surface scope · owning surface only", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Surface))
        });
    });

    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "surface scope · top/bottom split", |ui| {
            shell_mock(
                ui,
                theme,
                ShellProps::new(Scope::Surface).split(Split::Vertical),
            )
        });
        spec::cluster(ui, theme, "narrow surface · clamped to 8px inset", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Surface).narrow_clamp())
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "window scope",
                "scrim = window rect, radius = window radius",
            ),
            (
                "surface scope",
                "scrim = owning surface rect, radius = surface radius (0 today)",
            ),
            ("boundary", "border included · adjacent surface excluded"),
            (
                "excluded",
                "sidebar rail · pane tab bar · status bar · titlebar",
            ),
            ("inset", "spacing-sm (8px) between scope rect and popup"),
            ("clip", "popup layer is clipped to the scope rect"),
            ("alpha", "one scrim value for both scopes"),
        ],
        &[
            TokenChip::new(
                "scrim-bg",
                "same value in both scopes",
                theme.scrim().into(),
            ),
            TokenChip::new(
                "bg-sidebar",
                "rail left lit under a surface scrim",
                theme.bg_sidebar().into(),
            ),
            TokenChip::new(
                "border-default",
                "surface edge — inside the scrim rect",
                theme.border_default().into(),
            ),
            TokenChip::new(
                "bg-panel",
                "chrome left lit under a surface scrim",
                theme.bg_panel().into(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "surface 에 묶인 popup 은 그 surface rect 만 어둡게 한다 — 옆 surface 에서 도는 \
         작업이 계속 읽힌다.",
    );
    spec::dont(
        ui,
        theme,
        "scope 가 둘이라고 알파를 두 벌로 나누지 마라. 값은 한 벌이고 덮는 rect 만 다르다.",
    );

    spec::note(
        ui,
        theme,
        "scrim 이 덮는 rect 는 popup 이 소속된 scope 의 rect다. 커맨드 팔레트 · 설정 · \
         모든 가운데 confirm 은 창 전체를 계속 덮고, target 바인딩이 없는 호환 경로도 \
         거기 남는다 — 바인딩이 없으면 좁힐 대상이 없기 때문이다. surface scope 만 \
         소속 surface 로 좁아지며, 경계는 그 surface 의 보더를 포함하고 인접 \
         surface · 사이드바 · 탭바 · 상태바는 제외한다. radius 는 새 값이 아니라 \
         scope 대상 자신의 radius 이고, 오늘의 셸에서 surface radius 는 0 이라 직각으로 \
         떨어진다. 좁은 surface 에서는 popup 이 scope rect 에서 8px inset 안쪽으로 눌린다 \
         — 어둡게 하는 것과 입력을 막는 것은 다른 일이라, 이 Spec 은 dim 만 다루고 입력 \
         차단 · Esc/바깥클릭 순서는 그대로 둔다.",
    );
}

// ── Spec 2 — 부모 + 자식이 scope 하나를 나눠 쓴다 ─────────────────────────

pub fn draw_child(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "parent only", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Surface))
        });
        spec::cluster(ui, theme, "parent + child picker · one scrim", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Surface).child())
        });
    });

    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "window scope · parent + child", |ui| {
            shell_mock(ui, theme, ShellProps::new(Scope::Window).child())
        });
        spec::cluster(ui, theme, "top/bottom split · parent + child", |ui| {
            shell_mock(
                ui,
                theme,
                ShellProps::new(Scope::Surface)
                    .split(Split::Vertical)
                    .child(),
            )
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("scrim count", "one per scope, not one per popup"),
            ("z order", "scrim 0 · parent 1 · child 2"),
            ("child scope", "inherited from the parent popup"),
            ("child scrim", "none — the parent's scrim already covers it"),
            (
                "draw paths",
                "host popups and plugin popups share the scope's single scrim",
            ),
        ],
        &[
            TokenChip::new(
                "scrim-bg",
                "painted once — a second pass would double the alpha",
                theme.scrim().into(),
            ),
            TokenChip::new("bg-panel", "parent card fill", theme.bg_panel().into()),
            TokenChip::new(
                "surface-raised",
                "child picker fill",
                theme.surface_raised().into(),
            ),
        ],
    );

    spec::dont(
        ui,
        theme,
        "자식 picker 를 띄울 때 scrim 을 한 겹 더 깔지 마라 — 같은 토큰이 두 번 곱해져 \
         부모만 있을 때보다 어두워진다.",
    );

    spec::note(
        ui,
        theme,
        "부모 popup 과 그것이 연 file picker 는 같은 scope 를 공유한다 — 자식은 부모의 \
         scope 를 물려받으므로 scrim 은 그 scope 에 한 번만 깔리고, 두 카드는 그 위에 \
         z 만 나눠 얹힌다(scrim 0 · 부모 1 · 자식 2). 두 카드가 서로 다른 그리기 경로에서 \
         나오더라도 scrim 의 주인은 scope 하나다. 자식이 자기 scrim 을 덧그리면 알파가 \
         겹쳐, 같은 값을 쓰는데도 자식이 열린 순간 배경이 한 단 더 어두워진다 — 이 \
         specimen 의 왼쪽(부모만)과 오른쪽(부모+자식)의 배경 밝기가 같아야 한다는 것이 \
         그 판정이다. 부모/자식의 숨김-복원 정책과 draft 수명은 이 Spec 이 바꾸지 않는다.",
    );
}
