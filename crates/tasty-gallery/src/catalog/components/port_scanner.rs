//! Port Scanner popup 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/port_scanner.rs::draw_port_scanner_view` 가
//! 표현하는 시각 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만
//! gallery 가 본체 binary 에 의존할 수 없으므로 view 로직은 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 대표 상태:
//! - Empty: 결과 0건 (`no_ports` 메시지).
//! - Few: 결과 3건 (전형적 dev server 패턴).
//! - Many: 결과 30건 (스크롤 영역 검증).
//!
//! Action 은 카탈로그에서 시각 검증 전용이라 표시만 (실행 없음).

use tasty_type_appearance::theme::Theme;

#[derive(Clone, Debug)]
struct PortEntryView {
    port: u16,
    addr_display: String,
    pid: u32,
}

struct PortScannerProps<'a> {
    theme: &'a Theme,
    heading: &'a str,
    no_ports_label: &'a str,
    refresh_label: &'a str,
    hint_label: &'a str,
    entries: &'a [PortEntryView],
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PortScannerAction {
    None,
    Close,
    Refresh,
    OpenEntry(usize),
}

/// 본체 `draw_port_scanner_view` 의 시각 미러 (gallery 측 복제).
fn draw_port_scanner_view(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> PortScannerAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PortScannerAction::Close;
    }

    let th = props.theme;
    let mut action = PortScannerAction::None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        ui.label(
            egui::RichText::new(props.heading)
                .color(egui::Color32::from(th.text))
                .size(13.0),
        );
        ui.separator();

        if props.entries.is_empty() {
            ui.label(
                egui::RichText::new(props.no_ports_label)
                    .color(egui::Color32::from(th.subtext0))
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (i, entry) in props.entries.iter().enumerate() {
                        if draw_port_row(ui, th, entry) {
                            action = PortScannerAction::OpenEntry(i);
                        }
                    }
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(props.refresh_label).clicked() {
                action = PortScannerAction::Refresh;
            }
            ui.label(
                egui::RichText::new(props.hint_label)
                    .color(egui::Color32::from(th.overlay0))
                    .size(11.0),
            );
        });
    });

    action
}

fn draw_port_row(ui: &mut egui::Ui, th: &Theme, entry: &PortEntryView) -> bool {
    let full_width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(full_width, 22.0),
        egui::Sense::click().union(egui::Sense::hover()),
    );
    if resp.hovered() {
        // 본체 코드는 theme.hover_overlay (premultiplied) 사용. gallery 미러는
        // surface1 의 alpha 변형으로 시각 근사.
        let hover = egui::Color32::from(th.surface1);
        ui.painter().rect_filled(rect, 4.0, hover);
    }
    let label = format!(
        "{}  ·  {}  ·  PID {}",
        entry.port, entry.addr_display, entry.pid
    );
    ui.painter().text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        if resp.hovered() {
            egui::Color32::from(th.text)
        } else {
            egui::Color32::from(th.subtext0)
        },
    );
    resp.clicked()
}

fn mock_few() -> Vec<PortEntryView> {
    vec![
        PortEntryView {
            port: 3000,
            addr_display: "0.0.0.0".into(),
            pid: 12345,
        },
        PortEntryView {
            port: 5173,
            addr_display: "127.0.0.1".into(),
            pid: 12346,
        },
        PortEntryView {
            port: 8080,
            addr_display: "[::]".into(),
            pid: 12347,
        },
    ]
}

fn mock_many() -> Vec<PortEntryView> {
    (0..30)
        .map(|i| PortEntryView {
            port: 3000 + i as u16,
            addr_display: if i % 2 == 0 {
                "0.0.0.0".into()
            } else {
                "127.0.0.1".into()
            },
            pid: 12000 + i,
        })
        .collect()
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("draw_port_scanner_view — listening port viewer (3 mock states)")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/popup/port_scanner.rs::draw_port_scanner_popup",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let heading = "Listening ports";
    let no_ports_label = "No listening ports for this surface.";
    let refresh_label = "Refresh";
    let hint_label = "Click a row to open in browser.";

    // Case 1: Empty.
    ui.label(
        egui::RichText::new("Case 1 — Empty (0 entries)")
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            let props = PortScannerProps {
                theme,
                heading,
                no_ports_label,
                refresh_label,
                hint_label,
                entries: &[],
            };
            // 카탈로그는 시각 검증 전용 — action 폐기.
            drop(draw_port_scanner_view(ui, &props));
        });
    ui.add_space(16.0);

    // Case 2: Few entries.
    let few = mock_few();
    ui.label(
        egui::RichText::new("Case 2 — Few (3 entries, typical dev servers)")
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            let props = PortScannerProps {
                theme,
                heading,
                no_ports_label,
                refresh_label,
                hint_label,
                entries: &few,
            };
            // 카탈로그는 시각 검증 전용 — action 폐기.
            drop(draw_port_scanner_view(ui, &props));
        });
    ui.add_space(16.0);

    // Case 3: Many entries — scroll area.
    let many = mock_many();
    ui.label(
        egui::RichText::new("Case 3 — Many (30 entries, scroll active)")
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            let props = PortScannerProps {
                theme,
                heading,
                no_ports_label,
                refresh_label,
                hint_label,
                entries: &many,
            };
            // 카탈로그는 시각 검증 전용 — action 폐기.
            drop(draw_port_scanner_view(ui, &props));
        });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "Note: hover overlay 색은 본체의 theme.hover_overlay (premultiplied) 대신 \
             surface1 로 미러. 본체 wrapper 는 cache TTL 5s + descendant PID scan 으로 \
             결과를 채운다 — 갤러리에선 mock 정적 데이터.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
