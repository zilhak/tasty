/// Simple Markdown renderer using egui.
/// Handles headings, lists, blockquotes, separators, code blocks, and paragraphs.

use crate::theme;

/// Render markdown content into an egui Ui.
pub fn render_markdown(ui: &mut egui::Ui, content: &str) {
    let th = theme::theme();
    let mut in_code_block = false;
    let mut code_buf = String::new();

    for line in content.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End code block: render accumulated code
                if !code_buf.is_empty() {
                    egui::Frame::new()
                        .fill(th.surface0)
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&code_buf)
                                    .monospace()
                                    .size(12.0)
                                    .color(th.subtext1),
                            );
                        });
                    code_buf.clear();
                }
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(line);
            continue;
        }

        // Headings (check ### before ## before #)
        if line.starts_with("### ") {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&line[4..])
                    .strong()
                    .size(16.0)
                    .color(th.text),
            );
            ui.add_space(2.0);
        } else if line.starts_with("## ") {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&line[3..])
                    .strong()
                    .size(20.0)
                    .color(th.text),
            );
            ui.add_space(3.0);
        } else if line.starts_with("# ") {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(&line[2..])
                    .strong()
                    .size(24.0)
                    .color(th.text),
            );
            ui.add_space(4.0);
        } else if line.starts_with("- ") || line.starts_with("* ") {
            ui.horizontal(|ui| {
                ui.label("  \u{2022}");
                render_inline_markdown(ui, &line[2..]);
            });
        } else if line.starts_with("> ") {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{2502}")
                        .color(th.overlay0),
                );
                ui.label(
                    egui::RichText::new(&line[2..])
                        .italics()
                        .color(th.subtext0),
                );
            });
        } else if line.starts_with("---") || line.starts_with("***") || line.starts_with("___") {
            ui.separator();
        } else if line.is_empty() {
            ui.add_space(6.0);
        } else if line.starts_with("| ") {
            // Table row: render as monospace
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(12.0)
                    .color(th.subtext0),
            );
        } else {
            render_inline_markdown(ui, line);
        }
    }

    // Handle unclosed code block
    if in_code_block && !code_buf.is_empty() {
        egui::Frame::new()
            .fill(th.surface0)
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&code_buf)
                        .monospace()
                        .size(12.0)
                        .color(th.subtext1),
                );
            });
    }
}

/// Render a line with simple inline formatting: `code`, **bold**, *italic*.
fn render_inline_markdown(ui: &mut egui::Ui, text: &str) {
    let th = theme::theme();
    let mut job = egui::text::LayoutJob::default();
    let default_color = th.subtext1;
    let code_color = th.green;
    let bold_color = th.text;
    let italic_color = th.subtext1;

    let mut chars = text.char_indices().peekable();
    let mut segment_start = 0;
    let mut segments: Vec<(&str, TextStyle)> = Vec::new();

    #[derive(Clone, Copy)]
    enum TextStyle {
        Normal,
        Code,
        Bold,
        Italic,
    }

    while let Some(&(i, ch)) = chars.peek() {
        if ch == '`' {
            // Flush normal text before this
            if i > segment_start {
                segments.push((&text[segment_start..i], TextStyle::Normal));
            }
            chars.next();
            // Find closing backtick
            let code_start = i + 1;
            let mut code_end = code_start;
            while let Some(&(j, c)) = chars.peek() {
                if c == '`' {
                    code_end = j;
                    chars.next();
                    break;
                }
                code_end = j + c.len_utf8();
                chars.next();
            }
            if code_end > code_start {
                segments.push((&text[code_start..code_end], TextStyle::Code));
            }
            segment_start = code_end + 1;
        } else if ch == '*' {
            chars.next();
            // Check for ** (bold)
            if let Some(&(_, '*')) = chars.peek() {
                // Bold
                if i > segment_start {
                    segments.push((&text[segment_start..i], TextStyle::Normal));
                }
                chars.next();
                let bold_start = i + 2;
                let mut bold_end = bold_start;
                while let Some(&(j, c)) = chars.peek() {
                    if c == '*' {
                        chars.next();
                        if let Some(&(_, '*')) = chars.peek() {
                            bold_end = j;
                            chars.next();
                            break;
                        }
                    }
                    bold_end = j + c.len_utf8();
                    chars.next();
                }
                if bold_end > bold_start {
                    segments.push((&text[bold_start..bold_end], TextStyle::Bold));
                }
                segment_start = bold_end + 2;
            } else {
                // Italic
                if i > segment_start {
                    segments.push((&text[segment_start..i], TextStyle::Normal));
                }
                let italic_start = i + 1;
                let mut italic_end = italic_start;
                while let Some(&(j, c)) = chars.peek() {
                    if c == '*' {
                        italic_end = j;
                        chars.next();
                        break;
                    }
                    italic_end = j + c.len_utf8();
                    chars.next();
                }
                if italic_end > italic_start {
                    segments.push((&text[italic_start..italic_end], TextStyle::Italic));
                }
                segment_start = italic_end + 1;
            }
        } else {
            chars.next();
        }
    }

    // Flush remaining text
    if segment_start < text.len() {
        segments.push((&text[segment_start..], TextStyle::Normal));
    }

    // Build LayoutJob from segments
    let font_id = egui::FontId::proportional(14.0);
    let mono_font = egui::FontId::monospace(13.0);

    for (s, style) in &segments {
        let (color, font, is_bold, is_italic) = match style {
            TextStyle::Normal => (default_color, font_id.clone(), false, false),
            TextStyle::Code => (code_color, mono_font.clone(), false, false),
            TextStyle::Bold => (bold_color, font_id.clone(), true, false),
            TextStyle::Italic => (italic_color, font_id.clone(), false, true),
        };
        let mut text_format = egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        };
        if is_bold {
            // egui doesn't have bold in TextFormat directly, but we set the color stronger
            text_format.color = bold_color;
        }
        if is_italic {
            text_format.italics = true;
        }
        if matches!(style, TextStyle::Code) {
            text_format.background = th.surface0;
        }
        job.append(s, 0.0, text_format);
    }

    if !job.text.is_empty() {
        ui.label(job);
    }
}
