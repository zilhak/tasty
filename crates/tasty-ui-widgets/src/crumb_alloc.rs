//! 본체와 갤러리가 공유하는 경로 표시줄의 폭 배분. 호출자가 글자 폭을 측정해 전달한다.
//! 가용 폭은 뒤로 가기 버튼과 간격을 제외한 값이다.
//! 조상 경로를 하나씩 접고 부모·현재 폴더의 폭을 줄인 뒤 부모와 루트를 차례로 접는다.
//! 다시 펼칠 때는 hysteresis만큼 여유를 요구해 창 크기 변경 중 단계가 반복해서 바뀌지 않게 한다.

/// breadcrumb 한 칸 — 실제 성분이거나, 가운데를 접은 `…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrumbSlot {
    /// `crumbs[i]`.
    Crumb(usize),
    /// 숨긴 조상들의 인덱스 범위.
    Hidden(std::ops::Range<usize>),
}

/// 경로 요소의 역할. 최소 폭과 말줄임 방향을 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 루트 경로. 다른 표시 요소에 폭을 배분한 뒤 남은 폭에 맞출 수 있다.
    Root,
    /// root 와 현재 폴더 사이. 꼬리에서 말줄임한다.
    Ancestor,
    /// 현재 폴더. **앞에서** 말줄임한다 — 형제 폴더를 가르는 것은 꼬리다.
    Current,
    /// 접힌 조상들의 `…`. 줄지 않는다.
    Hidden,
}

/// 토큰에서 온 치수(배율 적용 후).
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    /// 한 칸의 상한. `component.fp-crumb-max-width`.
    pub crumb_max: f32,
    /// 조상·부모의 바닥. `component.fp-crumb-min-width`.
    pub ancestor_min: f32,
    /// 현재 폴더의 바닥. `component.fp-crumb-current-min-width`.
    pub current_min: f32,
    /// 되돌아 펴는 데 필요한 여유. `component.fp-bar-hysteresis`.
    pub hysteresis: f32,
}

/// 배분의 입력. 폭은 전부 논리 픽셀의 f32 다.
pub struct Measure<'a> {
    /// 성분마다의 말줄임 없는 폭. `[0]` 이 root, 마지막이 현재 폴더다.
    pub natural: &'a [f32],
    /// `…` 한 칸의 폭.
    pub ellipsis: f32,
    /// 구분자 글리프 + 좌우 gap — 칸 사이마다 한 번.
    pub separator: f32,
    /// 뒤 버튼과 gap 을 뺀 나머지.
    pub available: f32,
    pub caps: Caps,
}

/// 그릴 칸 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub slot: CrumbSlot,
    /// 이 칸에 준 폭. 말줄임은 이 폭에서 한다.
    pub width: f32,
    pub role: Role,
}

/// 한 프레임의 배분 결과.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub placed: Vec<Placed>,
    /// 접기 단계. 작을수록 더 많이 펼쳐져 있으며 다음 프레임의 히스테리시스 계산에 쓴다.
    pub step: usize,
}

/// 접기 단계의 구성. 실제 폭은 lay_out에서 계산한다.
#[derive(Clone, Copy)]
struct Rung {
    /// 접힌 조상의 범위(`1..1+h`). 비면 접힌 것이 없다.
    hidden: (usize, usize),
    /// 부모 칸의 상한.
    parent_cap: f32,
    /// 현재 폴더의 상한.
    current_cap: f32,
    /// root 를 보이는가. 거짓이면 `… › current` 한 단뿐이다.
    root_shown: bool,
    /// 루트를 상한보다 좁힐 수 있는지. 조상을 접기 전에는 허용하지 않는다.
    root_may_shrink: bool,
}

/// 덜 접힌 구성부터 순서대로 생성한다.
fn rungs(n: usize, caps: &Caps) -> Vec<Rung> {
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    // 조상(=root 와 현재 폴더 사이)에서 부모를 뺀 수. 이만큼이 한 칸씩 접힌다.
    let foldable = n.saturating_sub(3);
    for h in 0..=foldable {
        out.push(Rung {
            hidden: (1, 1 + h),
            parent_cap: caps.crumb_max,
            current_cap: caps.crumb_max,
            root_shown: true,
            root_may_shrink: false,
        });
    }
    if n >= 3 {
        let all = (1, n - 2);
        out.push(Rung {
            hidden: all,
            parent_cap: caps.ancestor_min,
            current_cap: caps.crumb_max,
            root_shown: true,
            root_may_shrink: true,
        });
        out.push(Rung {
            hidden: all,
            parent_cap: caps.ancestor_min,
            current_cap: caps.current_min,
            root_shown: true,
            root_may_shrink: true,
        });
    }
    if n >= 2 {
        // 부모까지 접는다 — `root › … › current`.
        out.push(Rung {
            hidden: (1, n - 1),
            parent_cap: caps.ancestor_min,
            current_cap: caps.current_min,
            root_shown: true,
            root_may_shrink: true,
        });
        // root 까지 접는다 — `… › current`. 디자인의 바닥이다.
        out.push(Rung {
            hidden: (0, n - 1),
            parent_cap: caps.ancestor_min,
            current_cap: caps.current_min,
            root_shown: false,
            root_may_shrink: false,
        });
    }
    out
}

/// 한 구성에 폭을 배분해 가용 폭 안에 들어가면 반환한다.
/// 루트를 줄일 때도 말줄임표 한 칸보다 좁아지면 이 구성은 사용할 수 없다.
fn lay_out(m: &Measure<'_>, rung: &Rung, budget: f32) -> Option<Vec<Placed>> {
    let n = m.natural.len();
    let (hs, he) = rung.hidden;
    let mut placed: Vec<Placed> = Vec::new();
    let mut root_at: Option<usize> = None;

    if rung.root_shown {
        root_at = Some(placed.len());
        placed.push(Placed {
            slot: CrumbSlot::Crumb(0),
            width: m.natural[0].min(m.caps.crumb_max),
            role: Role::Root,
        });
    }
    if he > hs {
        placed.push(Placed {
            slot: CrumbSlot::Hidden(hs..he),
            width: m.ellipsis,
            role: Role::Hidden,
        });
    }
    for i in he..n - 1 {
        let cap = if i + 2 == n {
            rung.parent_cap
        } else {
            m.caps.crumb_max
        };
        placed.push(Placed {
            slot: CrumbSlot::Crumb(i),
            width: m.natural[i].min(cap),
            role: Role::Ancestor,
        });
    }
    placed.push(Placed {
        slot: CrumbSlot::Crumb(n - 1),
        width: m.natural[n - 1].min(rung.current_cap),
        role: Role::Current,
    });

    let separators = m.separator * placed.len().saturating_sub(1) as f32;
    let total: f32 = placed.iter().map(|p| p.width).sum::<f32>() + separators;
    if total <= budget {
        return Some(placed);
    }
    if !rung.root_may_shrink {
        return None;
    }
    let over = total - budget;
    let at = root_at?;
    let room = placed[at].width - m.ellipsis;
    if room < over {
        return None;
    }
    placed[at].width -= over;
    Some(placed)
}

/// 현재 프레임의 구성. 다시 펼칠 때만 추가 여유를 요구하고 더 접을 때는 즉시 적용한다.
pub fn plan(m: &Measure<'_>, previous: Option<usize>) -> Plan {
    let ladder = rungs(m.natural.len(), &m.caps);
    if ladder.is_empty() {
        return Plan {
            placed: Vec::new(),
            step: 0,
        };
    }
    for (step, rung) in ladder.iter().enumerate() {
        let budget = match previous {
            Some(prev) if step < prev => m.available - m.caps.hysteresis,
            _ => m.available,
        };
        if let Some(placed) = lay_out(m, rung, budget) {
            return Plan { placed, step };
        }
    }
    // 가장 많이 접은 구성도 안 들어가면 그 구성의 최소 폭을 사용한다.
    let step = ladder.len() - 1;
    let placed = lay_out(m, &ladder[step], f32::INFINITY).unwrap_or_default();
    Plan { placed, step }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: Caps = Caps {
        crumb_max: 180.0,
        ancestor_min: 64.0,
        current_min: 96.0,
        hysteresis: 8.0,
    };

    fn measure<'a>(natural: &'a [f32], available: f32) -> Measure<'a> {
        Measure {
            natural,
            ellipsis: 10.0,
            separator: 20.0,
            available,
            caps: CAPS,
        }
    }

    fn slots(p: &Plan) -> Vec<CrumbSlot> {
        p.placed.iter().map(|x| x.slot.clone()).collect()
    }

    /// 가용 폭에 들어가면 깊이와 무관하게 모두 표시한다.
    #[test]
    fn a_path_that_fits_is_not_folded_however_deep_it_is() {
        let natural = [30.0; 8];
        let wide = measure(&natural, 1000.0);
        let p = plan(&wide, None);
        assert_eq!(p.step, 0);
        assert_eq!(p.placed.len(), 8, "다 들어가는데 접혔다");
    }

    /// 조상은 **한 칸씩** 접힌다. 한 번에 마지막 둘만 남기지 않는다.
    #[test]
    fn ancestors_fold_one_at_a_time() {
        let natural = [30.0; 8];
        // 8 칸 · 칸 30 · 구분자 20 → 240 + 140 = 380. 첫 접기는 칸 하나를 `…`(10)로
        // 바꾸므로 360 이고, 둘째 접기부터는 칸과 구분자가 함께 빠져 310 이 된다.
        let one = plan(&measure(&natural, 362.0), None);
        assert_eq!(
            slots(&one)[..2],
            [CrumbSlot::Crumb(0), CrumbSlot::Hidden(1..2)],
            "한 칸만 접혀야 한다: {:?}",
            slots(&one)
        );
        let two = plan(&measure(&natural, 330.0), None);
        assert_eq!(
            slots(&two)[1],
            CrumbSlot::Hidden(1..3),
            "둘째 칸이 안 접혔다"
        );
        assert!(two.step > one.step, "폭이 좁아졌는데 더 접히지 않았다");
    }

    /// 부모를 최소 폭까지 줄인 뒤 현재 폴더를 줄인다.
    #[test]
    fn the_parent_hits_its_floor_before_the_current_folder_shrinks() {
        let natural = [20.0, 300.0, 300.0, 300.0];
        // 조상을 다 접고 부모를 64 로 줄이면 root 20 + … 10 + 64 + 180 + 구분자 60 = 334.
        let p = plan(&measure(&natural, 340.0), None);
        let parent = p
            .placed
            .iter()
            .find(|x| x.slot == CrumbSlot::Crumb(2))
            .expect("부모 칸");
        let current = p
            .placed
            .iter()
            .find(|x| x.role == Role::Current)
            .expect("현재 칸");
        assert_eq!(
            parent.width, CAPS.ancestor_min,
            "부모가 최소 폭까지 줄지 않았다"
        );
        assert_eq!(
            current.width, CAPS.crumb_max,
            "부모가 최소 폭에 도달하기 전에 현재 폴더가 줄었다"
        );
    }

    /// 폭이 더 좁아지면 부모와 루트를 접어 말줄임표와 현재 폴더만 남긴다.
    #[test]
    fn the_floor_of_the_ladder_is_a_single_crumb() {
        let natural = [300.0, 300.0, 300.0, 300.0];
        let p = plan(&measure(&natural, 60.0), None);
        assert_eq!(
            slots(&p),
            [CrumbSlot::Hidden(0..3), CrumbSlot::Crumb(3)],
            "마지막 구성은 `… › current`여야 한다"
        );
        let current = &p.placed[1];
        assert_eq!(current.role, Role::Current);
        assert_eq!(
            current.width, CAPS.current_min,
            "마지막 구성에서도 현재 폴더의 최소 폭을 확보해야 한다"
        );
    }

    /// 긴 루트의 폭을 줄여 짧은 현재 폴더를 보존한다.
    #[test]
    fn a_long_root_gives_way_before_the_current_folder() {
        let natural = [180.0, 40.0, 40.0];
        let p = plan(&measure(&natural, 200.0), None);
        let root = &p.placed[0];
        let current = p.placed.last().expect("현재 칸");
        assert_eq!(root.role, Role::Root);
        assert!(root.width < CAPS.crumb_max, "root 가 안 줄었다");
        assert_eq!(current.width, 40.0, "root 가 줄기 전에 현재 폴더가 줄었다");
    }

    /// 다시 펼칠 때는 추가 여유가 필요하다.
    #[test]
    fn growing_back_needs_more_room_than_folding_did() {
        let natural = [30.0; 8];
        // 접힌 뒤의 단을 직전 값으로 준다.
        let folded = plan(&measure(&natural, 362.0), None);
        assert!(folded.step > 0);

        // 정확히 0 단이 맞는 폭(380)에서 **직전이 접힌 단이면** 아직 안 편다.
        let held = plan(&measure(&natural, 380.0), Some(folded.step));
        assert_eq!(held.step, folded.step, "여유 없이 되돌아 폈다");

        // 여유만큼 더 주면 편다.
        let grown = plan(
            &measure(&natural, 380.0 + CAPS.hysteresis),
            Some(folded.step),
        );
        assert_eq!(grown.step, 0, "여유를 줬는데 안 폈다");
    }

    /// 더 접는 데에는 추가 여유를 요구하지 않는다.
    #[test]
    fn folding_further_does_not_wait_for_the_hysteresis() {
        let natural = [30.0; 8];
        let narrow = plan(&measure(&natural, 330.0), Some(0));
        assert!(
            narrow.step > 0,
            "이전 상태가 펼쳐져 있어도 폭이 부족하면 접어야 한다"
        );
    }
}
