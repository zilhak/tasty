//! `UiNode` → egui 위젯 렌더링.
//!
//! 사용자 입력은 `RemoteSurface::pending_events`로 push 되어 다음 pump tick에 plugin에 송신.

use egui::Ui;

use crate::plugin::remote_surface::RemoteSurface;
use crate::plugin::ui_tree::{
    ButtonStyle, LabelStyle, SplitDir, TreeNode, UiEvent, UiNode,
};

pub fn render_remote_surface(ui: &mut Ui, surface: &RemoteSurface) {
    let tree_opt = surface.tree.lock().ok().and_then(|t| t.clone());
    match tree_opt {
        Some(node) => render_node(ui, &node, surface),
        None => {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(format!("Loading plugin surface: {}", surface.kind_static))
                        .italics(),
                );
                ui.label(
                    egui::RichText::new(&surface.plugin_id)
                        .small()
                        .color(egui::Color32::DARK_GRAY),
                );
            });
        }
    }
}

fn render_node(ui: &mut Ui, node: &UiNode, surface: &RemoteSurface) {
    match node {
        UiNode::Vbox { spacing, children } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = *spacing as f32;
                for c in children {
                    render_node(ui, c, surface);
                }
            });
        }
        UiNode::Hbox { spacing, children } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = *spacing as f32;
                for c in children {
                    render_node(ui, c, surface);
                }
            });
        }
        UiNode::Scroll {
            vertical,
            horizontal,
            child,
        } => {
            egui::ScrollArea::new([*horizontal, *vertical]).show(ui, |ui| {
                render_node(ui, child, surface);
            });
        }
        UiNode::Splitter {
            direction,
            ratio,
            first,
            second,
        } => {
            // 단순 구현: 가용 영역을 비율로 나눠 두 child를 배치.
            // egui native splitter 위젯이 없으므로 두 영역을 allocate.
            let avail = ui.available_size();
            let r = ratio.clamp(0.05, 0.95);
            match direction {
                SplitDir::Horizontal => {
                    ui.horizontal(|ui| {
                        ui.allocate_ui(
                            egui::vec2(avail.x * r, avail.y),
                            |ui| render_node(ui, first, surface),
                        );
                        ui.allocate_ui(
                            egui::vec2(avail.x * (1.0 - r), avail.y),
                            |ui| render_node(ui, second, surface),
                        );
                    });
                }
                SplitDir::Vertical => {
                    ui.vertical(|ui| {
                        ui.allocate_ui(
                            egui::vec2(avail.x, avail.y * r),
                            |ui| render_node(ui, first, surface),
                        );
                        ui.allocate_ui(
                            egui::vec2(avail.x, avail.y * (1.0 - r)),
                            |ui| render_node(ui, second, surface),
                        );
                    });
                }
            }
        }
        UiNode::Label { text, style, color } => {
            let mut rt = egui::RichText::new(text);
            rt = match style {
                LabelStyle::Body => rt,
                LabelStyle::Caption => rt.small(),
                LabelStyle::Heading => rt.heading(),
                LabelStyle::Dim => rt.color(egui::Color32::GRAY),
            };
            if let Some(c) = color.as_deref() {
                if let Some(parsed) = parse_color_token(c) {
                    rt = rt.color(parsed);
                }
            }
            ui.label(rt);
        }
        UiNode::Icon { name } => {
            ui.label(name);
        }
        UiNode::Button {
            id,
            label,
            enabled,
            style,
            tooltip_i18n_key,
        } => {
            let mut btn = egui::Button::new(label);
            if let ButtonStyle::Primary = style {
                btn = btn.fill(egui::Color32::from_rgb(48, 92, 222));
            }
            let resp = ui.add_enabled(*enabled, btn);
            let resp = match tooltip_i18n_key {
                Some(key) => resp.on_hover_text(crate::i18n::t(key)),
                None => resp,
            };
            if resp.clicked() {
                surface.push_event(UiEvent::Click {
                    node_id: id.clone(),
                });
            }
        }
        UiNode::Tree {
            id,
            nodes,
            selection_mode: _,
        } => {
            for n in nodes {
                render_tree_node(ui, id, n, "", surface);
            }
        }
        UiNode::Addressbar {
            id,
            text,
            placeholder_i18n_key,
        } => {
            let mut buf = text.clone();
            let placeholder = placeholder_i18n_key
                .as_ref()
                .map(|k| crate::i18n::t(k))
                .unwrap_or_default();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(placeholder)
                    .desired_width(f32::INFINITY),
            );
            if resp.changed() {
                surface.push_event(UiEvent::AddressbarChange {
                    node_id: id.clone(),
                    text: buf.clone(),
                });
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                surface.push_event(UiEvent::AddressbarSubmit {
                    node_id: id.clone(),
                    text: buf,
                });
            }
        }
        UiNode::TextPreview {
            content,
            language: _,
        } => {
            // syntax highlighting은 향후 — 현 단계는 plain monospace.
            ui.add(egui::Label::new(egui::RichText::new(content).monospace()).wrap());
        }
        UiNode::Spacer { size } => {
            ui.add_space(*size as f32);
        }
    }
}

fn render_tree_node(
    ui: &mut Ui,
    tree_id: &str,
    n: &TreeNode,
    parent_path: &str,
    surface: &RemoteSurface,
) {
    let path = if parent_path.is_empty() {
        n.id.clone()
    } else {
        format!("{parent_path}/{}", n.id)
    };
    let label = match &n.icon {
        Some(icon) => format!("{icon} {}", n.label),
        None => n.label.clone(),
    };
    if n.children.is_empty() {
        if ui.selectable_label(n.selected, label).clicked() {
            surface.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
    } else {
        let id_source = ui.make_persistent_id((tree_id, path.as_str()));
        let resp = egui::CollapsingHeader::new(label)
            .id_salt(id_source)
            .default_open(n.expanded)
            .show(ui, |ui| {
                for child in &n.children {
                    render_tree_node(ui, tree_id, child, &path, surface);
                }
            });
        if resp.header_response.clicked() {
            // 헤더 영역 클릭 자체로 selection 신호.
            surface.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
        // 사용자가 펼침/접음 토글 시 plugin에 알림. 단, immediate-mode 한계로
        // 실제 internal state와 plugin 측 expanded 상태가 어긋날 수 있음.
        // 단계 06+에서 메모리화 개선.
        let now_open = resp.openness > 0.5;
        if now_open != n.expanded {
            surface.push_event(UiEvent::TreeExpand {
                node_id: tree_id.to_string(),
                path: path.clone(),
                expanded: now_open,
            });
        }
    }
}

fn parse_color_token(token: &str) -> Option<egui::Color32> {
    if let Some(stripped) = token.strip_prefix('#') {
        if stripped.len() == 6 {
            let r = u8::from_str_radix(&stripped[0..2], 16).ok()?;
            let g = u8::from_str_radix(&stripped[2..4], 16).ok()?;
            let b = u8::from_str_radix(&stripped[4..6], 16).ok()?;
            return Some(egui::Color32::from_rgb(r, g, b));
        }
    }
    Some(match token {
        "text" => egui::Color32::from_rgb(205, 214, 244),
        "subtext1" => egui::Color32::from_rgb(186, 194, 222),
        "subtext0" => egui::Color32::from_rgb(166, 173, 200),
        "blue" => egui::Color32::from_rgb(137, 180, 250),
        "green" => egui::Color32::from_rgb(166, 227, 161),
        "red" => egui::Color32::from_rgb(243, 139, 168),
        "yellow" => egui::Color32::from_rgb(249, 226, 175),
        _ => return None,
    })
}
