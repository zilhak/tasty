//! 1px 점선 테두리. 대시는 곧은 변에만 두고 모서리는 실선 호로 그린다.
//! 변마다 대시 수를 정한 뒤 남는 길이를 간격에 고르게 나눠, 각 변이 대시로 시작해 대시로 끝나고
//! 무늬가 변 가운데를 기준으로 대칭이 되게 한다. 대시·간격은 `border_dash`·`border_dash_gap`이다.

use tasty_type_appearance::theme::Theme;

/// 호 한 개를 근사하는 선분 수.
const ARC_SEGMENTS: usize = 6;

/// 길이 `len`의 곧은 변에 놓을 대시 구간(시작, 끝)을 돌려준다.
/// 대시는 최소 간격 `gap`을 지키는 최대 개수이며 간격은 남는 길이만큼 넓어진다.
/// 변이 대시 하나보다 짧으면 변 전체를 대시 하나로 덮는다.
fn dash_spans(len: f32, dash: f32, gap: f32) -> Vec<(f32, f32)> {
    if len <= 0.0 {
        return Vec::new();
    }
    if len <= dash || dash <= 0.0 {
        return vec![(0.0, len)];
    }
    let count = (((len + gap) / (dash + gap)).floor() as usize).max(1);
    if count == 1 {
        let start = (len - dash) * 0.5;
        return vec![(start, start + dash)];
    }
    let spread = (len - count as f32 * dash) / (count - 1) as f32;
    (0..count)
        .map(|i| {
            let start = i as f32 * (dash + spread);
            (start, start + dash)
        })
        .collect()
}

/// `center`를 중심으로 `from`에서 `to` 라디안까지의 호를 선분 점으로 만든다.
fn arc_points(center: egui::Pos2, radius: f32, from: f32, to: f32) -> Vec<egui::Pos2> {
    (0..=ARC_SEGMENTS)
        .map(|i| {
            let a = from + (to - from) * i as f32 / ARC_SEGMENTS as f32;
            center + radius * egui::vec2(a.cos(), a.sin())
        })
        .collect()
}

/// `rect` 안쪽에 1px 점선 테두리를 그린다. 선 전체를 rect 안에 두고 모서리 반경은 `radius`다.
pub fn paint_dashed_outline(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    radius: f32,
    color: egui::Color32,
) {
    let width = theme.border_width.value();
    if rect.width() < width || rect.height() < width {
        return;
    }
    let stroke = egui::Stroke::new(width, color);
    let r = rect.shrink(width * 0.5);
    let radius = radius.clamp(0.0, (r.width().min(r.height()) * 0.5).max(0.0));
    let dash = theme.border_dash.value();
    let gap = theme.border_dash_gap.value();
    // (시작점, 방향, 길이) — 위·오른쪽·아래·왼쪽 변. 모서리 호를 뺀 곧은 부분이다.
    let edges = [
        (
            egui::pos2(r.min.x + radius, r.min.y),
            egui::Vec2::X,
            r.width() - 2.0 * radius,
        ),
        (
            egui::pos2(r.max.x, r.min.y + radius),
            egui::Vec2::Y,
            r.height() - 2.0 * radius,
        ),
        (
            egui::pos2(r.max.x - radius, r.max.y),
            -egui::Vec2::X,
            r.width() - 2.0 * radius,
        ),
        (
            egui::pos2(r.min.x, r.max.y - radius),
            -egui::Vec2::Y,
            r.height() - 2.0 * radius,
        ),
    ];
    for (origin, dir, len) in edges {
        for (a, b) in dash_spans(len, dash, gap) {
            painter.line_segment([origin + dir * a, origin + dir * b], stroke);
        }
    }
    if radius <= 0.0 {
        return;
    }
    use std::f32::consts::{FRAC_PI_2, PI};
    let corners = [
        (
            egui::pos2(r.max.x - radius, r.min.y + radius),
            -FRAC_PI_2,
            0.0,
        ),
        (
            egui::pos2(r.max.x - radius, r.max.y - radius),
            0.0,
            FRAC_PI_2,
        ),
        (
            egui::pos2(r.min.x + radius, r.max.y - radius),
            FRAC_PI_2,
            PI,
        ),
        (
            egui::pos2(r.min.x + radius, r.min.y + radius),
            PI,
            PI + FRAC_PI_2,
        ),
    ];
    for (center, from, to) in corners {
        painter.add(egui::Shape::line(
            arc_points(center, radius, from, to),
            stroke,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::dash_spans;

    /// 각 변은 대시로 시작해 대시로 끝나고 대시 길이는 그대로다.
    #[test]
    fn spans_start_and_end_on_a_dash() {
        for len in [20.0_f32, 37.0, 100.0, 333.0] {
            let spans = dash_spans(len, 4.0, 4.0);
            assert!(spans.len() >= 2, "{len}");
            assert_eq!(spans[0].0, 0.0, "{len}");
            let last = spans[spans.len() - 1].1;
            assert!((last - len).abs() < 1e-3, "{len}: {last}");
            for (a, b) in &spans {
                assert!((b - a - 4.0).abs() < 1e-3, "{len}");
            }
        }
    }

    /// 간격은 최소 간격보다 좁아지지 않고 모두 같다.
    #[test]
    fn gaps_never_shrink_below_the_token_and_stay_even() {
        let spans = dash_spans(37.0, 4.0, 4.0);
        let gaps: Vec<f32> = spans.windows(2).map(|w| w[1].0 - w[0].1).collect();
        for g in &gaps {
            assert!(*g >= 4.0 - 1e-3, "{g}");
            assert!((g - gaps[0]).abs() < 1e-3, "{gaps:?}");
        }
    }

    /// 대시 하나 길이보다 짧은 변은 실선 하나로, 대시 하나만 들어가는 변은 가운데에 둔다.
    #[test]
    fn short_edges_keep_one_centred_dash() {
        assert_eq!(dash_spans(3.0, 4.0, 4.0), vec![(0.0, 3.0)]);
        assert_eq!(dash_spans(10.0, 4.0, 4.0), vec![(3.0, 7.0)]);
        assert!(dash_spans(0.0, 4.0, 4.0).is_empty());
    }
}
