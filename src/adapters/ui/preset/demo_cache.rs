//! 미리보기 layout 캐시 — preset 마다 한 칸(egui temp memory).
//!
//! 미리보기와 설정 화면은 같은 preset 이면 같은 칸을 읽고 쓴다. 칸은 직전에 그린 pass 와
//! 지금 pass 에 쓰인 preset 의 것만 남고([`touch_slot`]), 각 칸은 자신이 지어진 저장소
//! 판([`LayoutBase`])을 함께 들고 있어 보기 모드 새로고침([`refresh_view_cache`])과 저장
//! 직전 경합 판정([`super::persist_layout`])의 기준이 된다.

use tasty_presets::{PresetKind, PresetStore};

use crate::adapters::ui::{ToastKind, ToastManager, ToastScope};
use crate::i18n::t;

use super::demo_layout::{DemoLayout, KindCatalog};
use super::layout_base::LayoutBase;

/// 선택된 preset 으로부터 미리보기 위젯을 만든다. `catalog` 는 registry 파생 kind
/// 스냅샷(미주입이면 빈 catalog → 정적 fallback).
fn build_demo(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
) -> Option<DemoLayout> {
    match kind {
        PresetKind::Workspace => store
            .get_workspace(name)
            .map(|p| DemoLayout::from_workspace(p, catalog)),
        PresetKind::Tab => store
            .get_tab(name)
            .map(|p| DemoLayout::from_tab(p, catalog)),
        PresetKind::Pane => store
            .get_pane(name)
            .map(|p| DemoLayout::from_pane(p, catalog)),
    }
}

/// 그 preset 을 직전에 그린 [`super::draw_preview`] 의 모드(편집이면 `true`) 의 temp memory id.
/// 캐시 칸과 따로 둔다 — 설정 화면·경합 재적재가 캐시를 새로 지어도 이 값이 흔들리지 않게.
/// 캐시 칸처럼 preset 마다 따로다 — 한 프레임에 다른 preset 의 보기가 먼저 그려져도 이
/// preset 의 편집이 전이 프레임으로 오인되지 않게.
pub(super) fn drew_editing_id(key: &str) -> egui::Id {
    egui::Id::new("preset_demo_drew_editing").with(key)
}

/// 그 preset 을 직전에 편집 모드로 그렸는가. 기록이 없으면 보기로 본다.
pub(super) fn drew_editing_last(ui: &egui::Ui, key: &str) -> bool {
    ui.data(|d| d.get_temp(drew_editing_id(key)))
        .unwrap_or(false)
}

pub(super) fn set_drew_editing_last(ui: &egui::Ui, key: &str, editing: bool) {
    ui.data_mut(|d| d.insert_temp(drew_editing_id(key), editing));
}

/// 미리보기 캐시 키 — `{kind}:{name}`. 설정 화면의 draft 도 같은 키로 자기 preset 을 적는다.
pub(super) fn preset_key(kind: PresetKind, name: &str) -> String {
    format!("{}:{}", kind.as_str(), name)
}

/// 미리보기 layout 캐시(egui temp memory) 의 id — preset 마다 한 칸. 같은 preset 이면
/// 미리보기와 설정 화면이 **같은 인스턴스**를 읽고 쓴다 — 설정 화면이 사본을 따로 지으면
/// 확인 뒤 미리보기와 어긋난다. 칸이 하나뿐이면 한 프레임에 다른 preset 을 그리는 호출이
/// 그 칸을 갈아 끼워, 편집 중인 preset 의 캐시가 경고 없이 저장소 판으로 돌아간다.
pub(super) fn demo_cache_id(key: &str) -> egui::Id {
    egui::Id::new("preset_demo_layout_cache").with(key)
}

/// 살아 있는 캐시 칸의 명부(egui temp memory) 의 id — [`DemoSlots`].
fn demo_slots_id() -> egui::Id {
    egui::Id::new("preset_demo_layout_slots")
}

/// 캐시 칸 명부. 칸은 직전에 그린 pass 와 지금 pass 에 쓰인 preset 의 것만 남는다 —
/// preset 을 둘러볼 때마다 칸이 쌓이지 않고, 다른 preset 으로 옮겼다 돌아오면 칸이 하나일
/// 때처럼 저장소에서 새로 짓는다.
#[derive(Clone, Default)]
struct DemoSlots {
    /// `now` 가 기록된 pass(`egui::Context::cumulative_pass_nr`).
    pass: u64,
    now: Vec<String>,
    prev: Vec<String>,
}

/// `key` 의 칸을 이번 pass 에 쓴다고 적는다. 새 pass 의 첫 기록이면 직전에 그린 pass 에도
/// 안 쓰인 칸(캐시·모드 기록)을 비운다.
fn touch_slot(ui: &egui::Ui, key: &str) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| {
        let mut slots: DemoSlots = d.get_temp(demo_slots_id()).unwrap_or_default();
        if slots.pass != pass {
            for stale in slots.prev.iter().filter(|k| !slots.now.contains(k)) {
                d.remove::<DemoCache>(demo_cache_id(stale));
                d.remove::<bool>(drew_editing_id(stale));
            }
            slots.prev = std::mem::take(&mut slots.now);
            slots.pass = pass;
        }
        if !slots.now.iter().any(|k| k == key) {
            slots.now.push(key.to_owned());
        }
        d.insert_temp(demo_slots_id(), slots);
    });
}

/// 미리보기 캐시 한 칸 — layout 과, 그것이 지어진(또는 마지막으로 저장된) 저장소 판.
#[derive(Clone)]
pub(super) struct DemoCache {
    pub(super) key: String,
    pub(super) layout: DemoLayout,
    /// 저장 직전 경합 판정의 기준([`super::persist_layout`]).
    pub(super) base: Option<LayoutBase>,
}

/// store 에서 캐시 한 칸을 새로 짓는다. preset 이 없으면 `None`.
pub(super) fn build_cache(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
) -> Option<DemoCache> {
    Some(DemoCache {
        key: preset_key(kind, name),
        layout: build_demo(store, kind, name, catalog)?,
        base: LayoutBase::current(store, kind, name),
    })
}

/// 캐시된 layout 을 꺼낸다. 키가 다르거나 없으면 store 에서 새로 짓는다.
pub(super) fn load_demo(
    ui: &egui::Ui,
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
) -> Option<DemoCache> {
    let key = preset_key(kind, name);
    touch_slot(ui, &key);
    let cached: Option<DemoCache> = ui.data(|d| d.get_temp(demo_cache_id(&key)));
    match cached {
        Some(c) if c.key == key => Some(c),
        _ => build_cache(store, kind, name, catalog),
    }
}

pub(super) fn store_demo(ui: &egui::Ui, cache: DemoCache) {
    ui.data_mut(|d| d.insert_temp(demo_cache_id(&cache.key), cache));
}

/// 저장이 [`super::Persisted::Conflict`] 로 끝났을 때: 캐시를 저장소 판으로 다시 짓고 알린다.
/// 이번 변경은 버린다 — 저장소의 쓰기가 남는다. leaf id 는 새 트리에서 다른 leaf 를
/// 가리킬 수 있으므로 선택도 푼다.
pub(super) fn reload_after_conflict(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
    cache: &mut DemoCache,
    selected_node: &mut Option<usize>,
    toasts: &mut ToastManager,
) {
    tracing::warn!("preset '{name}' changed behind the editor; reloaded instead of overwriting");
    if let Some(fresh) = build_cache(store, kind, name, catalog) {
        *cache = fresh;
    }
    *selected_node = None;
    toasts.push(
        t("preset.toast.changed_elsewhere"),
        ToastKind::Warning,
        ToastScope::Window,
    );
}

/// 보기 모드 캐시를 저장소에 맞춘다. 캐시가 지어진(또는 마지막으로 저장된) 뒤 저장소의
/// 레이아웃이 바뀌었으면(에이전트의 `preset.save` 등) 저장소 판으로 다시 짓고 `true`.
/// 보기 모드는 사용자가 고치는 중인 것이 없으므로 버릴 것이 없다 — 편집 모드에서는 부르지
/// 않는다. 저장소의 레이아웃이 그대로면 캐시를 건드리지 않아, 미리보기에서 누른 탭도 남는다.
/// preset 이 사라졌으면 캐시를 그대로 둔다.
pub(super) fn refresh_view_cache(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
    cache: &mut DemoCache,
) -> bool {
    if LayoutBase::current(store, kind, name) == cache.base {
        return false;
    }
    match build_cache(store, kind, name, catalog) {
        Some(fresh) => {
            *cache = fresh;
            true
        }
        None => false,
    }
}
