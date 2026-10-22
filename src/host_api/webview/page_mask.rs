//! 페이지 그림에서 뺄 구멍들(안내 카드·불투명 오버레이 상자)을 겹치지 않는 보이는 사각형으로 나눈다.
//! macOS 는 이 사각형들로 WebView layer mask 를 만든다. 구멍끼리 겹쳐도 even-odd 처럼 겹친 곳이 다시
//! 보이지 않는다. 좌표는 WebView 왼쪽 위 기준 물리 정수 px `[x, y, 너비, 높이]` 다.

/// `[x, y, 너비, 높이]` 물리 px.
pub(crate) type PxRect = [i32; 4];

/// `width`×`height` 페이지에서 `holes` 를 모두 뺀 보이는 영역을 겹치지 않는 사각형으로 돌려준다. 구멍이 페이지
/// 밖으로 나가도 된다. 빈 구멍은 무시한다.
pub(crate) fn visible_rects(width: i32, height: i32, holes: &[PxRect]) -> Vec<PxRect> {
    let mut visible = vec![[0, 0, width.max(0), height.max(0)]];
    visible.retain(|r| r[2] > 0 && r[3] > 0);
    for hole in holes.iter().filter(|h| h[2] > 0 && h[3] > 0) {
        visible = visible
            .into_iter()
            .flat_map(|r| subtract(r, *hole))
            .collect();
    }
    visible
}

/// `r` 에서 `hole` 을 뺀 나머지. 위·아래 띠는 전체 너비, 왼쪽·오른쪽 조각은 구멍 높이다.
fn subtract(r: PxRect, hole: PxRect) -> Vec<PxRect> {
    let [rx, ry, rw, rh] = r;
    let (r_right, r_bottom) = (rx + rw, ry + rh);
    let left = hole[0].max(rx);
    let top = hole[1].max(ry);
    let right = (hole[0] + hole[2]).min(r_right);
    let bottom = (hole[1] + hole[3]).min(r_bottom);
    if left >= right || top >= bottom {
        return vec![r];
    }
    [
        [rx, ry, rw, top - ry],
        [rx, bottom, rw, r_bottom - bottom],
        [rx, top, left - rx, bottom - top],
        [right, top, r_right - right, bottom - top],
    ]
    .into_iter()
    .filter(|p| p[2] > 0 && p[3] > 0)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(rects: &[PxRect]) -> i64 {
        rects
            .iter()
            .map(|r| i64::from(r[2]) * i64::from(r[3]))
            .sum()
    }

    fn covered(rects: &[PxRect], x: i32, y: i32) -> usize {
        rects
            .iter()
            .filter(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3])
            .count()
    }

    #[test]
    fn no_hole_keeps_the_whole_page() {
        assert_eq!(visible_rects(200, 100, &[]), vec![[0, 0, 200, 100]]);
        assert_eq!(
            visible_rects(200, 100, &[[10, 10, 0, 5]]),
            vec![[0, 0, 200, 100]]
        );
        assert!(visible_rects(0, 100, &[]).is_empty());
    }

    /// 겹치는 두 구멍(안내 카드와 팝업 상자)의 겹친 곳도 보이지 않고, 나머지는 정확히 한 번 덮인다.
    #[test]
    fn overlapping_holes_stay_hidden_and_the_rest_is_covered_once() {
        let holes = [[20, 10, 60, 40], [50, 30, 100, 50], [180, 90, 50, 50]];
        let rects = visible_rects(200, 100, &holes);
        for y in 0..100 {
            for x in 0..200 {
                let in_hole = holes
                    .iter()
                    .any(|h| x >= h[0] && x < h[0] + h[2] && y >= h[1] && y < h[1] + h[3]);
                assert_eq!(covered(&rects, x, y), usize::from(!in_hole), "({x},{y})");
            }
        }
    }

    #[test]
    fn a_hole_covering_the_page_leaves_nothing() {
        assert!(visible_rects(200, 100, &[[-5, -5, 300, 300]]).is_empty());
        let rects = visible_rects(200, 100, &[[0, 0, 200, 50]]);
        assert_eq!(rects, vec![[0, 50, 200, 50]]);
        assert_eq!(area(&rects), 200 * 50);
    }
}
