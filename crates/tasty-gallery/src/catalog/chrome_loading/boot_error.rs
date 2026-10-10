//! 부팅 실패 화면 예제. 본체와 같은 `boot_error_screen`을 최소 크기 창에 그린다.
//! 문구는 본체가 이 화면에 넘기는 i18n 키를 그대로 쓰고, 오류 상세와 경로만 예시 값이다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{BOOT_ERROR_CONTENT_WIDTH, BootErrorView, boot_error_screen};

use super::CANVAS_MIN;
use crate::catalog::spec::{TokenChip, meta, note, spec};
use crate::i18n::{t, t_fmt, t_fmt2};

/// 본체가 이 화면을 띄우는 세 경우.
#[derive(Clone, Copy)]
enum Case {
    /// 터미널 엔진을 만들지 못했다.
    Engine,
    /// 다른 Tasty가 같은 데이터 폴더를 쓰고 있다.
    HomeInUse,
    /// 저장된 웹훅 포트를 열지 못했다. 안내에 CLI 명령 code run이 여럿 들어간다.
    WebhookPort,
}

fn copy(case: Case) -> (String, String, String) {
    match case {
        Case::Engine => (
            t("boot.engine_error.title").to_owned(),
            t_fmt(
                "boot.engine_error.body",
                "No such file or directory (os error 2)",
            ),
            t("boot.engine_error.hint").to_owned(),
        ),
        Case::HomeInUse => (
            t("boot.home_in_use.title").to_owned(),
            t_fmt("boot.home_in_use.body", "/home/dev/.tasty"),
            t("boot.home_in_use.hint").to_owned(),
        ),
        Case::WebhookPort => (
            t("boot.webhook_port.title").to_owned(),
            t_fmt2(
                "boot.webhook_port.body",
                "7801",
                "Address already in use (os error 98)",
            ),
            t("boot.webhook_port.hint_config").to_owned(),
        ),
    }
}

fn frame(ui: &mut egui::Ui, theme: &Theme, index: usize, case: Case) {
    let bw = theme.border_width.value();
    let size = egui::vec2(CANVAS_MIN.0 + bw * 2.0, CANVAS_MIN.1 + bw * 2.0);
    let (outer, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(
        outer,
        theme.corner_radius_lg.value(),
        theme.border_strong().to_egui(),
    );
    let inner = outer.shrink(bw);
    let (title, body, hint) = copy(case);
    let view = BootErrorView {
        title: &title,
        body: &body,
        hint: &hint,
        quit: t("button.quit"),
    };
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .id_salt(("boot_error_frame", index)),
    );
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    // 예제는 화면만 보이므로 종료 눌림을 버린다.
    let _quit = boot_error_screen(&mut child, theme, &view);
}

/// 부팅 실패 화면 Spec. 같은 구역의 앞 Spec 뒤에 이어 그린다. 디자인 시안은 아직 없다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec(
        ui,
        theme,
        "Boot error screen",
        Some(
            "Shown instead of the app when Tasty cannot start: the terminal engine failed, \
             another Tasty holds the data folder, or the saved webhook port is taken. \
             Quit, Esc or Enter exits with code 1.",
        ),
    );
    let latte = crate::host_shell::latte_theme();
    let frames = [
        (theme, Case::Engine),
        (theme, Case::HomeInUse),
        (theme, Case::WebhookPort),
        (&latte, Case::HomeInUse),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing =
            egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
        for (index, (th, case)) in frames.into_iter().enumerate() {
            frame(ui, th, index, case);
        }
    });
    let width = format!(
        "{} content + space-lg padding on each side",
        BOOT_ERROR_CONTENT_WIDTH.value()
    );
    meta(
        ui,
        theme,
        &[
            ("surface", "bg-app fill, card centred"),
            (
                "card",
                "bg-sidebar · 1px border-default · radius-lg · shadow-modal",
            ),
            ("width", &width),
            (
                "stack",
                "title (heading, danger) → 2 → body (body) → space-md → hint (caption, muted) → space-lg → Quit",
            ),
            ("quit button", "120 × 34 · danger fill · bg-panel label"),
            ("CLI runs", "backtick runs in body and hint are code runs"),
        ],
        &[
            TokenChip::new("bg-sidebar", "card", theme.bg_sidebar().to_egui()),
            TokenChip::new(
                "accent-danger",
                "title · Quit",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new("text-muted", "hint", theme.text_muted().to_egui()),
            TokenChip::without_color("shadow-modal", "card lift"),
        ],
    );
    note(
        ui,
        theme,
        "No design mockup yet. The card width, the Quit size and the radius-6 Quit corner are \
         the values the host used before the screen moved to the shared widget.",
    );
}
