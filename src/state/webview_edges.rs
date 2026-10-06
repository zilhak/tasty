//! native WebView 배치와 입력 영역. native 창은 마우스를 직접 받으므로 분할선 드래그와 창 가장자리
//! 리사이즈 입력을 가리지 않게 해야 한다. Linux는 WebView를 surface에 꽉 채우고 그 입력 영역만
//! input shape으로 뺀다. 다른 OS는 아직 입력 영역을 빼지 못해 pane 외곽 변에 여백을 둔다.

use crate::model::{PhysicalPx, PhysicalRect};

/// native WebView가 leaf 안에서 비워 두는 변별 여백(물리 px). 왼쪽·오른쪽·아래 순서다.
/// Linux는 입력 영역을 input shape으로 빼므로 여백이 없다.
#[cfg(target_os = "linux")]
pub fn webview_edge_inset(
    _leaf: PhysicalRect,
    _content: PhysicalRect,
    _scale_factor: f32,
) -> [PhysicalPx; 3] {
    [PhysicalPx::default(); 3]
}

/// native WebView가 leaf 안에서 비워 두는 변별 여백(물리 px). 왼쪽·오른쪽·아래 순서다.
/// pane 콘텐츠 영역 외곽에 닿는 변에는 분할선 입력 영역만큼 비워 둔다. 내부 leaf 사이에는
/// divider gap만 있고, 위쪽은 탭 바와 닿아 여백이 없다. 입력 영역을 빼는 방법이 없는 OS의 동작이다.
#[cfg(not(target_os = "linux"))]
pub fn webview_edge_inset(
    leaf: PhysicalRect,
    content: PhysicalRect,
    scale_factor: f32,
) -> [PhysicalPx; 3] {
    let inset = PhysicalPx(crate::state::mouse::divider_hit_threshold_physical(
        scale_factor,
    ));
    let on_edge = |a: PhysicalPx, b: PhysicalPx| (a - b).abs() < PhysicalPx(0.5);
    let pick = |touches: bool| {
        if touches {
            inset
        } else {
            PhysicalPx::default()
        }
    };
    [
        pick(on_edge(leaf.x, content.x)),
        pick(on_edge(leaf.x + leaf.width, content.x + content.width)),
        pick(on_edge(leaf.y + leaf.height, content.y + content.height)),
    ]
}

/// 창 좌표의 host 입력 영역(물리 px). 분할선마다 hit-test 범위와 같은 띠를 만들고,
/// `resize_band` 가 0보다 크면 창 네 가장자리의 리사이즈 밴드를 더한다.
/// 분할선 사각형은 두 자식 사이의 틈이며 hit-test는 그 틈의 앞쪽 변을 중심으로
/// `divider_hit_threshold_physical` 만큼 양쪽을 본다(`find_divider_at`).
pub fn host_input_zones(
    dividers: &[PhysicalRect],
    window_width: PhysicalPx,
    window_height: PhysicalPx,
    resize_band: PhysicalPx,
    scale_factor: f32,
) -> Vec<PhysicalRect> {
    let reach = PhysicalPx(crate::state::mouse::divider_hit_threshold_physical(
        scale_factor,
    ));
    let mut zones: Vec<PhysicalRect> = dividers
        .iter()
        .map(|d| {
            if d.width <= d.height {
                PhysicalRect {
                    x: d.x - reach,
                    y: d.y,
                    width: reach + reach,
                    height: d.height,
                }
            } else {
                PhysicalRect {
                    x: d.x,
                    y: d.y - reach,
                    width: d.width,
                    height: reach + reach,
                }
            }
        })
        .collect();
    if resize_band > PhysicalPx::default() {
        let origin = PhysicalPx::default();
        zones.extend([
            PhysicalRect {
                x: origin,
                y: origin,
                width: resize_band,
                height: window_height,
            },
            PhysicalRect {
                x: window_width - resize_band,
                y: origin,
                width: resize_band,
                height: window_height,
            },
            PhysicalRect {
                x: origin,
                y: origin,
                width: window_width,
                height: resize_band,
            },
            PhysicalRect {
                x: origin,
                y: window_height - resize_band,
                width: window_width,
                height: resize_band,
            },
        ]);
    }
    zones
}

/// `webview` 와 겹치는 host 입력 영역을 WebView 창 기준 좌표로 돌려준다. 겹치지 않으면 뺀다.
pub fn webview_input_holes(webview: PhysicalRect, zones: &[PhysicalRect]) -> Vec<PhysicalRect> {
    zones
        .iter()
        .filter_map(|z| {
            let x0 = z.x.max(webview.x);
            let y0 = z.y.max(webview.y);
            let x1 = (z.x + z.width).min(webview.x + webview.width);
            let y1 = (z.y + z.height).min(webview.y + webview.height);
            (x1 > x0 && y1 > y0).then(|| PhysicalRect {
                x: x0 - webview.x,
                y: y0 - webview.y,
                width: x1 - x0,
                height: y1 - y0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(rects: &[PhysicalRect]) -> Vec<[f32; 4]> {
        rects
            .iter()
            .map(|r| [r.x.value(), r.y.value(), r.width.value(), r.height.value()])
            .collect()
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> PhysicalRect {
        PhysicalRect {
            x: PhysicalPx(x),
            y: PhysicalPx(y),
            width: PhysicalPx(width),
            height: PhysicalPx(height),
        }
    }

    /// 단일 pane: 분할선이 없고 창 오른쪽·아래 가장자리 밴드만 WebView와 겹친다.
    #[test]
    fn a_single_pane_webview_only_yields_the_window_resize_band() {
        let zones = host_input_zones(
            &[],
            PhysicalPx(1280.0),
            PhysicalPx(720.0),
            PhysicalPx(8.0),
            1.0,
        );
        let holes = webview_input_holes(rect(180.0, 61.0, 1100.0, 634.0), &zones);
        assert_eq!(values(&holes), values(&[rect(1092.0, 0.0, 8.0, 634.0)]));
    }

    /// 세로 분할: 분할선 hit 띠가 오른쪽 pane 의 왼쪽 끝을 덮고, 창 밴드는 오른쪽에만 닿는다.
    #[test]
    fn a_split_webview_yields_the_divider_band_and_the_window_edge() {
        let divider = rect(729.0, 30.0, 1.0, 665.0);
        let zones = host_input_zones(
            &[divider],
            PhysicalPx(1280.0),
            PhysicalPx(720.0),
            PhysicalPx(8.0),
            1.0,
        );
        let holes = webview_input_holes(rect(730.0, 61.0, 550.0, 634.0), &zones);
        // hit-test 는 |x - 729| < 4 라 띠는 725..733 이고 WebView 안쪽은 730..733 이다.
        assert_eq!(
            values(&holes),
            values(&[rect(0.0, 0.0, 3.0, 634.0), rect(542.0, 0.0, 8.0, 634.0)])
        );
    }

    /// 배율 2: 분할선 띠는 논리 4px = 물리 8px 씩 양쪽, 밴드는 물리 8px 그대로다.
    #[test]
    fn the_divider_band_scales_but_the_resize_band_stays_physical() {
        let divider = rect(1460.0, 60.0, 2.0, 1330.0);
        let zones = host_input_zones(
            &[divider],
            PhysicalPx(2560.0),
            PhysicalPx(1440.0),
            PhysicalPx(8.0),
            2.0,
        );
        assert_eq!(
            values(&zones[..1]),
            values(&[rect(1452.0, 60.0, 16.0, 1330.0)])
        );
        assert_eq!(
            values(&zones[2..3]),
            values(&[rect(2552.0, 0.0, 8.0, 1440.0)])
        );
    }

    /// 최대화·전체화면처럼 밴드가 0이면 창 가장자리는 WebView 입력에서 빼지 않는다.
    #[test]
    fn no_resize_band_leaves_the_window_edge_to_the_webview() {
        let zones = host_input_zones(
            &[],
            PhysicalPx(1280.0),
            PhysicalPx(720.0),
            PhysicalPx(0.0),
            1.0,
        );
        assert!(webview_input_holes(rect(180.0, 61.0, 1100.0, 634.0), &zones).is_empty());
    }

    /// 가로 분할선은 위아래로 띠를 만든다.
    #[test]
    fn a_horizontal_divider_yields_a_band_across() {
        let divider = rect(180.0, 400.0, 1100.0, 1.0);
        let zones = host_input_zones(
            &[divider],
            PhysicalPx(1280.0),
            PhysicalPx(720.0),
            PhysicalPx(0.0),
            1.0,
        );
        let holes = webview_input_holes(rect(180.0, 401.0, 1100.0, 294.0), &zones);
        assert_eq!(values(&holes), values(&[rect(0.0, 0.0, 1100.0, 3.0)]));
    }
}
