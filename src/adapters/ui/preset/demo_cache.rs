//! 프리셋별 미리보기 캐시. 미리보기와 설정 화면이 같은 인스턴스를 사용한다.
//! 현재·직전 pass에서 쓴 캐시만 남기며 저장소 비교 기준인 LayoutBase도 함께 보관한다.

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

/// 직전에 편집 모드로 그렸는지 프리셋별로 기록한다. 캐시를 다시 읽어도 이 기록은 유지한다.
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

/// 프리셋별 캐시 ID. 설정 화면과 미리보기가 공유하되 다른 프리셋과는 구분한다.
pub(super) fn demo_cache_id(key: &str) -> egui::Id {
    egui::Id::new("preset_demo_layout_cache").with(key)
}

/// 사용 중인 캐시 목록의 ID.
fn demo_slots_id() -> egui::Id {
    egui::Id::new("preset_demo_layout_slots")
}

/// 현재·직전 pass에서 사용한 캐시 목록.
#[derive(Clone, Default)]
struct DemoSlots {
    /// `now` 가 기록된 pass(`egui::Context::cumulative_pass_nr`).
    pass: u64,
    now: Vec<String>,
    prev: Vec<String>,
}

/// 이번 pass 사용을 기록하고 직전 pass에도 쓰지 않은 캐시·모드 기록을 지운다.
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

/// 레이아웃과 마지막으로 읽거나 저장한 저장소 기준값.
#[derive(Clone)]
pub(super) struct DemoCache {
    pub(super) key: String,
    pub(super) layout: DemoLayout,
    /// 저장 직전 경합 판정의 기준([`super::persist_layout`]).
    pub(super) base: Option<LayoutBase>,
}

/// 저장소에서 캐시를 만든다. 프리셋이 없으면 None이다.
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

/// 저장 충돌이면 편집본 대신 저장소 값을 읽고 알린다. 같은 ID가 다른 leaf를 가리킬 수 있어 선택도 푼다.
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

/// 보기 모드에서 저장소 레이아웃이 바뀌면 캐시를 갱신한다. 편집 중에는 호출하지 않는다.
/// 값이 같으면 미리보기 탭 선택을 유지하며, 프리셋이 사라졌으면 캐시를 그대로 둔다.
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
