//! 플러그인 egui-mesh surface 가 키를 받는 동안의 편집 키(복사·잘라내기·붙여넣기) 판정. 팝업과 같이
//! egui-winit 과 단축키 디스패치보다 먼저 한 번 한다. 회신을 약속한 플러그인의 surface 만 판정하고, 그 밖의
//! surface 는 기존 단축키 경로(`run_copy`·`run_paste`)가 받는다. 원격 mirror surface 의 편집 키는 원격 결과를
//! 받을 길이 없어 넘긴다. 계약: `docs/design/policies/key-mapping.md`의 "플러그인 입력칸" 절.

use winit::event::{ElementState, WindowEvent};

use super::MainView;
use super::plugin_popup_keys::{PluginEditAsk, popup_capability, popup_key};
use crate::adapters::ui::input::edit_key::{Capability, EditBindings, EditKey, verdict};
use crate::gfx::gpu::edit_keys::EguiKey;
use crate::plugin::manager::PluginCopyTarget;
use crate::runtime::engine_read::EngineRead;

/// 키를 받는 surface 중 이 판정이 맡는 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SurfaceEditTarget {
    /// 회신을 약속한 플러그인의 로컬 egui-mesh surface.
    Local { surface: u32, plugin_id: String },
    /// 원격 egui-mesh mirror. 편집 이벤트를 보내지 않고 원래 키를 원격 플러그인에 넘긴다.
    Mirror,
}

/// 키를 받는 surface 의 편집 판정 대상. 키가 호스트 egui(팝업·모달·무대)로 가거나, 포커스 surface 가
/// egui-mesh 가 아니거나, 그 플러그인이 회신을 약속하지 않았으면(직전 렌더 기준) 없다.
pub(super) fn surface_edit_target(
    overlay: bool,
    local: Option<(u32, &str, bool)>,
    mirror: bool,
) -> Option<SurfaceEditTarget> {
    if overlay {
        return None;
    }
    if let Some((surface, plugin_id, reports)) = local {
        return reports.then(|| SurfaceEditTarget::Local {
            surface,
            plugin_id: plugin_id.to_string(),
        });
    }
    mirror.then_some(SurfaceEditTarget::Mirror)
}

/// mirror 의 편집 바인딩은 넘긴다. 원격 결과를 받을 길이 없어 편집 이벤트를 보내지 않고, 원래 키가 원격
/// 플러그인에 `Key`로 간다. 바인딩이 아니면 판정하지 않는다.
fn mirror_key(binding: Option<EditKey>) -> EguiKey {
    popup_key(verdict(binding, |_| Capability::No)).0
}

/// 팔레트 편집 명령이 포커스 surface 에 하는 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PaletteSurfaceEdit {
    /// 이 판정의 대상이 아니다. 기존 팔레트 경로(`run_copy`·탐색기·`run_paste`)가 받는다.
    NotMine,
    /// 대상이지만 보낼 것이 없다(빈 클립보드의 붙여넣기). 터미널 붙여넣기로 넘기지 않는다.
    Nothing,
    /// 이 surface 의 플러그인에 편집 요청을 보낸다.
    Ask { surface: u32, plugin_id: String },
}

/// 팔레트 편집 명령의 대상. 회신을 약속한 플러그인의 로컬 surface 만 맡는다. mirror 와 약속하지 않은 플러그인은
/// 기존 경로로 간다. 키가 없으므로 할 수 없음을 알면 보내지 않고 끝낸다.
pub(super) fn palette_surface_edit(
    target: Option<SurfaceEditTarget>,
    edit: EditKey,
    clipboard_text: bool,
) -> PaletteSurfaceEdit {
    let Some(SurfaceEditTarget::Local { surface, plugin_id }) = target else {
        return PaletteSurfaceEdit::NotMine;
    };
    match popup_capability(edit, clipboard_text) {
        Capability::No => PaletteSurfaceEdit::Nothing,
        _ => PaletteSurfaceEdit::Ask { surface, plugin_id },
    }
}

impl MainView {
    /// 팔레트의 복사·잘라내기·붙여넣기를 회신을 약속한 플러그인 surface 에 요청으로 보낸다. 맡았으면 true 이고
    /// 기존 팔레트 경로를 타지 않는다. 키가 없으므로 원래 키 없이 남기고, App 이 회신의 복사 글자만 쓴다.
    pub(crate) fn ask_plugin_surface_palette_edit(
        &mut self,
        engine: &EngineRead<'_>,
        edit: EditKey,
    ) -> bool {
        let target = self.surface_edit_target_now(engine);
        let text = match (&target, edit) {
            (Some(SurfaceEditTarget::Local { .. }), EditKey::Paste) => {
                self.base.gpu.plugin_paste_text()
            }
            _ => None,
        };
        match palette_surface_edit(target, edit, text.is_some()) {
            PaletteSurfaceEdit::NotMine => false,
            PaletteSurfaceEdit::Nothing => true,
            PaletteSurfaceEdit::Ask { surface, plugin_id } => {
                self.plugin_edit_ask = Some(PluginEditAsk {
                    target: PluginCopyTarget::Surface(surface),
                    plugin_id: Some(plugin_id),
                    edit,
                    text,
                    original: None,
                });
                true
            }
        }
    }

    /// 창의 키를 지금 받는 로컬 egui-mesh surface. 호스트 egui 가 키를 받는 동안(팝업·모달·무대)은 없다.
    /// 보류의 대상이 아직 키를 받는지 App 이 이것으로 확인한다.
    pub(crate) fn plugin_surface_key_target(&self, engine: &EngineRead<'_>) -> Option<u32> {
        if self.host_egui_has_keys() {
            return None;
        }
        self.focused_egui_mesh_surface_id(engine)
    }

    /// 호스트 egui(팝업·모달·무대)가 키를 받는지. 무대는 키보드 오버레이 술어에 없어 함께 본다.
    fn host_egui_has_keys(&self) -> bool {
        self.state.keyboard_overlay_open() || self.state.fullscreen_stage_active()
    }

    fn surface_edit_target_now(&self, engine: &EngineRead<'_>) -> Option<SurfaceEditTarget> {
        let overlay = self.host_egui_has_keys();
        let local = self.focused_egui_mesh_surface_id(engine).and_then(|sid| {
            let plugin_id = engine.find_surface_by_id(sid)?.mesh()?.plugin_id.clone();
            let reports = self
                .egui_mesh
                .get(&sid)
                .is_some_and(|st| st.reports_edit_outcomes);
            Some((sid, plugin_id, reports))
        });
        let mirror = self.focused_attach_mesh_surface_id(engine).is_some();
        let local = local
            .as_ref()
            .map(|(sid, plugin_id, reports)| (*sid, plugin_id.as_str(), *reports));
        surface_edit_target(overlay, local, mirror)
    }

    /// 키를 받는 surface 가 이 판정의 대상일 때의 편집 키 판정. 다른 대상이면 `None`이고 다음 판정이 받는다.
    /// 호스트 입력칸의 보류가 있으면 그 판정이 이 키를 줄 세우도록 `None`이다.
    pub(super) fn judge_plugin_surface_edit_key(
        &mut self,
        engine: &EngineRead<'_>,
        event: &WindowEvent,
    ) -> Option<EguiKey> {
        let WindowEvent::KeyboardInput { event: key, .. } = event else {
            return None;
        };
        if self.base.gpu.edit_keys_busy() {
            return None;
        }
        let target = self.surface_edit_target_now(engine)?;
        if key.state != ElementState::Pressed {
            return Some(EguiKey::Free);
        }
        let mods = self.base.state.modifiers;
        let lookup =
            crate::shortcuts::shortcut_lookup_key(&key.logical_key, &key.physical_key, mods);
        let binding =
            EditBindings::from_settings(&engine.settings.keybindings).binding(&lookup, mods);
        let (surface, plugin_id) = match target {
            SurfaceEditTarget::Mirror => return Some(mirror_key(binding)),
            SurfaceEditTarget::Local { surface, plugin_id } => (surface, plugin_id),
        };
        let mut text = None;
        let judged = verdict(binding, |edit| {
            if edit == EditKey::Paste {
                text = self.base.gpu.plugin_paste_text();
            }
            popup_capability(edit, text.is_some())
        });
        let (key, asked) = popup_key(judged);
        if let Some(edit) = asked {
            self.plugin_edit_ask = Some(PluginEditAsk {
                target: PluginCopyTarget::Surface(surface),
                plugin_id: Some(plugin_id),
                edit,
                text,
                original: Some(event.clone()),
            });
        }
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 회신을 약속한 플러그인의 로컬 surface 만 판정한다. 약속하지 않은 플러그인(번들 image 등)의 surface 는
    /// 기존 단축키 경로로 가고, mirror 는 넘기는 대상이다. 호스트 egui 가 키를 받는 동안은 판정하지 않는다.
    #[test]
    fn only_surfaces_of_plugins_that_reply_or_mirrors_are_judged() {
        assert_eq!(
            surface_edit_target(false, Some((4, "demo", true)), false),
            Some(SurfaceEditTarget::Local {
                surface: 4,
                plugin_id: "demo".into()
            })
        );
        assert_eq!(
            surface_edit_target(false, Some((4, "image", false)), false),
            None
        );
        assert_eq!(
            surface_edit_target(false, None, true),
            Some(SurfaceEditTarget::Mirror)
        );
        assert_eq!(surface_edit_target(true, None, true), None);
        assert_eq!(surface_edit_target(false, None, false), None);
    }

    /// mirror 의 편집 바인딩은 넘긴다(원래 키가 원격 플러그인에 `Key`로 간다). 바인딩이 아니면 판정하지 않는다.
    #[test]
    fn a_mirror_passes_its_edit_keys() {
        assert_eq!(
            mirror_key(Some(EditKey::Paste)),
            EguiKey::Pass(EditKey::Paste)
        );
        assert_eq!(
            mirror_key(Some(EditKey::Copy)),
            EguiKey::Pass(EditKey::Copy)
        );
        assert_eq!(mirror_key(Some(EditKey::Cut)), EguiKey::Pass(EditKey::Cut));
        assert_eq!(mirror_key(None), EguiKey::Free);
    }

    /// 팔레트 편집은 회신을 약속한 로컬 surface 만 요청으로 보낸다. 빈 클립보드의 붙여넣기는 보내지 않고 끝내며
    /// (터미널로 넘기지 않는다) mirror·약속 없는 플러그인·대상 없음은 기존 경로로 간다.
    #[test]
    fn a_palette_edit_asks_only_a_surface_that_replies() {
        let local = || {
            Some(SurfaceEditTarget::Local {
                surface: 4,
                plugin_id: "demo".into(),
            })
        };
        let ask = PaletteSurfaceEdit::Ask {
            surface: 4,
            plugin_id: "demo".into(),
        };
        assert_eq!(palette_surface_edit(local(), EditKey::Copy, false), ask);
        assert_eq!(palette_surface_edit(local(), EditKey::Cut, false), ask);
        assert_eq!(palette_surface_edit(local(), EditKey::Paste, true), ask);
        assert_eq!(
            palette_surface_edit(local(), EditKey::Paste, false),
            PaletteSurfaceEdit::Nothing
        );
        assert_eq!(
            palette_surface_edit(Some(SurfaceEditTarget::Mirror), EditKey::Paste, true),
            PaletteSurfaceEdit::NotMine
        );
        assert_eq!(
            palette_surface_edit(None, EditKey::Copy, false),
            PaletteSurfaceEdit::NotMine
        );
    }

    /// 연결은 `MainView` 없이 시험할 수 없어 소스로 고정한다. 판정 입구는 재생 표시와 팝업 판정 다음, host 입력칸
    /// 판정 앞에서 이 판정을 부르고, 렌더는 surface 마다 그 플러그인의 회신 약속을 기록한다.
    #[test]
    fn the_gate_and_the_render_feed_this_judgment() {
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        let gate = squash(include_str!("edit_keys.rs"));
        let popup = gate
            .find(&squash("self.judge_plugin_popup_edit_key(engine, event)"))
            .expect("팝업 판정");
        let surface = gate
            .find(&squash(
                "if let Some(judged) = self.judge_plugin_surface_edit_key(engine, event) { return judged; }",
            ))
            .expect("surface 판정이 판정 입구에 없다");
        let host = gate
            .find(&squash("let entry = edit_key_entry("))
            .expect("host 판정");
        assert!(popup < surface && surface < host, "판정 순서가 바뀌었다");
        let render = squash(include_str!("egui_mesh.rs"));
        assert!(
            render.contains(&squash(
                "st.reports_edit_outcomes = mgr.supports_edit_outcome(&plugin_id);"
            )),
            "렌더가 회신 약속을 기록하지 않는다"
        );
    }

    /// 팔레트 연결도 소스로 고정한다. 팔레트 편집 명령은 기존 경로보다 먼저 이 판정을 묻고, App 은 팔레트가
    /// 실행되는 렌더 뒤에 편집 요청을 꺼낸다.
    #[test]
    fn the_palette_asks_this_judgment_first_and_the_frame_takes_it() {
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        let dispatch = squash(include_str!(
            "../../adapters/ui/input/shortcuts/dispatch.rs"
        ));
        let ask = dispatch
            .find(&squash(
                "let deferred = deferred.filter(|action| { !action .edit() .is_some_and(|edit| self.ask_plugin_surface_palette_edit(engine, edit)) });",
            ))
            .expect("팔레트가 surface 판정을 묻지 않는다");
        let routes = dispatch
            .find(&squash("match deferred {"))
            .expect("팔레트 기존 경로");
        assert!(ask < routes, "기존 경로가 먼저다");
        let frame: String = include_str!("../../app/view_frame.rs")
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//"))
            .collect();
        let frame = squash(&frame);
        let draw = frame
            .find(&squash("view.finish_redraw("))
            .expect("렌더 마무리");
        let take = frame
            .find(&squash(
                "self.process_remote_tool_requests(id); self.begin_plugin_field_edit(id); self.poll_port_scans();",
            ))
            .expect("프레임이 편집 요청을 꺼내지 않는다");
        assert!(draw < take, "팔레트 실행 전에 꺼낸다");
    }
}
