//! Unicode box-drawing 문자 (U+2500..=U+257F) 의 비트맵 렌더링.

use super::{fill_hline, fill_vline};


// ---- box drawing -----------------------------------------------------------

/// Line weight for each of the four directions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lw {
    None,
    Light,
    Heavy,
    Double,
}

/// Describes the four arms of a box-drawing character.
struct BoxDesc {
    left: Lw,
    right: Lw,
    up: Lw,
    down: Lw,
}

impl BoxDesc {
    const fn new(left: Lw, right: Lw, up: Lw, down: Lw) -> Self {
        Self {
            left,
            right,
            up,
            down,
        }
    }
}

#[allow(clippy::enum_glob_use)]
fn box_desc(cp: u32) -> Option<BoxDesc> {
    use Lw::*;
    let d = match cp {
        // ── Single horizontal / vertical ──
        0x2500 => BoxDesc::new(Light, Light, None, None), // ─
        0x2501 => BoxDesc::new(Heavy, Heavy, None, None), // ━
        0x2502 => BoxDesc::new(None, None, Light, Light), // │
        0x2503 => BoxDesc::new(None, None, Heavy, Heavy), // ┃

        // ── Dashed variants (rendered as their non-dashed equivalents) ──
        0x2504 => BoxDesc::new(Light, Light, None, None), // ┄ (triple dash horizontal light)
        0x2505 => BoxDesc::new(Heavy, Heavy, None, None), // ┅ (triple dash horizontal heavy)
        0x2506 => BoxDesc::new(None, None, Light, Light), // ┆ (triple dash vertical light)
        0x2507 => BoxDesc::new(None, None, Heavy, Heavy), // ┇ (triple dash vertical heavy)
        0x2508 => BoxDesc::new(Light, Light, None, None), // ┈ (quadruple dash horizontal light)
        0x2509 => BoxDesc::new(Heavy, Heavy, None, None), // ┉ (quadruple dash horizontal heavy)
        0x250A => BoxDesc::new(None, None, Light, Light), // ┊ (quadruple dash vertical light)
        0x250B => BoxDesc::new(None, None, Heavy, Heavy), // ┋ (quadruple dash vertical heavy)

        // ── Corners ──
        0x250C => BoxDesc::new(None, Light, None, Light), // ┌
        0x250D => BoxDesc::new(None, Heavy, None, Light), // ┍
        0x250E => BoxDesc::new(None, Light, None, Heavy), // ┎
        0x250F => BoxDesc::new(None, Heavy, None, Heavy), // ┏

        0x2510 => BoxDesc::new(Light, None, None, Light), // ┐
        0x2511 => BoxDesc::new(Heavy, None, None, Light), // ┑
        0x2512 => BoxDesc::new(Light, None, None, Heavy), // ┒
        0x2513 => BoxDesc::new(Heavy, None, None, Heavy), // ┓

        0x2514 => BoxDesc::new(None, Light, Light, None), // └
        0x2515 => BoxDesc::new(None, Heavy, Light, None), // ┕
        0x2516 => BoxDesc::new(None, Light, Heavy, None), // ┖
        0x2517 => BoxDesc::new(None, Heavy, Heavy, None), // ┗

        0x2518 => BoxDesc::new(Light, None, Light, None), // ┘
        0x2519 => BoxDesc::new(Heavy, None, Light, None), // ┙
        0x251A => BoxDesc::new(Light, None, Heavy, None), // ┚
        0x251B => BoxDesc::new(Heavy, None, Heavy, None), // ┛

        // ── T-junctions ──
        0x251C => BoxDesc::new(None, Light, Light, Light), // ├
        0x251D => BoxDesc::new(None, Heavy, Light, Light), // ┝
        0x251E => BoxDesc::new(None, Light, Heavy, Light), // ┞
        0x251F => BoxDesc::new(None, Light, Light, Heavy), // ┟
        0x2520 => BoxDesc::new(None, Light, Heavy, Heavy), // ┠
        0x2521 => BoxDesc::new(None, Heavy, Heavy, Light), // ┡
        0x2522 => BoxDesc::new(None, Heavy, Light, Heavy), // ┢
        0x2523 => BoxDesc::new(None, Heavy, Heavy, Heavy), // ┣

        0x2524 => BoxDesc::new(Light, None, Light, Light), // ┤
        0x2525 => BoxDesc::new(Heavy, None, Light, Light), // ┥
        0x2526 => BoxDesc::new(Light, None, Heavy, Light), // ┦
        0x2527 => BoxDesc::new(Light, None, Light, Heavy), // ┧
        0x2528 => BoxDesc::new(Light, None, Heavy, Heavy), // ┨
        0x2529 => BoxDesc::new(Heavy, None, Heavy, Light), // ┩
        0x252A => BoxDesc::new(Heavy, None, Light, Heavy), // ┪
        0x252B => BoxDesc::new(Heavy, None, Heavy, Heavy), // ┫

        0x252C => BoxDesc::new(Light, Light, None, Light), // ┬
        0x252D => BoxDesc::new(Heavy, Light, None, Light), // ┭
        0x252E => BoxDesc::new(Light, Heavy, None, Light), // ┮
        0x252F => BoxDesc::new(Heavy, Heavy, None, Light), // ┯
        0x2530 => BoxDesc::new(Light, Light, None, Heavy), // ┰
        0x2531 => BoxDesc::new(Heavy, Light, None, Heavy), // ┱
        0x2532 => BoxDesc::new(Light, Heavy, None, Heavy), // ┲
        0x2533 => BoxDesc::new(Heavy, Heavy, None, Heavy), // ┳

        0x2534 => BoxDesc::new(Light, Light, Light, None), // ┴
        0x2535 => BoxDesc::new(Heavy, Light, Light, None), // ┵
        0x2536 => BoxDesc::new(Light, Heavy, Light, None), // ┶
        0x2537 => BoxDesc::new(Heavy, Heavy, Light, None), // ┷
        0x2538 => BoxDesc::new(Light, Light, Heavy, None), // ┸
        0x2539 => BoxDesc::new(Heavy, Light, Heavy, None), // ┹
        0x253A => BoxDesc::new(Light, Heavy, Heavy, None), // ┺
        0x253B => BoxDesc::new(Heavy, Heavy, Heavy, None), // ┻

        // ── Crosses ──
        0x253C => BoxDesc::new(Light, Light, Light, Light), // ┼
        0x253D => BoxDesc::new(Heavy, Light, Light, Light), // ┽
        0x253E => BoxDesc::new(Light, Heavy, Light, Light), // ┾
        0x253F => BoxDesc::new(Heavy, Heavy, Light, Light), // ┿
        0x2540 => BoxDesc::new(Light, Light, Heavy, Light), // ╀
        0x2541 => BoxDesc::new(Light, Light, Light, Heavy), // ╁
        0x2542 => BoxDesc::new(Light, Light, Heavy, Heavy), // ╂
        0x2543 => BoxDesc::new(Heavy, Light, Heavy, Light), // ╃
        0x2544 => BoxDesc::new(Light, Heavy, Heavy, Light), // ╄
        0x2545 => BoxDesc::new(Heavy, Light, Light, Heavy), // ╅
        0x2546 => BoxDesc::new(Light, Heavy, Light, Heavy), // ╆
        0x2547 => BoxDesc::new(Heavy, Heavy, Heavy, Light), // ╇
        0x2548 => BoxDesc::new(Heavy, Heavy, Light, Heavy), // ╈
        0x2549 => BoxDesc::new(Heavy, Light, Heavy, Heavy), // ╉
        0x254A => BoxDesc::new(Light, Heavy, Heavy, Heavy), // ╊
        0x254B => BoxDesc::new(Heavy, Heavy, Heavy, Heavy), // ╋

        // ── More dashed variants (treat as non-dashed) ──
        0x254C => BoxDesc::new(Light, Light, None, None), // ╌
        0x254D => BoxDesc::new(Heavy, Heavy, None, None), // ╍
        0x254E => BoxDesc::new(None, None, Light, Light), // ╎
        0x254F => BoxDesc::new(None, None, Heavy, Heavy), // ╏

        // ── Double lines ──
        0x2550 => BoxDesc::new(Double, Double, None, None), // ═
        0x2551 => BoxDesc::new(None, None, Double, Double), // ║

        // ── Double corners ──
        0x2552 => BoxDesc::new(None, Double, None, Light), // ╒
        0x2553 => BoxDesc::new(None, Light, None, Double), // ╓
        0x2554 => BoxDesc::new(None, Double, None, Double), // ╔
        0x2555 => BoxDesc::new(Double, None, None, Light), // ╕
        0x2556 => BoxDesc::new(Light, None, None, Double), // ╖
        0x2557 => BoxDesc::new(Double, None, None, Double), // ╗
        0x2558 => BoxDesc::new(None, Double, Light, None), // ╘
        0x2559 => BoxDesc::new(None, Light, Double, None), // ╙
        0x255A => BoxDesc::new(None, Double, Double, None), // ╚
        0x255B => BoxDesc::new(Double, None, Light, None), // ╛
        0x255C => BoxDesc::new(Light, None, Double, None), // ╜
        0x255D => BoxDesc::new(Double, None, Double, None), // ╝

        // ── Double T-junctions ──
        0x255E => BoxDesc::new(None, Double, Light, Light), // ╞
        0x255F => BoxDesc::new(None, Light, Double, Double), // ╟
        0x2560 => BoxDesc::new(None, Double, Double, Double), // ╠
        0x2561 => BoxDesc::new(Double, None, Light, Light), // ╡
        0x2562 => BoxDesc::new(Light, None, Double, Double), // ╢
        0x2563 => BoxDesc::new(Double, None, Double, Double), // ╣
        0x2564 => BoxDesc::new(Double, Double, None, Light), // ╤
        0x2565 => BoxDesc::new(Light, Light, None, Double), // ╥
        0x2566 => BoxDesc::new(Double, Double, None, Double), // ╦
        0x2567 => BoxDesc::new(Double, Double, Light, None), // ╧
        0x2568 => BoxDesc::new(Light, Light, Double, None), // ╨
        0x2569 => BoxDesc::new(Double, Double, Double, None), // ╩
        // ── Double crosses ──
        0x256A => BoxDesc::new(Double, Double, Light, Light), // ╪
        0x256B => BoxDesc::new(Light, Light, Double, Double), // ╫
        0x256C => BoxDesc::new(Double, Double, Double, Double), // ╬

        // ── Rounded corners (light) ──
        0x256D => BoxDesc::new(None, Light, None, Light), // ╭
        0x256E => BoxDesc::new(Light, None, None, Light), // ╮
        0x256F => BoxDesc::new(Light, None, Light, None), // ╯
        0x2570 => BoxDesc::new(None, Light, Light, None), // ╰

        // ── Diagonal lines (render as light cross for approximation) ──
        0x2571 => BoxDesc::new(None, None, None, None), // ╱ (handled specially)
        0x2572 => BoxDesc::new(None, None, None, None), // ╲ (handled specially)
        0x2573 => BoxDesc::new(None, None, None, None), // ╳ (handled specially)

        // ── Half lines ──
        0x2574 => BoxDesc::new(Light, None, None, None), // ╴ left light
        0x2575 => BoxDesc::new(None, None, Light, None), // ╵ up light
        0x2576 => BoxDesc::new(None, Light, None, None), // ╶ right light
        0x2577 => BoxDesc::new(None, None, None, Light), // ╷ down light
        0x2578 => BoxDesc::new(Heavy, None, None, None), // ╸ left heavy
        0x2579 => BoxDesc::new(None, None, Heavy, None), // ╹ up heavy
        0x257A => BoxDesc::new(None, Heavy, None, None), // ╺ right heavy
        0x257B => BoxDesc::new(None, None, None, Heavy), // ╻ down heavy

        // ── Mixed weight lines ──
        0x257C => BoxDesc::new(Light, Heavy, None, None), // ╼ light left, heavy right
        0x257D => BoxDesc::new(None, None, Light, Heavy), // ╽ light up, heavy down
        0x257E => BoxDesc::new(Heavy, Light, None, None), // ╾ heavy left, light right
        0x257F => BoxDesc::new(None, None, Heavy, Light), // ╿ heavy up, light down

        _ => return ::core::option::Option::None,
    };
    Some(d)
}

pub(super) fn draw_box_drawing(cp: u32, bitmap: &mut [u8], w: u32, h: u32) -> bool {
    // Handle diagonal lines specially
    match cp {
        0x2571 => {
            draw_diagonal_forward(bitmap, w, h);
            return true;
        }
        0x2572 => {
            draw_diagonal_back(bitmap, w, h);
            return true;
        }
        0x2573 => {
            draw_diagonal_forward(bitmap, w, h);
            draw_diagonal_back(bitmap, w, h);
            return true;
        }
        _ => {}
    }

    let desc = match box_desc(cp) {
        Some(d) => d,
        None => return false,
    };

    let cx = w / 2;
    let cy = h / 2;

    // Line thickness
    let light_h = (w / 8).max(1); // horizontal light thickness (vertical extent)
    let heavy_h = (w / 4).max(2); // horizontal heavy thickness
    let light_v = (w / 8).max(1); // vertical light thickness (horizontal extent)
    let heavy_v = (w / 4).max(2); // vertical heavy thickness
    let double_gap = (w / 6).max(2); // gap between double lines (center-to-center distance)

    // Draw each arm
    // LEFT arm
    match desc.left {
        Lw::None => {}
        Lw::Light => fill_hline(bitmap, w, h, 0, cx + light_v / 2, cy, light_h),
        Lw::Heavy => fill_hline(bitmap, w, h, 0, cx + heavy_v / 2, cy, heavy_h),
        Lw::Double => {
            let offset = double_gap / 2;
            fill_hline(
                bitmap,
                w,
                h,
                0,
                cx + light_v / 2,
                cy.saturating_sub(offset),
                light_h,
            );
            fill_hline(
                bitmap,
                w,
                h,
                0,
                cx + light_v / 2,
                (cy + offset).min(h - 1),
                light_h,
            );
        }
    }

    // RIGHT arm
    match desc.right {
        Lw::None => {}
        Lw::Light => fill_hline(bitmap, w, h, cx.saturating_sub(light_v / 2), w, cy, light_h),
        Lw::Heavy => fill_hline(bitmap, w, h, cx.saturating_sub(heavy_v / 2), w, cy, heavy_h),
        Lw::Double => {
            let offset = double_gap / 2;
            fill_hline(
                bitmap,
                w,
                h,
                cx.saturating_sub(light_v / 2),
                w,
                cy.saturating_sub(offset),
                light_h,
            );
            fill_hline(
                bitmap,
                w,
                h,
                cx.saturating_sub(light_v / 2),
                w,
                (cy + offset).min(h - 1),
                light_h,
            );
        }
    }

    // UP arm
    match desc.up {
        Lw::None => {}
        Lw::Light => fill_vline(bitmap, w, h, 0, cy + light_h / 2, cx, light_v),
        Lw::Heavy => fill_vline(bitmap, w, h, 0, cy + heavy_h / 2, cx, heavy_v),
        Lw::Double => {
            let offset = double_gap / 2;
            fill_vline(
                bitmap,
                w,
                h,
                0,
                cy + light_h / 2,
                cx.saturating_sub(offset),
                light_v,
            );
            fill_vline(
                bitmap,
                w,
                h,
                0,
                cy + light_h / 2,
                (cx + offset).min(w - 1),
                light_v,
            );
        }
    }

    // DOWN arm
    match desc.down {
        Lw::None => {}
        Lw::Light => fill_vline(bitmap, w, h, cy.saturating_sub(light_h / 2), h, cx, light_v),
        Lw::Heavy => fill_vline(bitmap, w, h, cy.saturating_sub(heavy_h / 2), h, cx, heavy_v),
        Lw::Double => {
            let offset = double_gap / 2;
            fill_vline(
                bitmap,
                w,
                h,
                cy.saturating_sub(light_h / 2),
                h,
                cx.saturating_sub(offset),
                light_v,
            );
            fill_vline(
                bitmap,
                w,
                h,
                cy.saturating_sub(light_h / 2),
                h,
                (cx + offset).min(w - 1),
                light_v,
            );
        }
    }

    true
}

/// Draw a forward diagonal line ╱ (bottom-left to top-right).
fn draw_diagonal_forward(bitmap: &mut [u8], w: u32, h: u32) {
    let thickness = (w / 8).max(1);
    for py in 0..h {
        // Map py to x: when py=0 → x=w-1, when py=h-1 → x=0
        let fx = (h - 1 - py) as f32 * (w as f32 - 1.0) / (h as f32 - 1.0).max(1.0);
        let cx = fx.round() as u32;
        let half = thickness / 2;
        let x0 = cx.saturating_sub(half);
        let x1 = (cx + thickness - half).min(w);
        for px in x0..x1 {
            let idx = (py * w + px) as usize;
            if idx < bitmap.len() {
                bitmap[idx] = 255;
            }
        }
    }
}

/// Draw a backward diagonal line ╲ (top-left to bottom-right).
fn draw_diagonal_back(bitmap: &mut [u8], w: u32, h: u32) {
    let thickness = (w / 8).max(1);
    for py in 0..h {
        let fx = py as f32 * (w as f32 - 1.0) / (h as f32 - 1.0).max(1.0);
        let cx = fx.round() as u32;
        let half = thickness / 2;
        let x0 = cx.saturating_sub(half);
        let x1 = (cx + thickness - half).min(w);
        for px in x0..x1 {
            let idx = (py * w + px) as usize;
            if idx < bitmap.len() {
                bitmap[idx] = 255;
            }
        }
    }
}

