//! 부팅 실패 화면 예제. 본체와 같은 `boot_error_screen`을 최소 크기 창에 그린다.
//! 문구는 본체가 이 화면에 넘기는 i18n 키를 그대로 쓰고, 오류 상세와 경로만 예시 값이다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{BootErrorView, boot_error_screen};

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

/// 부팅 실패 화면 Spec. 같은 구역의 앞 Spec 뒤에 이어 그린다. 디자인 `BootErrorFrame`(b12)이다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec(
        ui,
        theme,
        "Boot error screen",
        Some(
            "Shown instead of the app when Tasty cannot start: the terminal engine failed, \
             another Tasty holds the data folder, or the saved webhook port is taken. Same boot \
             stage as the loading and shell setup screens, so the same structure: lockup, then a \
             360 form with no card. The glyph carries the danger tone; the title stays \
             text-primary. Quit, Esc, Enter or closing the window exits with code 1.",
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
    meta(
        ui,
        theme,
        &[
            (
                "structure",
                "lockup → (space-xl) → form 360 · gaps space-sm · no card, no shadow",
            ),
            (
                "title row",
                "alertCircle 16 · boot-error-glyph → accent-danger · gap space-sm · title 14 / 600 text-primary, wraps",
            ),
            (
                "body",
                "font-size-body · text-secondary · line-height-ui · wraps; paths and OS errors break anywhere",
            ),
            (
                "guidance",
                "caption · text-muted · CLI pieces = code runs (ui-code-*)",
            ),
            (
                "quit button",
                "Button secondary md · right · Enter / Esc / close = Quit (exit 1)",
            ),
            (
                "long content",
                "never truncated · taller than the window − 2 × space-xl: lockup dropped first, then the text scrolls with Quit pinned under it",
            ),
        ],
        &[
            TokenChip::without_color("boot-form-width", "→ size-360"),
            TokenChip::new(
                "boot-error-glyph",
                "→ accent-danger",
                theme.boot_error_glyph().to_egui(),
            ),
            TokenChip::new("text-secondary", "body", theme.text_secondary().to_egui()),
            TokenChip::new("text-muted", "guidance", theme.text_muted().to_egui()),
        ],
    );
    note(
        ui,
        theme,
        "Copy is the app's (boot.*); the OS error and path in the samples are illustrative.",
    );
}
