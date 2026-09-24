//! PTY 전경 이름으로 busy·마우스 캡처 설정을 계산하고 원격 busy 값은 따로 보관한다.

use super::CoreState;

impl CoreState {
    /// 로컬 PTY를 폴링한다. 반환값은 busy 집합 변화만 나타내며 이름·캡처 설정 변화는 제외한다.
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        // Windows에서 surface마다 전체 프로세스를 다시 조회하지 않도록 한 번에 해석한다.
        let mut sids: Vec<u32> = Vec::new();
        let mut shell_pids: Vec<u32> = Vec::new();
        for (sid, terminal) in self.terminals.iter() {
            if let Some(pid) = terminal.process_id() {
                sids.push(sid);
                shell_pids.push(pid);
            }
        }
        let foregrounds = tasty_terminal::foreground_process::resolve_foreground_many(&shell_pids);

        let mut busy: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut mouse_capture_disabled: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        let mut mouse_capture_banner_suppressed: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        let mut names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        for ((&sid, &shell_pid), fg) in sids.iter().zip(shell_pids.iter()).zip(foregrounds.iter()) {
            let Some(terminal) = self.terminals.get(sid) else {
                continue;
            };
            if terminal.busy_with_foreground(shell_pid, fg.as_ref()) {
                busy.insert(sid);
            }
            if let Some(f) = fg.as_ref() {
                if self.settings.general.mouse_capture_disabled_for(&f.name) {
                    mouse_capture_disabled.insert(sid);
                }
                if self
                    .settings
                    .general
                    .mouse_capture_banner_disabled_for(&f.name)
                {
                    mouse_capture_banner_suppressed.insert(sid);
                }
                names.insert(sid, f.name.clone());
            }
        }
        bump_foreground_generations(
            &mut self.foreground_generation,
            &self.foreground_names,
            &names,
        );

        self.mouse_capture_disabled_surfaces = mouse_capture_disabled;
        self.mouse_capture_banner_suppressed_surfaces = mouse_capture_banner_suppressed;
        self.foreground_names = names;
        let changed = self.busy_surfaces != busy;
        self.busy_surfaces = busy;
        changed
    }

    /// 마지막 폴링의 캡처 제외 설정. 클릭·드래그를 로컬 선택으로 처리하고 휠은 그대로 둔다.
    pub fn is_surface_mouse_capture_disabled(&self, surface_id: u32) -> bool {
        self.mouse_capture_disabled_surfaces.contains(&surface_id)
    }

    /// 캡처 동작은 유지하면서 안내 배너만 숨기는 설정이다.
    #[cfg(any(feature = "gui", test))]
    pub fn is_surface_mouse_capture_banner_suppressed(&self, surface_id: u32) -> bool {
        self.mouse_capture_banner_suppressed_surfaces
            .contains(&surface_id)
    }

    /// 로컬 폴링 또는 원격 push 중 하나가 busy이면 true다.
    pub fn is_surface_busy(&self, surface_id: u32) -> bool {
        self.is_locally_or_mirror_busy(surface_id)
    }

    /// 마지막 폴링의 전경 이름. 아직 해석하지 못한 surface는 None이다.
    pub fn foreground_name(&self, surface_id: u32) -> Option<&str> {
        self.foreground_names.get(&surface_id).map(String::as_str)
    }

    /// 관측한 전경 이름이 바뀔 때 증가한다. 배너가 이전 프로그램의 것인지 판단하는 데 쓴다.
    /// 같은 이름의 프로그램이 폴링 사이에 재실행된 경우는 구분하지 못한다.
    #[cfg(any(feature = "gui", test))]
    pub fn foreground_generation(&self, surface_id: u32) -> u64 {
        self.foreground_generation
            .get(&surface_id)
            .copied()
            .unwrap_or(0)
    }

    /// 로컬 폴링 결과와 원격 push 결과의 합집합.
    fn is_locally_or_mirror_busy(&self, surface_id: u32) -> bool {
        self.busy_surfaces.contains(&surface_id) || self.mirror_busy_surfaces.contains(&surface_id)
    }

    // 이유: 현재 호출자는 test 전용 코드다.
    #[allow(dead_code)]
    pub fn any_busy(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|&sid| self.is_locally_or_mirror_busy(sid))
    }

    pub fn busy_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|&&sid| self.is_locally_or_mirror_busy(sid))
            .count()
    }

    /// 원격 push 값을 적용한다. 호출자가 mirror ID를 넘기는지 여기서는 검증하지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub fn set_mirror_surface_busy(&mut self, surface_id: u32, busy: bool) {
        if busy {
            self.mirror_busy_surfaces.insert(surface_id);
        } else {
            self.mirror_busy_surfaces.remove(&surface_id);
        }
    }

    #[cfg(any(feature = "gui", test))]
    pub fn forget_mirror_surface_busy(&mut self, surface_id: u32) {
        self.mirror_busy_surfaces.remove(&surface_id);
    }

    /// 하드 점유한 surface의 로컬 busy 전송 후보. 최초 값과 holder·busy 변경을 담는다.
    /// 점유가 끝난 캐시는 지우고 같은 조회 간격 안의 holder 변경도 구분한다.
    /// 후보 생성 시 캐시를 갱신하므로 전송에 실패해도 같은 값은 재시도하지 않는다.
    pub fn busy_activity_forwards(
        &mut self,
    ) -> Vec<(crate::core::attach::AttachClientId, u32, bool)> {
        let locks = self.attach.locks_snapshot();
        let occupied: std::collections::HashSet<u32> = locks.iter().map(|&(sid, _)| sid).collect();
        self.last_forwarded_busy
            .retain(|sid, _| occupied.contains(sid));
        let mut out = Vec::new();
        for (sid, lock) in locks {
            let record = (lock.holder, self.busy_surfaces.contains(&sid));
            if self.last_forwarded_busy.get(&sid) != Some(&record) {
                self.last_forwarded_busy.insert(sid, record);
                out.push((record.0, sid, record.1));
            }
        }
        out
    }
}

/// 전경 이름이 달라지면 번호를 올리고 더는 관측하지 못한 surface는 제거한다.
fn bump_foreground_generations(
    generations: &mut std::collections::HashMap<u32, u64>,
    old_names: &std::collections::HashMap<u32, String>,
    new_names: &std::collections::HashMap<u32, String>,
) {
    for (&sid, new_name) in new_names.iter() {
        if old_names.get(&sid) != Some(new_name) {
            *generations.entry(sid).or_insert(0) += 1;
        }
    }
    generations.retain(|sid, _| new_names.contains_key(sid));
}

#[cfg(test)]
mod tests {
    use super::CoreState;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn mirror_busy_surface_counts_without_local_pty() {
        let mut e = engine();
        assert!(!e.is_surface_busy(42), "초기 상태는 idle");
        e.set_mirror_surface_busy(42, true);
        assert!(e.is_surface_busy(42));
        assert_eq!(e.busy_count(&[42]), 1);
        assert!(e.any_busy(&[42]));
        e.set_mirror_surface_busy(42, false);
        assert!(!e.is_surface_busy(42));
        assert_eq!(e.busy_count(&[42]), 0);
    }

    #[test]
    fn refresh_busy_surfaces_does_not_clobber_mirror_busy() {
        let mut e = engine();
        e.set_mirror_surface_busy(42, true);
        e.refresh_busy_surfaces(); // 로컬 터미널이 없으니 busy_surfaces 는 빈 채로 재계산.
        assert!(
            e.is_surface_busy(42),
            "로컬 refresh 가 mirror 의 busy 상태를 지우면 안 된다"
        );
    }

    #[test]
    fn forget_mirror_surface_busy_clears_entry() {
        let mut e = engine();
        e.set_mirror_surface_busy(42, true);
        e.forget_mirror_surface_busy(42);
        assert!(!e.is_surface_busy(42));
    }

    #[test]
    fn mouse_capture_banner_suppressed_accessor_reads_cache() {
        let mut e = engine();
        assert!(!e.is_surface_mouse_capture_banner_suppressed(42));
        e.mouse_capture_banner_suppressed_surfaces.insert(42);
        assert!(e.is_surface_mouse_capture_banner_suppressed(42));
    }

    #[test]
    fn refresh_busy_surfaces_replaces_both_mouse_capture_caches() {
        let mut e = engine();
        e.mouse_capture_disabled_surfaces.insert(99);
        e.mouse_capture_banner_suppressed_surfaces.insert(99);
        e.refresh_busy_surfaces(); // 로컬 터미널이 없으니 두 캐시 모두 빈 채로 재계산.
        assert!(!e.is_surface_mouse_capture_disabled(99));
        assert!(!e.is_surface_mouse_capture_banner_suppressed(99));
    }

    #[test]
    fn busy_activity_forwards_only_on_change() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");

        let first = e.busy_activity_forwards();
        assert_eq!(first, vec![(7, sid, false)]);

        assert!(e.busy_activity_forwards().is_empty());

        e.busy_surfaces.insert(sid);
        assert_eq!(e.busy_activity_forwards(), vec![(7, sid, true)]);

        assert!(e.busy_activity_forwards().is_empty());
    }

    #[test]
    fn busy_activity_forwards_resets_on_reacquire() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");
        assert_eq!(e.busy_activity_forwards(), vec![(7, sid, false)]);
        assert!(e.busy_activity_forwards().is_empty());

        e.attach.release(sid, 7).expect("release");
        assert!(e.busy_activity_forwards().is_empty());

        e.attach.acquire(sid, 9).expect("다른 client 재획득");
        assert_eq!(
            e.busy_activity_forwards(),
            vec![(9, sid, false)],
            "재획득 후에는 값이 이전과 같아도(false) 새 holder 에게 다시 push"
        );
    }

    #[test]
    fn busy_activity_forwards_holder_swap_within_one_tick_pushes_to_the_new_holder() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.busy_surfaces.insert(sid);
        e.attach.acquire(sid, 7).expect("lock 획득");
        assert_eq!(e.busy_activity_forwards(), vec![(7, sid, true)]);

        e.attach.release(sid, 7).expect("release");
        e.attach
            .acquire(sid, 9)
            .expect("같은 tick 창 안의 다른 client 획득");
        assert_eq!(
            e.busy_activity_forwards(),
            vec![(9, sid, true)],
            "값이 그대로여도 새 holder 는 초기 push 를 받아야 한다"
        );
        assert!(e.busy_activity_forwards().is_empty());
    }

    #[test]
    fn foreground_generation_bumps_on_name_change_and_holds_when_unchanged() {
        let mut gens = std::collections::HashMap::new();
        let mut old = std::collections::HashMap::new();
        let mut new = std::collections::HashMap::new();

        new.insert(7u32, "vim".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 1);

        old = new.clone();
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 1);

        old = new.clone();
        new.insert(7, "bash".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 2);

        old = new.clone();
        new.insert(7, "htop".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 3);
    }

    /// 일부만 정리하는지 구분하려면 사라진 surface와 계속 관측되는 surface가 모두 필요하다.
    #[test]
    fn foreground_generation_prunes_entries_for_surfaces_no_longer_resolved() {
        let mut gens = std::collections::HashMap::new();
        gens.insert(8u32, 0u64);
        gens.insert(9u32, 3u64);
        let old = std::collections::HashMap::new();
        let new = std::collections::HashMap::from([(8u32, "sh".to_string())]);
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert!(!gens.contains_key(&9));
        assert_eq!(gens.get(&8), Some(&1));
    }

    #[test]
    fn foreground_generation_accessor_reads_cache_and_defaults_to_zero() {
        let mut e = engine();
        assert_eq!(e.foreground_generation(42), 0);
        e.foreground_generation.insert(42, 5);
        assert_eq!(e.foreground_generation(42), 5);
    }
}
