//! 빌드타임 베이크된 벡터 아이콘을 egui painter 로 그리는 helper.
//!
//! plugin `build.rs` 가 `tasty-icons` 의 [`Icon::svg`](tasty_icons) 를 usvg 로
//! 평탄화해 `pub const <NAME>: &[&[[f32; 2]]]`(서브패스별 폴리라인, viewBox 0..24
//! 절대좌표)를 생성하고, 런타임은 이 점배열을 그릴 크기로 스케일해 벡터 stroke 로
//! 그린다(텍스처 없음·DPI 독립). image / markdown plugin 이 이 렌더 경로를 공유한다.
//!
//! stroke 색은 인자로 받은 `color`(테마 토큰) 를 그대로 쓴다 — 색을 여기서 박지 않는다.

use egui::epaint::PathStroke;
use egui::{Color32, Painter, Pos2, Shape, vec2};

/// viewBox 0..24 점배열(`icon`)을 `size`(logical px) 정사각으로 스케일해 `center` 를
/// 중심으로 벡터 stroke 로 그린다. stroke width 는 SVG 기준 2px 를 동일 비율로 스케일
/// (round cap/join 은 open 폴리라인 tessellation 이 담당).
pub fn draw(painter: &Painter, icon: &[&[[f32; 2]]], center: Pos2, size: f32, color: Color32) {
    let scale = size / 24.0;
    let origin = center - vec2(size * 0.5, size * 0.5);
    let width = 2.0 * scale; // SVG stroke-width 2 @ viewBox 24
    for sub in icon {
        if sub.len() < 2 {
            continue;
        }
        let points: Vec<Pos2> = sub
            .iter()
            .map(|p| origin + vec2(p[0] * scale, p[1] * scale))
            .collect();
        painter.add(Shape::line(points, PathStroke::new(width, color)));
    }
}
