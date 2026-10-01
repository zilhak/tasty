//! 모달을 제외한 MainView와 engine의 대상 자원을 찾는다.
//!
//! engine은 모두 App의 [`EngineRegistry`]에 있고 창·parked·임시는 관계 상태다.
//! 어느 관계든 [`EngineId`]로 식별한다. engine만 다루는 순회는
//! [`EngineScan`]·[`EngineScanMut`]을 거친다. 창 View 작업이 함께 필요한 창 루프와
//! 창 ID로 고른 engine 접근은 `window_pairs`·`window_pair`로 MainView와 engine을 함께 빌린다.
//! engine 소유 구조가 바뀌면 이 두 타입과 [`engines_mut!`]를 함께 고친다.

use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::collections::{HashMap, HashSet};

use winit::window::WindowId;

use crate::app::App;
use crate::app::engine_registry::{EngineRegistry, ParkedView};
use crate::core::layout_persistence::LayoutSlotId;
use crate::runtime::engine_session::EngineId;
use crate::state::MainViewState;
use crate::view;
use crate::view::main::MainView;

type ViewMap = HashMap<WindowId, Box<dyn view::ui::View>>;

/// App의 View·engine 필드만 가변으로 빌린다.
/// `&mut self` 메서드와 달리 같은 함수에서 `self.services` 같은 다른 필드를 함께 빌릴 수 있다.
macro_rules! engines_mut {
    ($app:expr) => {
        $crate::app::window_access::EngineScanMut::from_fields(
            &mut $app.view.views,
            &mut $app.engines,
        )
    };
}
pub(crate) use engines_mut;

/// 창·parked·임시 engine을 읽기 전용으로 순회한다.
///
/// 방문 순서는 창(`views` 순회 순서) → parked(보관 순서) → 임시 engine이다.
/// 한 engine은 세 관계 중 한 곳에만 있으므로 [`all`](Self::all)은 각 engine을 정확히 한 번 방문한다.
/// 모달 View는 engine이 없어 건너뛴다.
#[derive(Clone, Copy)]
pub(crate) struct EngineScan<'a> {
    views: &'a ViewMap,
    engines: &'a EngineRegistry,
}

impl<'a> EngineScan<'a> {
    pub(crate) fn from_fields(views: &'a ViewMap, engines: &'a EngineRegistry) -> Self {
        Self { views, engines }
    }

    /// 창 engine과 그 창의 ID.
    pub(crate) fn windows(self) -> impl Iterator<Item = (WindowId, EngineRef<'a>)> {
        self.engines.windows_in(self.views.keys().copied())
    }

    /// 창 MainView와 그 engine.
    pub(crate) fn window_pairs(
        self,
    ) -> impl Iterator<Item = (WindowId, &'a MainView, EngineRef<'a>)> {
        let engines = self.engines;
        self.views.iter().filter_map(move |(wid, w)| {
            let main = w.as_main()?;
            engines.window_engine(*wid).map(|e| (*wid, main, e))
        })
    }

    /// 창 ID로 고른 MainView와 그 engine.
    pub(crate) fn window_pair(self, wid: WindowId) -> Option<(&'a MainView, EngineRef<'a>)> {
        let main = self.views.get(&wid)?.as_main()?;
        self.engines.window_engine(wid).map(|e| (main, e))
    }

    /// 창과 parked 항목의 MainViewState·engine 쌍. 창 → parked 순서다.
    pub(crate) fn sessions(self) -> impl Iterator<Item = (&'a MainViewState, EngineRef<'a>)> {
        self.window_pairs()
            .map(|(_, m, e)| (&m.state, e))
            .chain(self.parked_sessions())
    }

    pub(crate) fn parked(self) -> impl Iterator<Item = EngineRef<'a>> {
        self.engines.parked_sessions().map(|(_, _, e)| e)
    }

    /// parked engine과 그 id. 보관 순서다.
    pub(crate) fn parked_with_ids(self) -> impl Iterator<Item = (EngineId, EngineRef<'a>)> {
        self.engines.parked_sessions().map(|(id, _, e)| (id, e))
    }

    pub(crate) fn parked_sessions(
        self,
    ) -> impl Iterator<Item = (&'a MainViewState, EngineRef<'a>)> {
        self.engines.parked_sessions().map(|(_, s, e)| (s, e))
    }

    pub(crate) fn parked_count(self) -> usize {
        self.engines.parked_count()
    }

    /// 창에 배정되기 전의 임시 engine.
    pub(crate) fn pending(self) -> Option<EngineRef<'a>> {
        self.engines.pending()
    }

    /// 공개된 창 engine을 우선하고, 첫 부팅처럼 창이 없을 때만 임시 engine을 쓴다.
    pub(crate) fn primary(self) -> Option<EngineRef<'a>> {
        self.windows()
            .next()
            .map(|(_, engine)| engine)
            .or_else(|| self.pending())
    }

    /// 창 → parked 순서. 임시 engine은 제외한다.
    pub(crate) fn windowed_and_parked(self) -> impl Iterator<Item = EngineRef<'a>> {
        self.windows().map(|(_, e)| e).chain(self.parked())
    }

    /// 창 → parked → 임시 순서로 모든 engine을 한 번씩 방문한다.
    pub(crate) fn all(self) -> impl Iterator<Item = EngineRef<'a>> {
        self.windowed_and_parked().chain(self.pending())
    }
}

/// [`EngineScanMut`]이 한 번에 나눠 빌린 필드. engine 참조는 id별로 한 번만 꺼낸다.
struct SplitMut<'a> {
    views: &'a mut ViewMap,
    by_id: HashMap<EngineId, EngineMut<'a>>,
    by_window: &'a HashMap<WindowId, EngineId>,
    parked: &'a mut Vec<ParkedView>,
    pending: Option<EngineId>,
}

/// 창·parked·임시 engine을 가변으로 순회한다. 순서와 1회 방문 성질은 [`EngineScan`]과 같다.
/// 메서드는 핸들을 소비한다. 한 함수에서 여러 번 쓰려면 [`reborrow`](Self::reborrow)한다.
pub(crate) struct EngineScanMut<'a> {
    views: &'a mut ViewMap,
    engines: &'a mut EngineRegistry,
}

impl<'a> EngineScanMut<'a> {
    /// [`engines_mut!`]가 쓴다. 호출부에서 필드를 직접 넘기지 않는다.
    pub(crate) fn from_fields(views: &'a mut ViewMap, engines: &'a mut EngineRegistry) -> Self {
        Self { views, engines }
    }

    pub(crate) fn reborrow(&mut self) -> EngineScanMut<'_> {
        EngineScanMut {
            views: self.views,
            engines: self.engines,
        }
    }

    fn split(self) -> SplitMut<'a> {
        let (by_id, by_window, parked, pending) = self.engines.split_by_id();
        SplitMut {
            views: self.views,
            by_id,
            by_window,
            parked,
            pending,
        }
    }

    pub(crate) fn windows(self) -> impl Iterator<Item = (WindowId, EngineMut<'a>)> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            ..
        } = self.split();
        let views: &'a ViewMap = views;
        views.keys().filter_map(move |wid| {
            let id = by_window.get(wid)?;
            by_id.remove(id).map(|e| (*wid, e))
        })
    }

    /// 창 MainView와 그 engine. 창 순회 순서다.
    pub(crate) fn window_pairs(
        self,
    ) -> impl Iterator<Item = (WindowId, &'a mut MainView, EngineMut<'a>)> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            ..
        } = self.split();
        views.iter_mut().filter_map(move |(wid, w)| {
            let id = by_window.get(wid)?;
            let main = w.as_main_mut()?;
            by_id.remove(id).map(|e| (*wid, main, e))
        })
    }

    /// [`window_pairs`](Self::window_pairs)에 engine id를 더한다. 사건을 engine 기준으로 되돌려 보낼 때 쓴다.
    pub(crate) fn window_entries(
        self,
    ) -> impl Iterator<Item = (EngineId, &'a mut MainView, EngineMut<'a>)> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            ..
        } = self.split();
        views.iter_mut().filter_map(move |(wid, w)| {
            let id = *by_window.get(wid)?;
            let main = w.as_main_mut()?;
            by_id.remove(&id).map(|e| (id, main, e))
        })
    }

    /// 창 ID로 고른 MainView와 그 engine.
    pub(crate) fn window_pair(self, wid: WindowId) -> Option<(&'a mut MainView, EngineMut<'a>)> {
        let main = self.views.get_mut(&wid)?.as_main_mut()?;
        self.engines.window_engine_mut(wid).map(|e| (main, e))
    }

    /// 창의 MainViewState·engine 쌍.
    pub(crate) fn window_sessions(
        self,
    ) -> impl Iterator<Item = (&'a mut MainViewState, EngineMut<'a>)> {
        self.window_pairs().map(|(_, m, e)| (&mut m.state, e))
    }

    /// 창과 parked 항목의 MainViewState·engine 쌍. 창 → parked 순서다.
    pub(crate) fn sessions(self) -> impl Iterator<Item = (&'a mut MainViewState, EngineMut<'a>)> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            parked,
            ..
        } = self.split();
        let windowed: Vec<(&'a mut MainViewState, EngineMut<'a>)> = views
            .iter_mut()
            .filter_map(|(wid, w)| {
                let id = by_window.get(wid)?;
                let main = w.as_main_mut()?;
                by_id.remove(id).map(|e| (&mut main.state, e))
            })
            .collect();
        windowed.into_iter().chain(
            parked
                .iter_mut()
                .filter_map(move |p| by_id.remove(&p.engine).map(|e| (&mut p.state, e))),
        )
    }

    pub(crate) fn parked(self) -> impl Iterator<Item = EngineMut<'a>> {
        self.engines.parked_sessions_mut().map(|(_, _, e)| e)
    }

    /// parked 항목의 id·MainViewState·engine. id는 `DispatchSource::Engine`에 싣는 값이다.
    pub(crate) fn parked_sessions_with_ids(
        self,
    ) -> impl Iterator<Item = (EngineId, &'a mut MainViewState, EngineMut<'a>)> {
        self.engines.parked_sessions_mut()
    }

    /// parked 항목의 MainViewState·engine 쌍. 보관 순서다.
    pub(crate) fn parked_sessions(
        self,
    ) -> impl Iterator<Item = (&'a mut MainViewState, EngineMut<'a>)> {
        self.engines.parked_sessions_mut().map(|(_, s, e)| (s, e))
    }

    /// id로 parked 항목을 찾는다. 사이에 다른 항목을 보관하거나 꺼내도 같은 engine을 가리킨다.
    pub(crate) fn parked_session(
        self,
        id: EngineId,
    ) -> Option<(&'a mut MainViewState, EngineMut<'a>)> {
        self.engines.parked_session_mut(id)
    }

    /// 가장 먼저 보관한 parked 항목. 대상 없는 요청의 기본 engine이다.
    pub(crate) fn first_parked_session(self) -> Option<(&'a mut MainViewState, EngineMut<'a>)> {
        self.parked_sessions().next()
    }

    /// 요청 대상 자원을 가진 parked 항목. 창의 소유 판정과 같은 `engine_has_resource`를 쓴다.
    pub(crate) fn parked_session_with_resource(
        self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<(&'a mut MainViewState, EngineMut<'a>)> {
        self.parked_sessions()
            .find(|(_, e)| crate::core::request_target::engine_has_resource(&e.as_ref(), rid))
    }

    pub(crate) fn pending(self) -> Option<EngineMut<'a>> {
        self.engines.pending_mut()
    }

    /// 창 → 임시 순서. parked engine은 제외한다.
    pub(crate) fn windows_and_pending(self) -> impl Iterator<Item = EngineMut<'a>> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            pending,
            ..
        } = self.split();
        let views: &'a ViewMap = views;
        let windowed: Vec<EngineMut<'a>> = views
            .keys()
            .filter_map(|wid| by_window.get(wid).and_then(|id| by_id.remove(id)))
            .collect();
        windowed
            .into_iter()
            .chain(pending.and_then(move |id| by_id.remove(&id)))
    }

    /// 창 → parked 순서. 임시 engine은 제외한다.
    pub(crate) fn windowed_and_parked(self) -> impl Iterator<Item = EngineMut<'a>> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            parked,
            ..
        } = self.split();
        let views: &'a ViewMap = views;
        let windowed: Vec<EngineMut<'a>> = views
            .keys()
            .filter_map(|wid| by_window.get(wid).and_then(|id| by_id.remove(id)))
            .collect();
        let parked: &'a Vec<ParkedView> = parked;
        windowed
            .into_iter()
            .chain(parked.iter().filter_map(move |p| by_id.remove(&p.engine)))
    }

    /// 요청이 가리킨 engine과 그 MainViewState. 창이 있으면 그 창의 ViewBase도 준다.
    pub(crate) fn resolve(self, id: EngineId) -> Option<DispatchCtx<'a>> {
        let SplitMut {
            views,
            mut by_id,
            by_window,
            parked,
            ..
        } = self.split();
        let engine = by_id.remove(&id)?;
        if let Some((wid, _)) = by_window.iter().find(|(_, e)| **e == id) {
            let MainView { state, base, .. } = views.get_mut(wid)?.as_main_mut()?;
            return Some(DispatchCtx {
                state,
                engine,
                view: Some(base),
            });
        }
        let p = parked.iter_mut().find(|p| p.engine == id)?;
        Some(DispatchCtx {
            state: &mut p.state,
            engine,
            view: None,
        })
    }

    /// 마지막 창을 닫거나 백그라운드로 보낼 때 창 관계를 parked로 바꾼다. 뒤에 붙인다.
    pub(crate) fn park(self, wid: WindowId, view_restore: MainViewState) -> Option<EngineId> {
        self.engines.park(wid, view_restore)
    }

    /// 새 창에 넘길 parked 항목을 가장 먼저 보관한 것부터 임시 관계로 옮긴다.
    pub(crate) fn unpark_first(self) -> Option<(EngineId, MainViewState)> {
        self.engines.unpark_first()
    }
}

/// [`EngineScanMut::resolve`]가 준 요청 대상. 창이 없는 parked engine이면 `view`가 없다.
pub(crate) struct DispatchCtx<'a> {
    pub(crate) state: &'a mut MainViewState,
    pub(crate) engine: EngineMut<'a>,
    pub(crate) view: Option<&'a mut view::ViewBase>,
}

impl App {
    pub(crate) fn engines(&self) -> EngineScan<'_> {
        EngineScan::from_fields(&self.view.views, &self.engines)
    }

    pub(crate) fn engines_mut(&mut self) -> EngineScanMut<'_> {
        engines_mut!(self)
    }

    /// 요청한 engine이 창에 있으면 그 창을 다시 그리게 한다. parked engine이면 아무것도 하지 않는다.
    pub(crate) fn mark_source_window_dirty(
        &mut self,
        source: crate::app::dispatch_domain::DispatchSource,
    ) {
        if let Some(wid) = self.engines.window_of(source.engine())
            && let Some(w) = self.view.views.get_mut(&wid)
        {
            w.mark_dirty();
        }
    }
}

impl App {
    pub(crate) fn focused_window(&self) -> Option<&view::main::MainView> {
        self.view
            .focused_view_id
            .and_then(|id| self.view.views.get(&id))
            .and_then(|w| w.as_main())
    }

    /// 포커스된 MainView와 그 engine.
    pub(crate) fn focused_pair(&self) -> Option<(&MainView, EngineRef<'_>)> {
        self.engines().window_pair(self.view.focused_view_id?)
    }

    pub(crate) fn focused_window_mut(&mut self) -> Option<&mut view::main::MainView> {
        self.view
            .focused_view_id
            .and_then(|id| self.view.views.get_mut(&id))
            .and_then(|w| w.as_main_mut())
    }

    /// 안내는 포커스가 모달에 있어도 다른 MainView에 표시할 수 있다.
    pub(crate) fn notice_window_mut(&mut self) -> Option<&mut view::main::MainView> {
        let id = match self.focused_window() {
            Some(_) => self.view.focused_view_id,
            None => self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id),
        }?;
        self.view.views.get_mut(&id).and_then(|w| w.as_main_mut())
    }

    pub(crate) fn main_windows_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut view::main::MainView> {
        self.view.views.values_mut().filter_map(|w| w.as_main_mut())
    }

    pub(crate) fn main_window_count(&self) -> usize {
        self.view
            .views
            .values()
            .filter(|w| w.as_main().is_some())
            .count()
    }

    /// 새 engine이 공용 레지스트리·ID 발급기를 공유할 기존 engine을 찾는다. 창을 먼저, parked를 나중에 본다.
    pub(crate) fn any_main_engine(&self) -> Option<EngineRef<'_>> {
        self.engines().windowed_and_parked().next()
    }

    /// 점유를 별도로 관리하지 않고 살아 있는 engine의 슬롯을 모은다.
    /// 창에 등록되기 전 임시 engine도 포함해야 중복 배정을 피할 수 있다.
    pub(crate) fn occupied_layout_slots(&self) -> HashSet<LayoutSlotId> {
        let mut occupied = occupied_slots(self.engines());
        occupied.extend(self.engines.retiring_slots());
        occupied
    }

    /// 빈 슬롯을 고르기만 한다. engine이 만들어져야 실제 점유로 센다.
    pub(crate) fn claim_free_layout_slot(&self) -> LayoutSlotId {
        let mut slots = crate::core::layout_persistence::list_slots();
        slots.retain(|slot| !self.journal.layout_slot_retired(*slot));
        slots.extend(self.journal.known_layout_slots());
        slots.sort_unstable();
        slots.dedup();
        pick_free_slot(&slots, &self.occupied_layout_slots())
    }

    pub(crate) fn find_main_with_surface(&self, surface_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.has_surface(surface_id))
            .map(|(wid, _)| wid)
    }

    pub(crate) fn find_main_with_workspace(&self, workspace_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.has_workspace(workspace_id))
            .map(|(wid, _)| wid)
    }

    /// mirror workspace의 engine이 창 또는 parked 상태에 남아 있으면 세션을 유지한다.
    /// cleanup_mirror_workspace·mirror_output_host와 같은 범위를 찾아야 한다.
    /// parked 조회는 find_parked_with_workspace를 공유하지만 창 있는 engine의 순회는 별도다.
    /// 세 경로의 집합이 같은지 자동 비교하는 검사는 없어 함께 검토해야 한다.
    /// 임시 engine은 제외한다. start_gui_attach는 실제 창에만 mirror를 만들고
    /// 임시 engine에는 정리 후 active_workspace를 보정할 MainViewState도 없다. 이 조건이 바뀌면 세 경로를 함께 고친다.
    pub(crate) fn mirror_workspace_engine_alive(&self, workspace_id: u32) -> bool {
        let engines = self.engines();
        any_engine_has_workspace(
            engines.windows().map(|(_, e)| e.core),
            engines.parked_with_ids().map(|(id, e)| (id, e.core)),
            workspace_id,
        )
    }

    pub(crate) fn find_main_with_pane(&self, pane_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.has_pane(pane_id))
            .map(|(wid, _)| wid)
    }

    pub(crate) fn find_main_with_tab(&self, tab_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.find_pane_for_tab(tab_id).is_some())
            .map(|(wid, _)| wid)
    }

    /// PTY 레지스트리는 engine별로 있으므로 ID의 소유 창을 전체 MainView에서 찾는다.
    pub(crate) fn find_main_with_headless_pty(&self, pty_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.runtime.terminals.is_standalone(pty_id))
            .map(|(wid, _)| wid)
    }

    pub(crate) fn find_main_with_resource(
        &self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<WindowId> {
        use crate::core::request_target::Kind;
        // 범위를 넘는 ID를 잘라 다른 자원으로 바꾸지 않는다.
        let narrow = u32::try_from(rid.id).ok();
        match rid.kind {
            Kind::Surface => narrow.and_then(|id| self.find_main_with_surface(id)),
            Kind::Workspace => narrow.and_then(|id| self.find_main_with_workspace(id)),
            Kind::Pane => narrow.and_then(|id| self.find_main_with_pane(id)),
            Kind::Tab => narrow.and_then(|id| self.find_main_with_tab(id)),
            Kind::HeadlessPty => narrow.and_then(|id| self.find_main_with_headless_pty(id)),
            Kind::Hook | Kind::GlobalHook | Kind::Observer | Kind::Category => {
                self.find_main_with_engine_resource(rid)
            }
        }
    }

    /// hook·observer 등은 parked 라우팅과 같은 engine_has_resource 판정을 사용한다.
    fn find_main_with_engine_resource(
        &self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
            .map(|(wid, _)| wid)
    }

    /// 전체 창에서 숫자 ID를 먼저 찾고 없으면 이름을 찾는다.
    /// 같은 이름이 여러 창에 있으면 HashMap 순회 순서로 고르지 않고 거절한다.
    pub(crate) fn find_main_with_workspace_target(
        &self,
        target: &str,
    ) -> Result<Option<WindowId>, String> {
        if let Ok(id) = target.parse::<u32>()
            && let Some(wid) = self.find_main_with_workspace(id)
        {
            return Ok(Some(wid));
        }
        find_workspace_by_name(
            self.engines().windows().map(|(wid, e)| (wid, e.core)),
            target,
        )
    }
}

/// 이름이 일치하는 창이 없으면 Ok(None), 하나면 해당 창, 여럿이면 Err다.
fn find_workspace_by_name<'a>(
    windows: impl Iterator<Item = (WindowId, &'a crate::core::CoreState)>,
    target: &str,
) -> Result<Option<WindowId>, String> {
    let mut matches: Vec<WindowId> = Vec::new();
    for (wid, engine) in windows {
        if engine.workspaces().into_iter().any(|ws| ws.name == target) {
            matches.push(wid);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches[0])),
        n => Err(format!(
            "workspace name '{target}' matches {n} windows, use --surface/--workspace-id instead"
        )),
    }
}

/// 창과 parked engine 양쪽을 확인한다. parked 조회는 mirror 적용·정리 경로와 공유한다.
fn any_engine_has_workspace<'a>(
    mut main: impl Iterator<Item = &'a crate::core::CoreState>,
    parked: impl IntoIterator<Item = (EngineId, &'a crate::core::CoreState)>,
    workspace_id: u32,
) -> bool {
    main.any(|e| e.has_workspace(workspace_id))
        || super::attach_client::find_parked_with_workspace(parked, workspace_id).is_some()
}

/// 모든 engine의 슬롯을 모은다. 창·parked·임시 engine을 [`EngineScan::all`]로 한 번씩 본다.
fn occupied_slots(engines: EngineScan<'_>) -> HashSet<LayoutSlotId> {
    engines.all().filter_map(|e| e.persistence.slot).collect()
}

/// 저장된 레이아웃을 우선 복원하도록 오름차순 files의 첫 미점유 슬롯을 고른다.
/// 모두 점유 중이면 files와 occupied의 최댓값 다음 번호를 쓴다.
/// 아직 파일이 없는 engine의 슬롯도 중복 배정을 피하려고 포함하며, 둘 다 비었으면 1이다.
fn pick_free_slot(files: &[LayoutSlotId], occupied: &HashSet<LayoutSlotId>) -> LayoutSlotId {
    if let Some(free) = files.iter().find(|s| !occupied.contains(s)) {
        return *free;
    }
    let max = files
        .iter()
        .chain(occupied.iter())
        .copied()
        .max()
        .unwrap_or(0);
    max + 1
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn engine_with_workspace_name(name: &str) -> crate::runtime::engine_session::EngineSession {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .name = name.to_string();
        engine_session
    }

    #[test]
    fn ambiguous_workspace_name_across_windows_returns_error() {
        let mut e1_session = engine_with_workspace_name("main");
        let e1 = e1_session.borrow_mut();
        let mut e2_session = engine_with_workspace_name("main");
        let e2 = e2_session.borrow_mut();
        let w1 = WindowId::from(1u64);
        let w2 = WindowId::from(2u64);

        let err = find_workspace_by_name([(w1, &*e1.core), (w2, &*e2.core)].into_iter(), "main")
            .expect_err("이름이 같은 창이 둘이면 Err여야 한다");
        assert!(
            err.contains("main"),
            "에러 메시지에 workspace 이름이 포함돼야 한다: {err}"
        );
        assert!(
            err.contains('2'),
            "에러 메시지에 매칭 개수가 포함돼야 한다: {err}"
        );
    }

    #[test]
    fn unique_workspace_name_still_resolves() {
        let mut e1_session = engine_with_workspace_name("main");
        let e1 = e1_session.borrow_mut();
        let mut e2_session = engine_with_workspace_name("other");
        let e2 = e2_session.borrow_mut();
        let w1 = WindowId::from(1u64);
        let w2 = WindowId::from(2u64);

        assert_eq!(
            find_workspace_by_name([(w1, &*e1.core), (w2, &*e2.core)].into_iter(), "other"),
            Ok(Some(w2))
        );
    }

    #[test]
    fn no_match_returns_ok_none() {
        let mut e1_session = engine_with_workspace_name("main");
        let e1 = e1_session.borrow_mut();
        let w1 = WindowId::from(1u64);

        assert_eq!(
            find_workspace_by_name([(w1, &*e1.core)].into_iter(), "nonexistent"),
            Ok(None)
        );
    }

    /// 이름 순서대로 park한 registry와 각 id.
    fn parked(names: &[&str]) -> (EngineRegistry, Vec<EngineId>) {
        let mut reg = EngineRegistry::default();
        let ids = names
            .iter()
            .map(|name| {
                let (state, mut engine_session) = crate::state::tests::test_state();
                let mut engine = engine_session.borrow_mut();
                engine
                    .workspace_at_mut(0)
                    .expect("workspace index is valid")
                    .name = (*name).to_string();
                reg.park_for_test(state, engine_session)
            })
            .collect();
        (reg, ids)
    }

    fn engine_mut(reg: &mut EngineRegistry, id: EngineId) -> &mut crate::core::CoreState {
        reg.get_mut(id).expect("registry에 있는 engine").core
    }

    fn parked_pairs(
        reg: &EngineRegistry,
    ) -> impl Iterator<Item = (EngineId, &crate::core::CoreState)> {
        reg.parked_sessions().map(|(id, _, e)| (id, e.core))
    }

    #[test]
    fn workspace_in_windowed_engine_is_not_orphaned() {
        let mut windowed_session = engine_with_workspace_name("mirror");
        let windowed = windowed_session.borrow_mut();
        let ws = windowed
            .workspace_at(0)
            .expect("workspace index is valid")
            .id;
        assert!(any_engine_has_workspace(
            [&*windowed.core].into_iter(),
            std::iter::empty(),
            ws
        ));
    }

    #[test]
    fn workspace_only_in_parked_engine_is_not_orphaned() {
        let (reg, ids) = parked(&["mirror"]);
        let ws = reg
            .get(ids[0])
            .expect("engine")
            .workspace_at(0)
            .expect("workspace index is valid")
            .id;
        assert!(any_engine_has_workspace(
            std::iter::empty(),
            parked_pairs(&reg),
            ws
        ));
    }

    #[test]
    fn workspace_in_later_parked_engine_is_not_orphaned() {
        let (mut reg, ids) = parked(&["first", "second"]);
        // 첫 engine과 겹치지 않는 ID를 두 번째 engine에만 만든다.
        let target = engine_mut(&mut reg, ids[0])
            .workspace_at(0)
            .expect("workspace index is valid")
            .id
            + 5_000;
        engine_mut(&mut reg, ids[1])
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .id = target;
        assert!(any_engine_has_workspace(
            std::iter::empty(),
            parked_pairs(&reg),
            target
        ));
    }

    #[test]
    fn workspace_in_no_engine_is_orphaned() {
        let mut windowed_session = engine_with_workspace_name("local");
        let windowed = windowed_session.borrow_mut();
        let (reg, ids) = parked(&["other"]);
        let missing = windowed
            .workspace_at(0)
            .expect("workspace index is valid")
            .id
            + reg
                .get(ids[0])
                .expect("engine")
                .workspace_at(0)
                .expect("workspace index is valid")
                .id
            + 1_000;
        assert!(!any_engine_has_workspace(
            [&*windowed.core].into_iter(),
            parked_pairs(&reg),
            missing
        ));
    }

    fn names<'a>(engines: impl Iterator<Item = EngineRef<'a>>) -> Vec<String> {
        engines
            .map(|e| {
                e.workspace_at(0)
                    .expect("workspace index is valid")
                    .name
                    .clone()
            })
            .collect()
    }

    fn with_pending(
        reg: &mut EngineRegistry,
        engine: crate::runtime::engine_session::EngineSession,
    ) -> EngineId {
        reg.insert_pending(engine)
            .unwrap_or_else(|_| panic!("임시 engine이 이미 있다"))
    }

    #[test]
    fn scan_visits_parked_then_pending_once_each() {
        let (mut reg, _) = parked(&["p0", "p1"]);
        with_pending(&mut reg, engine_with_workspace_name("tmp"));
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &reg);
        assert_eq!(names(scan.all()), ["p0", "p1", "tmp"]);
        assert_eq!(names(scan.windowed_and_parked()), ["p0", "p1"]);
        assert_eq!(names(scan.parked()), ["p0", "p1"]);
        assert_eq!(scan.parked_count(), 2);
        assert_eq!(
            names(scan.primary().into_iter()),
            ["tmp"],
            "창이 없으면 임시 engine을 쓴다"
        );
    }

    #[test]
    fn scan_without_pending_has_no_primary() {
        let (reg, _) = parked(&["p0"]);
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &reg);
        assert!(scan.primary().is_none(), "primary는 parked를 보지 않는다");
        assert_eq!(names(scan.all()), ["p0"]);
    }

    #[test]
    fn occupied_slots_counts_each_engine_once() {
        let (mut reg, ids) = parked(&["p0", "p1"]);
        engine_mut(&mut reg, ids[0]).persistence.slot = Some(4);
        engine_mut(&mut reg, ids[1]).persistence.slot = Some(2);
        let mut pending_session = engine_with_workspace_name("tmp");
        let mut pending = pending_session.borrow_mut();
        pending.persistence.slot = Some(7);
        with_pending(&mut reg, pending_session);
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &reg);
        assert_eq!(occupied_slots(scan), slots(&[2, 4, 7]));
        assert_eq!(
            scan.all().filter(|e| e.persistence.slot.is_some()).count(),
            3,
            "슬롯을 가진 engine이 각각 한 번만 보인다"
        );
    }

    /// View가 없는 창 관계는 창 순회에 나오지 않는다. 창 순회는 View 목록이 정한다.
    #[test]
    fn window_relation_without_a_view_is_not_scanned() {
        let mut reg = EngineRegistry::default();
        let id = with_pending(&mut reg, engine_with_workspace_name("w"));
        reg.attach_window(WindowId::from(1u64), id);
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &reg);
        assert!(scan.windows().next().is_none());
        assert!(scan.all().next().is_none());
    }

    #[test]
    fn scan_mut_keeps_order_and_excludes_by_place() {
        let (mut reg, _) = parked(&["p0", "p1"]);
        with_pending(&mut reg, engine_with_workspace_name("tmp"));
        let mut views = HashMap::new();
        let mut scan = EngineScanMut::from_fields(&mut views, &mut reg);
        let visit = |it: &mut dyn Iterator<Item = EngineMut<'_>>| -> Vec<String> {
            it.map(|e| {
                e.workspace_at(0)
                    .expect("workspace index is valid")
                    .name
                    .clone()
            })
            .collect()
        };
        assert_eq!(
            visit(&mut scan.reborrow().windowed_and_parked()),
            ["p0", "p1"]
        );
        assert_eq!(visit(&mut scan.reborrow().windows_and_pending()), ["tmp"]);
        assert_eq!(visit(&mut scan.reborrow().parked()), ["p0", "p1"]);
        let sessions: Vec<String> = scan
            .reborrow()
            .sessions()
            .map(|(_, e)| {
                e.workspace_at(0)
                    .expect("workspace index is valid")
                    .name
                    .clone()
            })
            .collect();
        assert_eq!(
            sessions,
            ["p0", "p1"],
            "sessions는 임시 engine을 넣지 않는다"
        );
        assert_eq!(
            scan.reborrow().pending().map(|e| e
                .workspace_at(0)
                .expect("workspace index is valid")
                .name
                .clone()),
            Some("tmp".to_string())
        );
    }

    #[test]
    fn park_appends_and_unpark_takes_the_oldest() {
        let (mut reg, ids) = parked(&["p0"]);
        let mut views = HashMap::new();
        let (state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .name = "p1".to_string();
        let p1 = with_pending(&mut reg, engine_session);
        let w = WindowId::from(1u64);
        reg.attach_window(w, p1);
        let mut scan = EngineScanMut::from_fields(&mut views, &mut reg);
        assert_eq!(scan.reborrow().park(w, state), Some(p1));
        let (first, _) = scan.reborrow().unpark_first().expect("보관한 항목");
        assert_eq!(first, ids[0]);
        assert!(
            scan.reborrow().unpark_first().is_none(),
            "임시 engine이 창에 붙기 전에는 다음 항목을 꺼내지 않는다"
        );
        reg.attach_window(WindowId::from(2u64), first);
        let mut scan = EngineScanMut::from_fields(&mut views, &mut reg);
        let (second, _) = scan.reborrow().unpark_first().expect("보관한 항목");
        assert_eq!(second, p1);
        reg.attach_window(WindowId::from(3u64), second);
        assert!(
            EngineScanMut::from_fields(&mut views, &mut reg)
                .unpark_first()
                .is_none()
        );
    }

    #[test]
    fn parked_navigation_survives_application_mutations_and_unpark() {
        use crate::app::command::DomainIntent;
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .first_pane()
            .unwrap()
            .id;
        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        for _ in 0..2 {
            crate::app::structural_exec::execute(
                &mut core,
                &mut state,
                &mut engine,
                DomainIntent::CreateTab {
                    pane_id,
                    cwd: None,
                    kind: "empty".into(),
                    name: None,
                    surface_params: serde_json::json!({}),
                    activate: false,
                },
            )
            .unwrap();
        }
        let pane = engine.find_pane_by_id(pane_id).unwrap();
        let selected = pane.tabs[2].id;
        let removed = pane.tabs[1].id;
        state.navigation.select_tab(pane, selected);
        let mut reg = EngineRegistry::default();
        let id = reg.park_for_test(state, engine_session);
        let mut views = HashMap::new();
        {
            let scan = EngineScanMut::from_fields(&mut views, &mut reg);
            let (state, mut engine) = scan.parked_session(id).unwrap();
            let tab_id = engine.find_pane_by_id(pane_id).unwrap().tabs[0].id;
            crate::app::structural_exec::execute(
                &mut core,
                state,
                &mut engine,
                DomainIntent::MoveTab {
                    pane_id,
                    tab_id,
                    to_index: 2,
                },
            )
            .unwrap();
            crate::app::structural_exec::execute(
                &mut core,
                state,
                &mut engine,
                DomainIntent::CloseTab { tab_id: removed },
            )
            .unwrap();
            assert_eq!(
                state
                    .navigation
                    .tab_id(engine.find_pane_by_id(pane_id).unwrap()),
                Some(selected)
            );
        }
        let (unparked, state) = EngineScanMut::from_fields(&mut views, &mut reg)
            .unpark_first()
            .unwrap();
        assert_eq!(unparked, id);
        let engine = reg.get(id).unwrap();
        assert_eq!(
            state
                .navigation
                .tab_id(engine.find_pane_by_id(pane_id).unwrap()),
            Some(selected)
        );
    }

    #[test]
    fn parked_session_lookups_find_the_engine_by_id() {
        let (mut reg, ids) = parked(&["p0", "p1"]);
        // 첫 항목과 겹치지 않는 ID를 두 번째 항목에만 둔다.
        let target = engine_mut(&mut reg, ids[0])
            .workspace_at(0)
            .expect("workspace index is valid")
            .id
            + 5_000;
        engine_mut(&mut reg, ids[1])
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .id = target;
        let mut other = EngineRegistry::default();
        let unknown = with_pending(&mut other, engine_with_workspace_name("x"));
        let mut views = HashMap::new();
        let mut scan = EngineScanMut::from_fields(&mut views, &mut reg);
        let (_, e) = scan
            .reborrow()
            .parked_session(ids[1])
            .expect("두 번째 항목");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p1"
        );
        let (_, e) = scan.reborrow().first_parked_session().expect("첫 항목");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p0"
        );
        assert!(scan.reborrow().parked_session(unknown).is_none());
        let rid = crate::core::request_target::ResourceId {
            kind: crate::core::request_target::Kind::Workspace,
            id: u64::from(target),
        };
        let (_, e) = scan
            .parked_session_with_resource(rid)
            .expect("workspace를 가진 parked 항목");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p1"
        );
    }

    /// 보관 순서가 바뀌어도 id는 처음 가리킨 engine을 계속 가리킨다.
    #[test]
    fn engine_id_survives_park_and_unpark_of_other_engines() {
        let (mut reg, ids) = parked(&["p0", "p1"]);
        let mut views = HashMap::new();
        let (p0, view_restore) = EngineScanMut::from_fields(&mut views, &mut reg)
            .unpark_first()
            .expect("가장 먼저 보관한 항목");
        assert_eq!(p0, ids[0]);
        let (_, e) = EngineScanMut::from_fields(&mut views, &mut reg)
            .parked_session(ids[1])
            .expect("앞 항목을 꺼낸 뒤에도 같은 id로 찾는다");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p1"
        );

        let w = WindowId::from(1u64);
        reg.attach_window(w, p0);
        let mut scan = EngineScanMut::from_fields(&mut views, &mut reg);
        scan.reborrow().park(w, view_restore);
        let (_, e) = scan
            .reborrow()
            .parked_session(p0)
            .expect("다시 보관한 engine은 원래 id를 유지한다");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p0"
        );
        let (_, e) = scan
            .reborrow()
            .parked_session(ids[1])
            .expect("뒤에 보관한 항목이 있어도 같은 engine");
        assert_eq!(
            e.workspace_at(0).expect("workspace index is valid").name,
            "p1"
        );
    }

    fn slots(v: &[LayoutSlotId]) -> HashSet<LayoutSlotId> {
        v.iter().copied().collect()
    }

    #[test]
    fn picks_lowest_free_slot() {
        assert_eq!(pick_free_slot(&[1, 2, 3], &slots(&[1])), 2);
    }

    #[test]
    fn allocates_new_slot_when_all_occupied() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2])), 3);
    }

    #[test]
    fn starts_at_one_when_nothing_exists() {
        assert_eq!(pick_free_slot(&[], &slots(&[])), 1);
    }

    #[test]
    fn prefers_existing_slot_file_over_lower_unused_number() {
        assert_eq!(pick_free_slot(&[2, 3], &slots(&[])), 2);
    }

    #[test]
    fn new_slot_exceeds_both_files_and_occupancy() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2, 5])), 6);
    }
}
