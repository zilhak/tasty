//! attach 초기 화면을 VT 바이트로 만들어 mirror의 feed_bytes에 전달한다.
//! 이후 변화는 raw PTY 출력으로 보낸다. 현재 화면·커서·주요 모드만 포함하며
//! 스크롤백은 포함하지 않는다. 색·굵기·dim·italic·밑줄·blink·reverse·invisible·
//! strikethrough·overline을 기록하되 일부 밑줄 모양은 단일 밑줄로 근사한다.

use termwiz::cell::{CellAttributes, Intensity, Underline};
use termwiz::color::ColorAttribute;
use termwiz::surface::Surface as TwSurface;

use crate::TerminalState;

impl TerminalState {
    /// 현재 화면을 mirror가 feed_bytes로 읽을 VT 바이트로 직렬화한다.
    pub fn snapshot_as_vt(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();

        // 모드 복원 — mirror 가 동일 모드를 갖도록 화면 내용보다 먼저 emit.
        // alt-screen 이면 alternate surface 로 전환(이후 self.surface() 가 그 surface).
        if self.is_alternate_screen() {
            out.extend_from_slice(b"\x1b[?1049h");
        }
        if self.application_cursor_keys() {
            out.extend_from_slice(b"\x1b[?1h");
        }
        if self.bracketed_paste() {
            out.extend_from_slice(b"\x1b[?2004h");
        }

        // clear + home, 그리고 SGR 초기화.
        out.extend_from_slice(b"\x1b[2J\x1b[H\x1b[0m");

        let surface: &TwSurface = self.surface();
        let lines = surface.screen_lines();
        // 직전에 emit 한 SGR 문자열(중복 emit 방지). 시작은 reset 상태.
        // SGR 상태는 행을 넘어도 유지되므로(파서가 상태 보존) 루프 밖에서 추적한다.
        let mut prev_sgr = String::from("\x1b[0m");

        for (row, line) in lines.iter().enumerate() {
            // 행마다 절대 위치로 이동(1-based). `\r\n` 은 한 행을 가득 채울 때
            // auto-wrap 으로 다음 행이 밀리므로 절대 커서 주소를 쓴다.
            out.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
            let mut expected_col = 0usize;
            for cell in line.visible_cells() {
                let idx = cell.cell_index();
                // visible_cells 사이의 빈 칸(중간 gap)은 기본 속성 공백으로 패딩.
                if idx > expected_col {
                    if prev_sgr != "\x1b[0m" {
                        out.extend_from_slice(b"\x1b[0m");
                        prev_sgr = String::from("\x1b[0m");
                    }
                    out.resize(out.len() + (idx - expected_col), b' ');
                }
                let sgr = sgr_for(cell.attrs());
                if sgr != prev_sgr {
                    out.extend_from_slice(sgr.as_bytes());
                    prev_sgr = sgr;
                }
                out.extend_from_slice(cell.str().as_bytes());
                let w = cell
                    .str()
                    .chars()
                    .next()
                    .and_then(unicode_width::UnicodeWidthChar::width)
                    .unwrap_or(1)
                    .max(1);
                expected_col = idx + w;
            }
        }

        // 속성 초기화 후 커서 위치 복원(1-based row;col).
        out.extend_from_slice(b"\x1b[0m");
        let (cx, cy) = surface.cursor_position();
        out.extend_from_slice(format!("\x1b[{};{}H", cy + 1, cx + 1).as_bytes());
        if !self.cursor_visible() {
            out.extend_from_slice(b"\x1b[?25l");
        }

        out
    }
}

/// 셀 속성을 reset으로 시작하는 SGR로 변환한다. 호출자가 같은 문자열의 반복을 생략한다.
fn sgr_for(attrs: &CellAttributes) -> String {
    let mut codes: Vec<String> = vec!["0".to_string()];

    match attrs.intensity() {
        Intensity::Normal => {}
        Intensity::Bold => codes.push("1".to_string()),
        Intensity::Half => codes.push("2".to_string()),
    }
    if attrs.italic() {
        codes.push("3".to_string());
    }
    match attrs.underline() {
        Underline::None => {}
        Underline::Double => codes.push("21".to_string()),
        // single/curly/dotted/dashed는 단일 밑줄로 근사한다.
        _ => codes.push("4".to_string()),
    }
    match attrs.blink() {
        termwiz::cell::Blink::None => {}
        termwiz::cell::Blink::Slow => codes.push("5".to_string()),
        termwiz::cell::Blink::Rapid => codes.push("6".to_string()),
    }
    if attrs.reverse() {
        codes.push("7".to_string());
    }
    if attrs.invisible() {
        codes.push("8".to_string());
    }
    if attrs.strikethrough() {
        codes.push("9".to_string());
    }
    if attrs.overline() {
        codes.push("53".to_string());
    }

    push_color(&mut codes, &attrs.foreground(), false);
    push_color(&mut codes, &attrs.background(), true);

    format!("\x1b[{}m", codes.join(";"))
}

/// 색 속성을 SGR 코드로 push. `background=true` 면 배경(40/48/100 계열), 아니면 전경.
fn push_color(codes: &mut Vec<String>, color: &ColorAttribute, background: bool) {
    match color {
        // Default 는 reset(`0`)이 이미 처리.
        ColorAttribute::Default => {}
        ColorAttribute::PaletteIndex(idx) => {
            let idx = *idx;
            if idx < 8 {
                let base = if background { 40 } else { 30 };
                codes.push((base + idx as u16).to_string());
            } else if idx < 16 {
                // bright 8..15 → 90/100 계열.
                let base = if background { 100 } else { 90 };
                codes.push((base + (idx as u16 - 8)).to_string());
            } else {
                let lead = if background { 48 } else { 38 };
                codes.push(format!("{lead};5;{idx}"));
            }
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _)
        | ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
            let r = (srgba.0 * 255.0).round() as u8;
            let g = (srgba.1 * 255.0).round() as u8;
            let b = (srgba.2 * 255.0).round() as u8;
            let lead = if background { 48 } else { 38 };
            codes.push(format!("{lead};2;{r};{g};{b}"));
        }
    }
}
