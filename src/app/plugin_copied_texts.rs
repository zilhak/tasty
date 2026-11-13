//! 플러그인 egui 입력칸의 복사 결과를 창으로 보낸다. pump 뒤 같은 tick에서 popup·surface 결과를
//! 한 번에 꺼내고, 각 결과를 소유를 확인한 창의 공통 복사 함수(쓰기와 토스트)로 넘긴다.
//! egui-mesh surface의 결과는 아직 소비하지 않고 debug 로그만 남긴다.

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
                    tracing::debug!(
                        "plugin '{plugin_id}' copy for surface {surface} dropped: surface copies are not consumed"
                    );
                }
            }
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
}
