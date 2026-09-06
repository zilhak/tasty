//! Surface 의 "busy" 상태 폴링/조회. `refresh_busy_surfaces` 는 매 tick 에 호출되어
//! 각 PTY 의 foreground process 를 비교해 `busy_surfaces` 를 갱신한다.

use super::CoreState;

impl CoreState {
    /// Recompute `busy_surfaces` by polling every PTY's foreground
    /// process. Returns true if the set changed (caller should redraw).
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        // Resolve every live surface's foreground program from a single system
        // snapshot. On Windows the foreground lookup snapshots all processes
        // (≈6ms with a few hundred); doing it per surface put O(surfaces ×
        // processes) on the main thread every 1Hz tick (≈370ms at 60 live
        // surfaces), stalling workspace switches and input. One snapshot per
        // tick collapses that to O(processes + surfaces).
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
        // 같은 1Hz foreground resolve 결과를 재사용해 마우스 캡처 블랙리스트 매칭도
        // 함께 계산한다(별도 프로세스 스냅샷 없음). 빈 블랙리스트면 매칭 헬퍼가
        // 즉시 false 라 비용 무시 가능.
        let mut mouse_capture_disabled: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        // 캡처 블랙리스트와 독립적인 축 — 안내 배너만 억제하는 블랙리스트 매칭도
        // 같은 foreground resolve 결과에 편승해 계산한다.
        let mut mouse_capture_banner_suppressed: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        // 같은 resolve 결과에서 StatusBar 용 foreground 이름도 함께 모은다. StatusBar 가
        // 매 프레임 프로세스 스냅샷을 다시 뜨는 대신 이 캐시를 읽는다.
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
        // foreground 이름이 바뀐 surface 의 generation 을 올린다 — 덮어쓰기(다음 줄) 전에
        // 옛 이름과 비교해야 하므로 `self.foreground_names` 교체 직전에 호출한다.
        bump_foreground_generations(
            &mut self.foreground_generation,
            &self.foreground_names,
            &names,
        );

        // 블랙리스트·이름 캐시는 다음 입력/프레임 조회로 반영되므로 redraw 신호(changed)
        // 에는 포함하지 않는다 — busy set 변화만으로 dirty 를 판정한다. 닫힌 surface 의
        // stale 엔트리가 남지 않도록 매 tick 맵 전체를 교체한다.
        self.mouse_capture_disabled_surfaces = mouse_capture_disabled;
        self.mouse_capture_banner_suppressed_surfaces = mouse_capture_banner_suppressed;
        self.foreground_names = names;
        let changed = self.busy_surfaces != busy;
        self.busy_surfaces = busy;
        changed
    }

    /// Whether the given surface's foreground process matches the mouse-capture
    /// blacklist (cached from the last `refresh_busy_surfaces` poll). When true,
    /// the host treats the surface's click/drag tracking as `None` (local
    /// select / context menu); the wheel is unaffected.
    pub fn is_surface_mouse_capture_disabled(&self, surface_id: u32) -> bool {
        self.mouse_capture_disabled_surfaces.contains(&surface_id)
    }

    /// Whether the given surface's foreground process matches the mouse-capture
    /// **banner suppression** blacklist (cached from the last
    /// `refresh_busy_surfaces` poll). When true, the capture-active hint banner
    /// is skipped while capture itself remains on — independent of
    /// [`is_surface_mouse_capture_disabled`](Self::is_surface_mouse_capture_disabled).
    pub fn is_surface_mouse_capture_banner_suppressed(&self, surface_id: u32) -> bool {
        self.mouse_capture_banner_suppressed_surfaces
            .contains(&surface_id)
    }

    /// Whether the given surface is currently busy — either a local terminal
    /// running a non-shell foreground program (cached from the last
    /// `refresh_busy_surfaces` poll) or a mirror terminal whose remote host
    /// last reported it busy (`set_mirror_surface_busy`).
    pub fn is_surface_busy(&self, surface_id: u32) -> bool {
        self.is_locally_or_mirror_busy(surface_id)
    }

    /// The cached foreground process name for the given surface (resolved by the
    /// last `refresh_busy_surfaces` poll). The StatusBar reads this every frame
    /// instead of re-snapshotting all system processes; `None` until the first
    /// poll resolves the surface (≤1s after spawn) or if it has no PID.
    pub fn foreground_name(&self, surface_id: u32) -> Option<&str> {
        self.foreground_names.get(&surface_id).map(String::as_str)
    }

    /// The current foreground "incarnation" generation for a surface — bumped
    /// every time its resolved foreground process **name** changes (shell↔TUI or
    /// TUI↔TUI transitions alike), cached by the last `refresh_busy_surfaces`
    /// poll. `0` until the first poll resolves the surface.
    ///
    /// Used to tell whether a banner pinned to a specific foreground instance
    /// (`BannerState::origin_generation`) is still about *this* incarnation or a
    /// stale one from before a transition — the auto-close condition in
    /// `App::poll_busy_states` is `origin_generation != foreground_generation`.
    ///
    /// Name-based, so back-to-back runs of the *same* program (`vim` exits,
    /// `vim` starts again before the next poll) are not distinguished as a new
    /// incarnation — a known limitation shared with the mouse-capture blacklist
    /// matching (ADR-0055), which is also name-based rather than pid-based.
    pub fn foreground_generation(&self, surface_id: u32) -> u64 {
        self.foreground_generation
            .get(&surface_id)
            .copied()
            .unwrap_or(0)
    }

    /// Union of the local (foreground-process) and mirror (remote-forwarded)
    /// busy sets — the single "is this surface busy" predicate every consumer
    /// (status dot, status bar, `surface.list` IPC) should use. A surface is
    /// only ever a member of one of the two underlying sets (mirror terminals
    /// have no local PTY to poll; local terminals never receive `Activity`
    /// pushes), so there is no precedence to resolve.
    fn is_locally_or_mirror_busy(&self, surface_id: u32) -> bool {
        self.busy_surfaces.contains(&surface_id) || self.mirror_busy_surfaces.contains(&surface_id)
    }

    /// Whether any surface in the given list is busy.
    // 이유: 현재 실제 호출처가 전부 #[cfg(test)] — 과거 engine.rs → core/ 재배치로
    // core 가 pub(crate) 로 캡슐화되며 드러남.
    #[allow(dead_code)]
    pub fn any_busy(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|&sid| self.is_locally_or_mirror_busy(sid))
    }

    /// Number of busy surfaces among the given list.
    pub fn busy_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|&&sid| self.is_locally_or_mirror_busy(sid))
            .count()
    }

    /// Set (or clear) a **mirror** terminal's busy state, driven by a
    /// `StreamControl::Activity` push from the remote host it mirrors
    /// (`app/attach_client.rs`). This is the only writer of
    /// `mirror_busy_surfaces` — local terminals are never touched here.
    pub fn set_mirror_surface_busy(&mut self, surface_id: u32, busy: bool) {
        if busy {
            self.mirror_busy_surfaces.insert(surface_id);
        } else {
            self.mirror_busy_surfaces.remove(&surface_id);
        }
    }

    /// Drop a mirror terminal's tracked busy state entirely (surface removed —
    /// mirror workspace/session torn down, or a structural delta dropped it).
    pub fn forget_mirror_surface_busy(&mut self, surface_id: u32) {
        self.mirror_busy_surfaces.remove(&surface_id);
    }

    /// Occupied-surface busy transitions ready to forward over the attach
    /// stream, as `(holder client, surface, busy)` triples. Diffs against
    /// `last_forwarded_busy` so a client only gets a push when the value
    /// actually flips — dropped/lagged frames self-heal on the next 1Hz tick
    /// since this always re-diffs from the live `busy_surfaces` set, never from
    /// a client ack. Entries for surfaces no longer hard-occupied are dropped
    /// from the cache on every call, so a later re-attach (possibly by a
    /// different client) always gets a fresh initial push.
    ///
    /// Only ever considers `busy_surfaces` (this instance's own local
    /// foreground-process polling) — the attach lock registry only ever holds
    /// locks over surfaces *this* instance hosts (real or deferred PTYs), never
    /// over its own mirror terminals, so `mirror_busy_surfaces` is irrelevant
    /// here by construction.
    pub fn busy_activity_forwards(
        &mut self,
    ) -> Vec<(crate::core::attach::AttachClientId, u32, bool)> {
        let locks = self.attach.locks_snapshot();
        let occupied: std::collections::HashSet<u32> = locks.iter().map(|&(sid, _)| sid).collect();
        self.last_forwarded_busy
            .retain(|sid, _| occupied.contains(sid));
        let mut out = Vec::new();
        for (sid, lock) in locks {
            let busy = self.busy_surfaces.contains(&sid);
            if self.last_forwarded_busy.get(&sid) != Some(&busy) {
                self.last_forwarded_busy.insert(sid, busy);
                out.push((lock.holder, sid, busy));
            }
        }
        out
    }
}

/// 이전/현재 foreground 이름 스냅샷을 비교해 이름이 바뀐 surface 의 generation 을 1
/// 올린다(처음 관측되는 surface 도 `None != Some(name)` 이라 1로 시작). 더 이상 foreground
/// 가 resolve 되지 않는(surface 가 닫혔거나 PTY 를 잃은) surface 의 엔트리는 제거한다 —
/// `foreground_names` 와 동일하게 stale 값이 남지 않아야 한다.
///
/// 순수 함수(engine 비의존)라 `refresh_busy_surfaces` 호출 없이 결정론적으로 테스트한다.
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

    /// mirror surface 는 로컬 PTY 가 없어 `refresh_busy_surfaces` 가 절대 채우지
    /// 못하는 `busy_surfaces` 와 별개로, `set_mirror_surface_busy` 로만 채워지는
    /// `mirror_busy_surfaces` 를 통해 `busy_count`/`is_surface_busy` 가 이 버그의
    /// 근본 원인(원격 활동 상태가 로컬로 전달되지 않음)이 고쳐졌음을 검증한다.
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

    /// `refresh_busy_surfaces` 는 로컬 폴링 결과로 `busy_surfaces` 를 통째로
    /// 교체한다 — 이 교체가 `mirror_busy_surfaces` 는 건드리지 않아야 한다(교체
    /// 됐다면 매 1Hz tick 마다 mirror 활동 표시가 사라지는 회귀).
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

    /// `forget_mirror_surface_busy` 는 mirror surface 제거(cleanup/구조 delta) 시
    /// 호출되는 경로 — 잔류 stale 값이 남지 않아야 한다.
    #[test]
    fn forget_mirror_surface_busy_clears_entry() {
        let mut e = engine();
        e.set_mirror_surface_busy(42, true);
        e.forget_mirror_surface_busy(42);
        assert!(!e.is_surface_busy(42));
    }

    /// `is_surface_mouse_capture_banner_suppressed` 는 캐시(`mouse_capture_banner_suppressed_surfaces`)
    /// 를 그대로 읽는 접근자다 — `is_surface_mouse_capture_disabled` 와 대칭.
    #[test]
    fn mouse_capture_banner_suppressed_accessor_reads_cache() {
        let mut e = engine();
        assert!(!e.is_surface_mouse_capture_banner_suppressed(42));
        e.mouse_capture_banner_suppressed_surfaces.insert(42);
        assert!(e.is_surface_mouse_capture_banner_suppressed(42));
    }

    /// `refresh_busy_surfaces` 는 로컬 PTY 가 없는 surface 에 대해 캡처 블랙리스트
    /// 캐시와 배너 억제 캐시를 각각 독립적으로 통째로 교체한다(닫힌 surface 의 stale
    /// 엔트리가 남지 않음 — 두 캐시 모두 대칭적으로 동작해야 한다).
    #[test]
    fn refresh_busy_surfaces_replaces_both_mouse_capture_caches() {
        let mut e = engine();
        e.mouse_capture_disabled_surfaces.insert(99);
        e.mouse_capture_banner_suppressed_surfaces.insert(99);
        e.refresh_busy_surfaces(); // 로컬 터미널이 없으니 두 캐시 모두 빈 채로 재계산.
        assert!(!e.is_surface_mouse_capture_disabled(99));
        assert!(!e.is_surface_mouse_capture_banner_suppressed(99));
    }

    /// `busy_activity_forwards` 는 값이 실제로 바뀔 때만 forward 한다(중복 억제).
    #[test]
    fn busy_activity_forwards_only_on_change() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");

        // 최초 호출: 아직 busy 아님(idle) → 최초 diff(None → false)라 1건 forward.
        let first = e.busy_activity_forwards();
        assert_eq!(first, vec![(7, sid, false)]);

        // 같은 상태 재호출 — 변화 없으니 forward 없음.
        assert!(e.busy_activity_forwards().is_empty());

        // busy 로 전환 → 1건 forward.
        e.busy_surfaces.insert(sid);
        assert_eq!(e.busy_activity_forwards(), vec![(7, sid, true)]);

        // 다시 재호출 — 여전히 변화 없음.
        assert!(e.busy_activity_forwards().is_empty());
    }

    /// lock 해제 후 재획득(다른 client)하면, 값이 이전과 같아도 항상 fresh 하게
    /// 1건 forward 해야 한다 — stale 캐시로 신규 holder 가 초기 push 를 못 받는
    /// 회귀를 막는다.
    #[test]
    fn busy_activity_forwards_resets_on_reacquire() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");
        assert_eq!(e.busy_activity_forwards(), vec![(7, sid, false)]);
        assert!(e.busy_activity_forwards().is_empty());

        e.attach.release(sid, 7).expect("release");
        // 점유 해제 — 다음 diff 호출에서 캐시가 정리된다(occupied 집합에서 빠짐).
        assert!(e.busy_activity_forwards().is_empty());

        e.attach.acquire(sid, 9).expect("다른 client 재획득");
        assert_eq!(
            e.busy_activity_forwards(),
            vec![(9, sid, false)],
            "재획득 후에는 값이 이전과 같아도(false) 새 holder 에게 다시 push"
        );
    }

    /// 이전 tick=vim(비-쉘), 이번 tick=bash(쉘) — 이름이 바뀌었으니 generation 이
    /// 올라간다("전이가 감지된다"). 같은 이름이 유지되면 올라가지 않는다.
    #[test]
    fn foreground_generation_bumps_on_name_change_and_holds_when_unchanged() {
        let mut gens = std::collections::HashMap::new();
        let mut old = std::collections::HashMap::new();
        let mut new = std::collections::HashMap::new();

        // 최초 관측(surface 7 = vim) — 처음 보는 surface 도 새 incarnation 으로 센다.
        new.insert(7u32, "vim".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 1);

        // 같은 이름 유지 — 증가 없음.
        old = new.clone();
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 1);

        // vim(비-쉘) → bash(쉘) 전이 — 증가.
        old = new.clone();
        new.insert(7, "bash".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 2);

        // TUI → TUI 전이(쉘 경유 없음)도 동일하게 감지된다.
        old = new.clone();
        new.insert(7, "htop".to_string());
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert_eq!(gens[&7], 3);
    }

    /// surface 가 닫혀 더 이상 foreground 가 resolve 되지 않으면 generation 엔트리도
    /// `foreground_names` 와 동일하게 정리된다(stale 잔류 방지).
    ///
    /// ★ **살아 있는 surface 8 을 함께 둔다.** `new_names` 가 비면 프로덕션의
    /// `retain(|sid, _| new_names.contains_key(sid))` 는 그 입력에서 `clear()` 와
    /// 글자 그대로 같은 함수가 되고, 그러면 "안 보이는 것만 솎는다" 와 "전부 지운다" 가
    /// 구별되는 관측을 하나도 안 만든다 — 변이를 돌려볼 것도 없이 두 구현이 이미 같다.
    /// 8 이 살아남는다는 단정이 그 둘을 가른다(덤으로 bump 갈래도 함께 잡힌다).
    #[test]
    fn foreground_generation_prunes_entries_for_surfaces_no_longer_resolved() {
        let mut gens = std::collections::HashMap::new();
        gens.insert(8u32, 0u64);
        gens.insert(9u32, 3u64);
        let old = std::collections::HashMap::new();
        // surface 8 은 계속 관측되고, 9 는 더 이상 안 보인다.
        let new = std::collections::HashMap::from([(8u32, "sh".to_string())]);
        super::bump_foreground_generations(&mut gens, &old, &new);
        assert!(!gens.contains_key(&9));
        assert_eq!(gens.get(&8), Some(&1));
    }

    /// `foreground_generation` 접근자는 캐시를 그대로 읽고, 관측 전 surface 는 0.
    #[test]
    fn foreground_generation_accessor_reads_cache_and_defaults_to_zero() {
        let mut e = engine();
        assert_eq!(e.foreground_generation(42), 0);
        e.foreground_generation.insert(42, 5);
        assert_eq!(e.foreground_generation(42), 5);
    }
}
