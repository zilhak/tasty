//! 플러그인 egui 입력칸의 복사 결과를 창으로 보낸다. pump 뒤 같은 tick에서 popup·surface 결과를
//! 한 번에 꺼내고, 각 결과를 소유를 확인한 창의 공통 복사 함수(쓰기와 토스트)로 넘긴다. 편집 요청의
//! 회신에 실린 복사는 이 경로가 아니라 보류를 푸는 곳(`plugin_edit_holds`)이 쓴다.

use std::collections::HashMap;

use super::App;
use crate::plugin::manager::{PluginCopiedText, PluginCopyTarget};

/// 복사 결과를 어느 창에도 보내지 않은 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupCopyDrop {
    /// 그 instance가 이제 열려 있지 않다.
    Closed,
    /// 결과를 보낸 플러그인이 그 팝업의 주인이 아니다.
    NotOwner,
    /// 어느 창도 그 팝업에 복사·잘라내기를 보내지 않았다.
    NoEditingWindow,
}

/// 팝업 복사 결과를 받을 창. `owner`는 그 instance를 연 플러그인이고, `sent`는 창마다 그 팝업에
/// 복사·잘라내기를 보낸 순번이다. 가장 최근에 보낸 창 하나만 고르므로 여러 창에 보이는 팝업도
/// 결과가 한 번만 쓰인다.
fn popup_copy_window<W>(
    plugin_id: &str,
    owner: Option<&str>,
    sent: impl IntoIterator<Item = (W, Option<u64>)>,
) -> Result<W, PopupCopyDrop> {
    match owner {
        None => return Err(PopupCopyDrop::Closed),
        Some(owner) if owner != plugin_id => return Err(PopupCopyDrop::NotOwner),
        Some(_) => {}
    }
    sent.into_iter()
        .filter_map(|(window, seq)| seq.map(|seq| (seq, window)))
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, window)| window)
        .ok_or(PopupCopyDrop::NoEditingWindow)
}

/// surface 복사 결과를 어느 창에도 보내지 않은 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceCopyDrop {
    /// 그 surface 를 그리는 창이 없다(닫혔거나 아직 그리지 않았다).
    NotShown,
    /// 그 surface 를 그리는 창의 기록으로는 결과를 보낸 플러그인이 그 surface 의 주인이 아니다.
    NotOwner,
    /// 그린 기록은 그 플러그인이지만 지금 엔진에 그 surface 가 없거나 그 mesh 의 주인이 다르다. 전송
    /// 기록은 다음 렌더에서야 정리되므로 그 사이에 닫힌 surface 의 늦은 결과다.
    Gone,
}

/// 창 하나가 본 surface 의 주인. `drawn`은 그 창이 직전 렌더에 그린 플러그인(전송 기록)이고, `live`는 그 창의
/// 엔진에 지금 있는 그 surface 의 mesh 주인이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceOwners<'a> {
    drawn: Option<&'a str>,
    live: Option<&'a str>,
}

/// 창마다 그 surface 의 두 주인을 모은다. `windows`는 창, 그 창이 직전 렌더에 그린 플러그인, 그 창의 엔진이고,
/// `live_owner`는 엔진에서 surface id 로 지금 그 surface mesh 의 주인을 찾는다. 닫힌 surface 는 엔진 조회가 없어
/// 그린 기록이 남아 있어도 `live`가 없다.
fn surface_owners<'a, W, E>(
    surface: u32,
    windows: impl IntoIterator<Item = (W, Option<&'a str>, E)>,
    live_owner: impl Fn(E, u32) -> Option<&'a str>,
) -> Vec<(W, SurfaceOwners<'a>)> {
    windows
        .into_iter()
        .map(|(window, drawn, engine)| {
            let live = live_owner(engine, surface);
            (window, SurfaceOwners { drawn, live })
        })
        .collect()
}

/// surface 복사 결과를 받을 창. 결과를 보낸 플러그인이 그렸고 지금도 그 surface 의 mesh 를 소유하는 첫 창
/// 하나만 고르므로 결과가 한 번만 쓰인다.
fn surface_copy_window<'a, W>(
    plugin_id: &str,
    shown: impl IntoIterator<Item = (W, SurfaceOwners<'a>)>,
) -> Result<W, SurfaceCopyDrop> {
    let mut drop = SurfaceCopyDrop::NotShown;
    for (window, owners) in shown {
        match owners.drawn {
            Some(drawn) if drawn == plugin_id => {
                if owners.live == Some(plugin_id) {
                    return Ok(window);
                }
                drop = SurfaceCopyDrop::Gone;
            }
            Some(_) if drop == SurfaceCopyDrop::NotShown => drop = SurfaceCopyDrop::NotOwner,
            _ => {}
        }
    }
    Err(drop)
}

impl App {
    /// pump가 받은 플러그인 복사 결과를 모두 꺼내 처리한다.
    pub(super) fn deliver_plugin_copied_texts(&mut self) {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        let copied = mgr.take_copied_texts();
        if copied.is_empty() {
            return;
        }
        let owners: HashMap<u64, String> = mgr
            .popup_instances()
            .map(|(id, inst)| (id, inst.plugin_id.clone()))
            .collect();
        for PluginCopiedText {
            plugin_id,
            target,
            text,
        } in copied
        {
            match target {
                PluginCopyTarget::Popup(instance) => {
                    self.deliver_popup_copy(&plugin_id, instance, &text, &owners);
                }
                PluginCopyTarget::Surface(surface) => {
                    self.deliver_surface_copy(&plugin_id, surface, &text);
                }
            }
        }
    }

    fn deliver_surface_copy(&mut self, plugin_id: &str, surface: u32, text: &str) {
        // 창 고르기는 엔진 조회까지 호출식 하나다. 중간 값을 두면 판정 앞에서 바꿔칠 자리가 생긴다.
        let engines = &self.engines;
        match surface_copy_window(
            plugin_id,
            surface_owners(
                surface,
                self.view.views.iter().map(|(id, view)| {
                    let drawn = view
                        .as_main()
                        .and_then(|main| main.egui_mesh_surface_plugin(surface));
                    (*id, drawn, engines.window_read(*id))
                }),
                |engine, surface| {
                    engine?
                        .find_surface_by_id(surface)?
                        .mesh()
                        .map(|mesh| mesh.plugin_id.as_str())
                },
            ),
        ) {
            Ok(id) => {
                if let Some(main) = self.view.views.get_mut(&id).and_then(|v| v.as_main_mut()) {
                    main.copy_text(
                        text,
                        false,
                        "toast.copied",
                        Vec::new(),
                        crate::adapters::ui::ToastScope::Surface(surface),
                    );
                }
            }
            Err(reason) => tracing::warn!(
                "plugin '{plugin_id}' copy for surface {surface} dropped: {reason:?}"
            ),
        }
    }

    fn deliver_popup_copy(
        &mut self,
        plugin_id: &str,
        instance: u64,
        text: &str,
        owners: &HashMap<u64, String>,
    ) {
        let sent = self.view.views.iter().filter_map(|(id, view)| {
            let main = view.as_main()?;
            Some((
                *id,
                main.state.plugin_popup_edit_sent.get(&instance).copied(),
            ))
        });
        let window = popup_copy_window(plugin_id, owners.get(&instance).map(String::as_str), sent);
        match window {
            Ok(id) => {
                if let Some(main) = self.view.views.get_mut(&id).and_then(|v| v.as_main_mut()) {
                    main.copy_plugin_popup_text(text);
                }
            }
            Err(reason) => tracing::debug!(
                "plugin '{plugin_id}' copy for popup {instance} dropped: {reason:?}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 같은 팝업이 두 창에 보여도 복사·잘라내기를 가장 최근에 보낸 창 하나만 결과를 받는다.
    #[test]
    fn the_window_that_sent_the_latest_edit_gets_the_copy() {
        let sent = [("a", Some(3)), ("b", Some(7)), ("c", None)];
        assert_eq!(popup_copy_window("p", Some("p"), sent), Ok("b"));
    }

    fn owners<'a>(drawn: Option<&'a str>, live: Option<&'a str>) -> SurfaceOwners<'a> {
        SurfaceOwners { drawn, live }
    }

    /// surface 결과는 그 surface 를 그린 플러그인이 보낸 것만 그 창이 받는다. 그리는 창이 없거나 다른 플러그인이
    /// 그린 surface 의 결과는 쓰지 않는다.
    #[test]
    fn a_surface_copy_goes_only_to_a_window_where_its_plugin_draws_it() {
        assert_eq!(
            surface_copy_window(
                "p",
                [
                    ("a", owners(None, None)),
                    ("b", owners(Some("p"), Some("p")))
                ]
            ),
            Ok("b")
        );
        assert_eq!(
            surface_copy_window("p", [("a", owners(None, None))]),
            Err(SurfaceCopyDrop::NotShown)
        );
        assert_eq!(
            surface_copy_window("p", [("a", owners(Some("q"), Some("q")))]),
            Err(SurfaceCopyDrop::NotOwner)
        );
    }

    /// 창마다 그린 기록과 엔진 조회를 따로 담는다. 닫힌 surface(조회 없음)와 mesh 주인이 바뀐 surface 는 그린 기록이
    /// 남아도 `live`가 그 조회 그대로이고, 살아 있는 surface 는 두 주인이 같다.
    #[test]
    fn owners_take_the_live_owner_from_the_engine_lookup() {
        let engines = [
            ("closed", None),
            ("swapped", Some("q")),
            ("alive", Some("p")),
        ];
        let lookup = |engine: &str, surface: u32| {
            assert_eq!(surface, 9);
            engines
                .iter()
                .find(|(name, _)| *name == engine)
                .and_then(|(_, owner)| *owner)
        };
        let shown = surface_owners(
            9,
            [
                ("a", Some("p"), "closed"),
                ("b", Some("p"), "swapped"),
                ("c", Some("p"), "alive"),
            ],
            lookup,
        );
        assert_eq!(
            shown,
            [
                ("a", owners(Some("p"), None)),
                ("b", owners(Some("p"), Some("q"))),
                ("c", owners(Some("p"), Some("p"))),
            ]
        );
        assert_eq!(surface_copy_window("p", shown), Ok("c"));
        assert_eq!(
            surface_copy_window("p", surface_owners(9, [("a", Some("p"), "closed")], lookup)),
            Err(SurfaceCopyDrop::Gone)
        );
    }

    /// 닫힌 surface 는 다음 렌더까지 전송 기록에 그 플러그인이 남는다. 그 사이에 온 늦은 결과는 지금 엔진에 그
    /// surface 가 없거나 mesh 주인이 다르면 쓰지 않는다.
    #[test]
    fn a_late_copy_for_a_closed_surface_is_dropped() {
        assert_eq!(
            surface_copy_window("p", [("a", owners(Some("p"), None))]),
            Err(SurfaceCopyDrop::Gone)
        );
        assert_eq!(
            surface_copy_window("p", [("a", owners(Some("p"), Some("q")))]),
            Err(SurfaceCopyDrop::Gone)
        );
    }

    /// 닫힌 팝업, 다른 플러그인의 결과, 편집을 보낸 창이 없는 결과는 어느 창도 받지 않는다.
    #[test]
    fn closed_foreign_or_unrequested_copies_are_dropped() {
        let sent = || [("a", Some(1))];
        assert_eq!(
            popup_copy_window("p", None, sent()),
            Err(PopupCopyDrop::Closed)
        );
        assert_eq!(
            popup_copy_window("p", Some("q"), sent()),
            Err(PopupCopyDrop::NotOwner)
        );
        assert_eq!(
            popup_copy_window("p", Some("p"), [("a", None)]),
            Err(PopupCopyDrop::NoEditingWindow)
        );
    }

    /// 창 고르기 앞뒤의 연결은 `MainView` 없이 시험할 수 없어 소스로 고정한다. surface 결과는 그 창이 그린
    /// 플러그인 기록으로 고르고, 그 surface 범위 토스트로 쓴다.
    #[test]
    fn surface_copies_reach_the_copy_door_of_the_drawing_window() {
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        let file = include_str!("plugin_copied_texts.rs");
        let (code, _) = file
            .split_once("#[cfg(test)]")
            .expect("시험 모듈 앞의 코드");
        let src = squash(code);
        for needle in [
            "PluginCopyTarget::Surface(surface) => { self.deliver_surface_copy(&plugin_id, surface, &text); }",
            "let engines = &self.engines; match surface_copy_window( plugin_id, surface_owners( surface, self.view.views.iter().map(|(id, view)| { let drawn = view .as_main() .and_then(|main| main.egui_mesh_surface_plugin(surface)); (*id, drawn, engines.window_read(*id)) }), |engine, surface| { engine? .find_surface_by_id(surface)? .mesh() .map(|mesh| mesh.plugin_id.as_str()) }, ), ) { Ok(id) => {",
            "crate::adapters::ui::ToastScope::Surface(surface),",
        ] {
            assert!(src.contains(&squash(needle)), "연결이 바뀌었다: {needle}");
        }
    }
}
