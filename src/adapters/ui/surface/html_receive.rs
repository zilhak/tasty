//! Received-design size decisions and page state; all transport effects remain App-owned.
use crate::runtime::html_receive::{ReceiveAction, ReceiveActionKind, ReceiveSnapshot};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    HtmlReceiveKind, HtmlReceiveView, HtmlSizeAction, HtmlSizeBannerView, HtmlSizeDetail,
    html_receive_state, html_size_banner, inset_banner_zone,
};

pub(crate) fn size_banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    snapshot: &ReceiveSnapshot,
    panel: egui::Rect,
    actions: &mut Vec<ReceiveAction>,
) -> Option<LogicalPx> {
    let size = snapshot.size_notice;
    if size.is_none() && !snapshot.paused {
        return None;
    }
    let bytes = size.unwrap_or(snapshot.received);
    let count = decimal_bytes(bytes);
    let title = if size.is_some() {
        crate::i18n::t("html_remote.size_title").to_owned()
    } else {
        crate::i18n::t_fmt("html_remote.limit_title", &count)
    };
    let waiting = (snapshot.waiting > 0)
        .then(|| crate::i18n::t_fmt("html_remote.waiting", &snapshot.waiting.to_string()));
    let view = HtmlSizeBannerView {
        title: &title,
        detail: if size.is_some() {
            HtmlSizeDetail::File {
                path: &snapshot.address,
                size: &count,
                waiting: waiting.as_deref(),
            }
        } else {
            HtmlSizeDetail::Limit {
                body: crate::i18n::t("html_remote.limit_body"),
            }
        },
        decline: crate::i18n::t(if size.is_some() {
            "html_remote.decline"
        } else {
            "html_remote.stop"
        }),
        allow: crate::i18n::t(if size.is_some() {
            "html_remote.allow"
        } else {
            "html_remote.continue"
        }),
        narrow: LogicalPx(panel.width()) < theme.banner_narrow_below(),
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inset_banner_zone(panel, theme)));
    child.set_clip_rect(ui.clip_rect().intersect(panel));
    let result = html_size_banner(&mut child, theme, &view);
    let kind = match (result.action, size.is_some()) {
        (HtmlSizeAction::Allow, true) => Some(ReceiveActionKind::Allow),
        (HtmlSizeAction::Allow, false) => Some(ReceiveActionKind::Continue),
        (HtmlSizeAction::Decline, true) => Some(ReceiveActionKind::Decline),
        (HtmlSizeAction::Decline, false) => Some(ReceiveActionKind::Stop),
        _ => None,
    };
    if let Some(action) = kind.and_then(|kind| snapshot.action(kind)) {
        actions.push(action);
    }
    Some(LogicalPx(result.rect.height()))
}
pub(crate) fn page(ui: &mut egui::Ui, theme: &Theme, snapshot: &ReceiveSnapshot, rect: egui::Rect) {
    if snapshot.ready {
        return;
    }
    let progress = crate::i18n::t_count(
        "html_remote.progress",
        1,
        &[&decimal_bytes(snapshot.received), "1"],
    );
    let paused = snapshot.paused || snapshot.size_notice.is_some();
    let (kind, title, subtitle) = if !snapshot.connected {
        (
            HtmlReceiveKind::Disconnected,
            crate::i18n::t("html_address.disconnected"),
            crate::i18n::t("html_remote.disconnected_sub"),
        )
    } else if snapshot.receiving {
        (
            if paused {
                HtmlReceiveKind::Paused
            } else {
                HtmlReceiveKind::Receiving
            },
            crate::i18n::t("html_remote.receiving"),
            if paused {
                crate::i18n::t("html_remote.paused")
            } else {
                &progress
            },
        )
    } else {
        return;
    };
    html_receive_state(
        ui,
        theme,
        rect,
        &HtmlReceiveView {
            kind,
            title,
            subtitle,
        },
    );
}

// Transfer limits and the received design use decimal KB/MB.
fn decimal_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
