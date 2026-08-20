//! 그래프 공간(형제 축 = `cross`, 레이어 축 = `along`)에서의 엣지 폴리라인 산출.
//!
//! `rust-sugiyama` 는 레이어를 건너뛰는 엣지를 위해 dummy vertex 를 만들지만
//! **최종 출력에서 걷어내고 돌려준다**(phase 3 이 `is_dummy` 를 필터). 즉 dummy
//! 좌표는 라이브러리 밖으로 나오지 않는다 — 유령 노드가 샐 위험은 없는 대신
//! 꺾임점은 이쪽에서 직접 만들어야 한다.
//!
//! 만드는 방식: 엣지가 건너뛰는 각 중간 레이어마다 점을 하나씩 놓되, 출발/도착
//! 형제 좌표를 선형 보간한 위치를 후보로 삼고 그 자리가 실노드 카드에 막혀 있으면
//! 가장 가까운 **빈 통로**로 밀어낸다. 그래서 꺾임점은 절대 카드 위를 지나지 않는다.
//! 라이브러리가 dummy 폭만큼 통로를 이미 비워두므로 대개 보간 위치가 그대로 통과한다.

use std::collections::BTreeMap;

/// 레이아웃 치수 묶음. 전부 그래프 공간 기준 raw 값이다.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Metrics {
    /// 형제 축으로 노드 카드가 차지하는 길이.
    pub cross_extent: f32,
    /// 레이어 축으로 노드 카드가 차지하는 길이.
    pub along_extent: f32,
    /// 이웃한 두 레이어 사이 빈 간격.
    pub layer_gap: f32,
    /// 같은 레이어 안 이웃 노드 사이 빈 간격.
    pub sibling_gap: f32,
}

/// 레이어 번호 → 그 레이어 카드 중심의 레이어 축 좌표.
pub(crate) fn along_center(layer: u32, m: &Metrics) -> f32 {
    layer as f32 * (m.along_extent + m.layer_gap) + m.along_extent * 0.5
}

/// 엣지 하나의 폴리라인을 그래프 공간 `(cross, along)` 점 목록으로 만든다.
///
/// `channels` 는 **같은 조각(component) 안에서** 레이어별 실노드 중심의 형제 축
/// 좌표를 오름차순으로 담은 맵이다.
///
/// 반환 점은 항상 2 개 이상이고, 첫 점과 끝 점은 각 카드의 마주보는 변 위에 있다.
pub(crate) fn route(
    from: (u32, f32),
    to: (u32, f32),
    channels: &BTreeMap<u32, Vec<f32>>,
    m: &Metrics,
) -> Vec<(f32, f32)> {
    let (from_layer, from_cross) = from;
    let (to_layer, to_cross) = to;

    if from_layer == to_layer {
        return same_layer(from, to, m);
    }

    let half = m.along_extent * 0.5;
    let forward = to_layer > from_layer;
    let sign = if forward { 1.0 } else { -1.0 };
    let start = (from_cross, along_center(from_layer, m) + sign * half);
    let end = (to_cross, along_center(to_layer, m) - sign * half);

    let span = (i64::from(to_layer) - i64::from(from_layer)).abs();
    let mut points = Vec::with_capacity(span as usize + 1);
    points.push(start);

    let step: i64 = if forward { 1 } else { -1 };
    let mut layer = i64::from(from_layer) + step;
    while layer != i64::from(to_layer) {
        let t = (layer - i64::from(from_layer)) as f32 / span as f32;
        let target = from_cross + (to_cross - from_cross) * t;
        #[allow(clippy::cast_sign_loss)] // layer 는 두 u32 사이를 오가므로 항상 >= 0
        let l = layer as u32;
        let occupied = channels.get(&l).map_or(&[][..], Vec::as_slice);
        points.push((free_channel(occupied, target, m), along_center(l, m)));
        layer += step;
    }

    points.push(end);
    points
}

/// 같은 레이어끼리 이어지는 엣지 — 레이어 축으로 진행할 곳이 없으므로 형제 축
/// 방향으로 마주보는 변끼리 잇는다.
///
/// 정상적인 레이어 배치에서는 나오지 않는다(최소 엣지 길이가 1 이라 양 끝 레이어가
/// 다르다). 라이브러리가 규칙을 어겼을 때 폴리라인이 비지 않게 하는 방어 코드다.
fn same_layer(from: (u32, f32), to: (u32, f32), m: &Metrics) -> Vec<(f32, f32)> {
    let (layer, from_cross) = from;
    let (_, to_cross) = to;
    let along = along_center(layer, m);
    let direction = if to_cross >= from_cross { 1.0 } else { -1.0 };
    let half = m.cross_extent * 0.5;
    vec![
        (from_cross + direction * half, along),
        (to_cross - direction * half, along),
    ]
}

/// `target` 이 실노드 카드에 막혀 있지 않으면 그대로, 막혀 있으면 가장 가까운 빈
/// 통로 좌표를 돌려준다.
///
/// 통로 후보는 (1) 이웃한 두 카드의 중점 (2) 양 끝 카드 바깥이다. 같은 레이어의
/// 이웃 중심 간 거리가 `cross_extent + sibling_gap` 이상이므로, 중점은 두 카드
/// 어느 쪽과도 `cross_extent/2 + sibling_gap/2` 이상 떨어진다 — 아래 `clearance`
/// 를 그보다 작게 잡은 이유다(항상 후보가 하나는 통과한다).
fn free_channel(occupied: &[f32], target: f32, m: &Metrics) -> f32 {
    let clearance = m.cross_extent * 0.5 + m.sibling_gap * 0.25;
    let is_free = |c: f32| occupied.iter().all(|o| (c - o).abs() >= clearance);

    if occupied.is_empty() || is_free(target) {
        return target;
    }

    let outside = m.cross_extent * 0.5 + m.sibling_gap * 0.5;
    let mut best: Option<f32> = None;
    let mut consider = |c: f32| {
        if !is_free(c) {
            return;
        }
        // 같은 거리면 먼저 본 후보를 유지한다 — 결정적 출력을 위해.
        if best.is_none_or(|b| (c - target).abs() < (b - target).abs()) {
            best = Some(c);
        }
    };

    for pair in occupied.windows(2) {
        consider((pair[0] + pair[1]) * 0.5);
    }
    if let Some(first) = occupied.first() {
        consider(first - outside);
    }
    if let Some(last) = occupied.last() {
        consider(last + outside);
    }

    // 모든 후보가 막힌 병리적 배치(간격 0 등)에서는 마지막 카드 바깥으로 밀어
    // 최소한 겹치지는 않게 한다.
    best.unwrap_or_else(|| occupied.last().map_or(target, |last| last + outside))
}
