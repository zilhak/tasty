//! IME 허용 판정. 창의 egui 출력 입구(`GpuState::apply_platform_output`)가 다음 입력의 IME 허용 값을 이 함수로 정한다.
//! Windows 는 대상과 관계없이 IME 를 허용하므로 이 판정을 읽지 않는다.

/// IME 허용을 정하는 대상.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    /// 관리자 포커스 칸의 host 팝업(`PopupManager::has_focused`).
    HostPopup,
    /// 그 밖의 노드.
    Other,
}

/// host 팝업 대상은 egui 텍스트 위젯이 이번 패스에 IME 출력을 냈을 때만 켜고(`egui_ime_output`), 그 밖의 대상은 켠다.
pub(crate) fn ime_allowed(target: ImeTarget, egui_ime_output: bool) -> bool {
    match target {
        ImeTarget::HostPopup => egui_ime_output,
        ImeTarget::Other => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{ImeTarget, ime_allowed};

    /// 판정 표: 대상과 egui IME 출력의 모든 조합이 옛 식(`!(has_focused && !ime_widget_focused)`)과 같다.
    #[test]
    fn ime_permission_matches_the_host_focus_rule_for_every_input() {
        for has_focused in [false, true] {
            for output in [false, true] {
                let target = if has_focused {
                    ImeTarget::HostPopup
                } else {
                    ImeTarget::Other
                };
                assert_eq!(
                    ime_allowed(target, output),
                    !(has_focused && !output),
                    "has_focused={has_focused} output={output}"
                );
            }
        }
    }
}
