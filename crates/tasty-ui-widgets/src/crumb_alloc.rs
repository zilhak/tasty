//! breadcrumb 폭 배분 — 어느 칸을 접고, 남은 칸에 얼마를 주는가.
//!
//! **본체와 갤러리가 같은 사다리를 탄다.** 이 판정이 한쪽에만 있으면 specimen 이 보이는
//! 접힘과 본체가 하는 접힘이 갈리고, 가장 먼저 갈리는 축은 배율이다 — 그래서 여기 산다.
//!
//! 경계: **수만 다룬다.** 측정(각 성분의 자연 폭 · 구분자 폭)은 부르는 쪽이 하고, 여기서는
//! 그 수로 사다리를 훑는다. 그래서 이 판정에는 egui 없이 붙는 단위 테스트가 있다.
//!
//! 재는 폭은 **뒤 버튼과 그 gap 을 뺀 나머지**다 — popup 폭이 아니다. 우선순위는 높은
//! 것부터 현재 폴더 → 부모 → root → `…` 메뉴이고, 그래서 짜내기는 이 순서로 간다(각
//! 단계는 앞 단계가 바닥을 친 뒤에만):
//!
//! 1. 조상이 한 칸씩 `…` 메뉴로 접힌다
//! 2. 부모가 `crumb-max` 에서 `crumb-min` 까지 줄어든다
//! 3. 현재 폴더가 `crumb-max` 에서 `crumb-current-min` 까지 줄어든다
//! 4. 부모도 접힌다 — `root › … › current`
//! 5. root 도 접힌다 — `… › current`, 디자인의 바닥
//!
//! 되돌아 펴는 데에는 바닥보다 `bar-hysteresis` 만큼 더 필요하다. 그 여유가 없으면
//! 드래그 리사이즈가 경계 폭에서 두 단계를 오간다(펴면 안 맞아 다시 접히고, 접히면 맞아
//! 다시 펴진다).

/// breadcrumb 한 칸 — 실제 성분이거나, 가운데를 접은 `…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrumbSlot {
    /// `crumbs[i]`.
    Crumb(usize),
    /// 숨긴 조상들의 인덱스 범위.
    Hidden(std::ops::Range<usize>),
}

/// 한 칸이 맡은 역할. 역할이 바닥 폭과 **말줄임 방향**을 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 경로의 뿌리. 바닥이 없다시피 해서 가장 먼저 양보한다(긴 UNC root 가 현재 폴더보다
    /// 먼저 줄어드는 이유).
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
    /// 사다리에서 몇 번째 단인가. 히스테리시스가 이 수를 기억한다 — 값이 작을수록 덜 접혔다.
    pub step: usize,
}

/// 사다리 한 단의 서술. 실제 폭은 [`lay_out`] 이 잰다.
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
    /// root 가 자기 상한 아래로 줄어도 되는가.
    ///
    /// 조상이 아직 펴져 있는 단에서는 **거짓**이다 — `…` 메뉴가 우선순위의 맨 아래라,
    /// root 를 깎기 전에 조상이 먼저 그 메뉴로 접혀야 한다. 이것이 참이 아니면 root 가
    /// 매 단에서 조금씩 양보해 사다리가 첫 단을 떠나지 못한다.
    root_may_shrink: bool,
}

/// 사다리 — 덜 접힌 것부터. 각 단은 앞 단이 바닥을 친 뒤에만 온다.
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

/// 한 단을 실제 폭으로 편다. `budget` 안에 들어가면 `Some`.
///
/// root 는 마지막으로 양보한다 — 다른 칸을 다 준 뒤 남은 폭을 받고, `…` 한 칸보다 좁아지면
/// 그 단은 안 맞는 것으로 본다(보이는 말줄임 없이 잘리는 칸을 만들지 않는다).
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
    // root 가 남은 몫을 받는다. 바닥은 `…` 한 칸 — 그보다 좁으면 이 단은 안 맞는다.
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

/// 이 프레임의 배분. `previous` 는 직전 프레임의 단 — 덜 접힌 쪽으로 **되돌아갈 때만**
/// 히스테리시스를 요구한다(더 접히는 것은 즉시 일어나야 경로가 잘리지 않는다).
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
    // 바닥조차 안 맞으면 바닥을 그린다 — 현재 폴더는 자기 바닥 폭에서 앞 말줄임한다.
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

    /// 다 들어가면 아무것도 접지 않는다 — 접기는 깊이가 아니라 **넘침**이 부른다.
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
        assert!(two.step > one.step, "더 좁은데 단이 안 내려갔다");
    }

    /// 부모가 먼저 바닥을 치고, 그 다음에 현재 폴더가 줄어든다 — 우선순위의 순서다.
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
        assert_eq!(parent.width, CAPS.ancestor_min, "부모가 바닥까지 안 갔다");
        assert_eq!(
            current.width, CAPS.crumb_max,
            "부모가 바닥을 치기 전에 현재 폴더가 줄었다"
        );
    }

    /// 더 좁아지면 현재 폴더도 자기 바닥까지 내려가고, 그 다음에 부모가 접히고,
    /// 마지막에 root 가 접혀 `… › current` 만 남는다.
    #[test]
    fn the_floor_of_the_ladder_is_a_single_crumb() {
        let natural = [300.0, 300.0, 300.0, 300.0];
        let p = plan(&measure(&natural, 60.0), None);
        assert_eq!(
            slots(&p),
            [CrumbSlot::Hidden(0..3), CrumbSlot::Crumb(3)],
            "바닥이 `… › current` 가 아니다"
        );
        let current = &p.placed[1];
        assert_eq!(current.role, Role::Current);
        assert_eq!(
            current.width, CAPS.current_min,
            "바닥에서도 현재 폴더는 자기 바닥 폭을 받는다 — 보이는 말줄임이 있어야 한다"
        );
    }

    /// root 는 다른 칸보다 먼저 양보한다 — 긴 UNC root 와 짧은 현재 폴더의 갈래다.
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

    /// 되돌아 펴는 데에는 여유가 더 필요하다 — 없으면 경계 폭에서 두 단을 오간다.
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

    /// 더 접히는 쪽에는 여유를 요구하지 않는다 — 그쪽이 늦으면 경로가 잘린 채 한 프레임 간다.
    #[test]
    fn folding_further_does_not_wait_for_the_hysteresis() {
        let natural = [30.0; 8];
        let narrow = plan(&measure(&natural, 330.0), Some(0));
        assert!(narrow.step > 0, "직전이 덜 접힌 단이라고 안 접혔다");
    }
}
