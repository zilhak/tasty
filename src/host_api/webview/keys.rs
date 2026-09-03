//! webview 자식 창이 받은 키를 host 단축키 계층으로 올리는 **플랫폼 공통 계약**.
//!
//! native webview 는 세 OS 모두 winit 창과 별개의 OS 자식 창/뷰다(X11 child window /
//! WKWebView subview / child HWND). 그 자식이 OS 키보드 포커스를 잡으면 winit 은
//! `WindowEvent::KeyboardInput` 을 받지 못하고, host 단축키 경로
//! (`view/main/keyboard.rs` → `shortcuts::dispatch`)가 통째로 도달 불가능해진다.
//! 이 모듈은 그 구멍을 메우는 단일 계약이다 — 세 백엔드는 자기 native 키 이벤트를
//! [`WebViewKeyEvent`] 로 정규화해 [`WebViewKeyBridge`] 에만 넣고, **우선순위 판정은
//! 여기 한 곳에서만** 한다(백엔드마다 규칙이 갈라지지 않게).
//!
//! 결정 배경·대안·재검토 조건: `docs/adr/0102-webview-key-forwarding.md`.
//!
//! # 판정은 동기, 실행은 다음 프레임
//!
//! [`WebViewKeyBridge::capture_key`] 는 백엔드 콜백 안에서 **동기적으로** "host 가
//! 이 키를 가져가는가" 를 답한다 — 백엔드는 그 답으로 페이지 전파를 그 자리에서
//! 막을지 정한다. 실제 액션 실행은 host 가 다음 프레임에 큐를 비우며 수행한다.
//! 판정이 이미 끝났으므로 페이지와 host 가 같은 키를 이중 처리하는 일은 없다.
//!
//! # 무엇을 가져가는가 (우선순위 규칙)
//!
//! [`HostShortcutPolicy`] 는 `KeybindingSettings` 에서 **전량 도출**한다. 이 파일에
//! 키 콤보 리터럴은 하나도 없다(CLAUDE.md "단축키" 정책 — 하드코딩 금지).
//! 두 축으로 걸러진다.
//!
//! 1. **modifier 를 가진 콤보만** 후보다(`ctrl` / `alt` / `option` 중 하나 이상).
//!    수식 없는 키와 `shift` 만 붙은 키는 페이지 소유로 남긴다 — 그래야 문서 안
//!    텍스트 입력(주소창·find 바)의 타이핑, IME 조합, 폼 내비게이션(Tab/Enter/
//!    화살표), 페이지의 Esc 처리가 전부 그대로 살아있다.
//! 2. **페이지 예약 액션은 제외**한다([`PAGE_RESERVED_FIELDS`]). 페이지가 자체로
//!    같은 의미를 구현하는 액션(find-in-page, 복사/잘라내기/붙여넣기/전체선택)은
//!    브라우저와 동일하게 페이지가 갖는다. `find` 는 host 쪽에도 이미 같은 취지의
//!    kind 게이트가 있다(`adapters/ui/input/shortcuts/keybinding.rs`).
//!
//! key-up / repeat 은 애초에 큐에 오르지 않는다 — 백엔드가 **press·비repeat** 만
//! [`capture_key`](WebViewKeyBridge::capture_key) 를 호출한다. host 는 modifier 상태를
//! 이벤트에 실려온 값으로만 읽고 자기 `base.modifiers` 를 갱신하지 않으므로,
//! "modifier down 은 webview / up 은 host" 경계에서 상태가 눌린 채 남는 일이 없다.

use std::cell::RefCell;

use winit::keyboard::{Key, ModifiersState};

use crate::settings::KeybindingSettings;

/// 페이지가 소유하는(= host 로 포워딩하지 않는) 단축키 액션의 `GENERAL_BINDING_FIELDS`
/// field id. 콤보가 아니라 **액션 id** 목록이라, 사용자가 그 액션의 콤보를 바꿔도
/// 규칙이 따라간다(하드코딩된 키 문자열이 아니다).
const PAGE_RESERVED_FIELDS: &[&str] = &["find", "copy", "cut", "paste", "select_all"];

/// 백엔드가 채우는 정규화된 키 이벤트. 세 OS 의 native 키 표현
/// (GDK keyval / NSEvent charactersIgnoringModifiers / Win32 VK)을 winit 타입으로
/// 맞춰 올린다 — host 단축키 매칭이 winit `Key`/`ModifiersState` 기준이라
/// 여기서 한 번만 변환하면 이후 경로가 winit 키 경로와 완전히 같아진다.
#[derive(Debug, Clone)]
pub struct WebViewKeyEvent {
    /// 키를 받은 webview 가 붙어 있는 surface. host 는 큐를 비울 때 이 surface 의
    /// webview 가 아직 살아 있는지 확인하는 데 쓴다(콜백과 drain 사이에 surface 가
    /// 닫히는 레이스). 모델 포커스 이동은 키가 아니라 **클릭**([`WebViewKeyBridge::note_focus`])
    /// 에만 붙인다 — 이유는 `app/webview_keys.rs` 참조.
    pub surface_id: u32,
    pub key: Key,
    pub mods: ModifiersState,
}

/// host 가 가져갈 콤보의 스냅샷. 매 프레임 `KeybindingSettings` 에서 다시 만들어
/// 브리지에 밀어 넣는다(설정 변경이 즉시 반영된다).
#[derive(Debug, Clone, Default)]
pub struct HostShortcutPolicy {
    combos: Vec<String>,
}

impl HostShortcutPolicy {
    /// `KeybindingSettings` 전량에서 도출한다. 고정 액션 필드 + quick-switch 3 축
    /// (슬롯/다음/이전) + 사용자 스크립트 바인딩을 모두 포함하고, 위 두 축
    /// (modifier 보유 / 페이지 예약 제외)으로 거른다.
    pub fn from_keybindings(kb: &KeybindingSettings) -> Self {
        let mut combos: Vec<String> = Vec::new();
        let mut push = |combo: &str| {
            if crate::shortcuts::binding_has_modifier(combo) && !combos.iter().any(|c| c == combo) {
                combos.push(combo.to_string());
            }
        };

        for (field_id, _label) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            if PAGE_RESERVED_FIELDS.contains(field_id) {
                continue;
            }
            for binding in kb.get_bindings(field_id).unwrap_or(&[]) {
                push(binding);
            }
        }

        // quick-switch 3 축. 축 modifier 가 `INDIVIDUAL_SWITCH_MODIFIER` sentinel 이면
        // 슬롯/다음/이전 필드가 이미 완성된 콤보이고, 아니면 `<modifier>+<raw key>` 로
        // 합성한다 — dispatch(`shortcuts/numeric.rs`)와 같은 규칙이다.
        let axes: [(&str, Vec<&str>); 3] = [
            (
                kb.tab_switch_modifier.as_str(),
                kb.tab_switch_slot_keys
                    .iter()
                    .map(|s| s.as_str())
                    .chain([kb.tab_next_key(), kb.tab_prev_key()])
                    .collect(),
            ),
            (
                kb.workspace_switch_modifier.as_str(),
                kb.workspace_switch_slot_keys
                    .iter()
                    .map(|s| s.as_str())
                    .chain([kb.workspace_next_key(), kb.workspace_prev_key()])
                    .collect(),
            ),
            (
                kb.category_switch_modifier.as_str(),
                kb.category_switch_slot_keys
                    .iter()
                    .map(|s| s.as_str())
                    .chain([kb.category_next_key(), kb.category_prev_key()])
                    .collect(),
            ),
        ];
        for (modifier, raw_keys) in axes {
            for raw in raw_keys {
                if raw.is_empty() {
                    continue;
                }
                if modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER {
                    push(raw);
                } else {
                    push(&format!("{modifier}+{raw}"));
                }
            }
        }

        for b in &kb.script_bindings {
            push(&b.combo);
        }

        Self { combos }
    }

    /// 이 키가 host 가 가져갈 콤보에 해당하는가. 매칭은 winit 키 경로와 **같은**
    /// `matches_binding` 을 쓴다(플랫폼별 modifier 매핑 규칙도 그대로 따른다 —
    /// macOS 의 `alt`→Command / `option`→Option 포함).
    pub fn claims(&self, key: &Key, mods: ModifiersState) -> bool {
        crate::shortcuts::matches_any_binding(&self.combos, key, mods)
    }
}

/// host 와 native webview 백엔드 사이의 단일 접점. `MainView` 가 하나를 소유해
/// 그 창의 모든 webview 에 `Rc` 로 공유한다 — 백엔드는 이 타입의 메서드만 부른다.
///
/// GTK 시그널 / WKWebView 콜백 / WebView2 이벤트는 모두 winit main thread 에서
/// 발화하므로 `RefCell` 로 충분하다(기존 `pending_navigations` 와 같은 관례).
#[derive(Debug, Default)]
pub struct WebViewKeyBridge {
    policy: RefCell<HostShortcutPolicy>,
    pending: RefCell<Vec<WebViewKeyEvent>>,
    focus_requests: RefCell<Vec<u32>>,
}

impl WebViewKeyBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// host 가 매 프레임 현재 `KeybindingSettings` 스냅샷을 밀어 넣는다.
    pub fn set_policy(&self, policy: HostShortcutPolicy) {
        *self.policy.borrow_mut() = policy;
    }

    /// **백엔드 진입점.** 이 키를 host 가 가져가면 `true`(백엔드는 페이지 전파를
    /// 중단), 아니면 `false`(페이지가 평소대로 처리). `true` 인 경우에만 큐에
    /// 쌓이며, host 가 다음 프레임에 비워 실제 액션을 실행한다.
    ///
    /// 백엔드는 **key press 이고 repeat 이 아닌** 이벤트에서만 호출한다.
    pub fn capture_key(&self, surface_id: u32, key: Key, mods: ModifiersState) -> bool {
        if !self.policy.borrow().claims(&key, mods) {
            return false;
        }
        self.pending.borrow_mut().push(WebViewKeyEvent {
            surface_id,
            key,
            mods,
        });
        true
    }

    /// host 가 매 프레임 큐를 비운다(도착 순서 보존).
    pub fn take_pending(&self) -> Vec<WebViewKeyEvent> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }

    /// 백엔드가 native 클릭/포커스 획득을 관측했을 때 호출. host 는 이 값으로
    /// 모델 포커스(`focused_surface`/`focused_pane`)를 그 surface 로 옮긴다 —
    /// 클릭이 winit 에 도달하지 않아 `try_click_to_activate` 가 실행되지 않는
    /// 구조적 공백을 메운다.
    pub fn note_focus(&self, surface_id: u32) {
        let mut q = self.focus_requests.borrow_mut();
        if q.last() != Some(&surface_id) {
            q.push(surface_id);
        }
    }

    /// host 가 매 프레임 포커스 동기화 요청을 비운다.
    pub fn take_focus_requests(&self) -> Vec<u32> {
        std::mem::take(&mut *self.focus_requests.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::NamedKey;

    fn kb() -> KeybindingSettings {
        KeybindingSettings::default()
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    /// 백엔드에서 올라온 키가 host 단축키에 매칭되면 소비(`true`), 아니면 페이지로
    /// 흘림(`false`). 이 TODO 의 "완료 확인 방법" 이 요구한 단위 테스트 지점이다
    /// (OS 포커스 자체는 단위 테스트로 재현할 수 없다).
    #[test]
    fn capture_key_consumes_host_shortcut_and_passes_the_rest() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(HostShortcutPolicy::from_keybindings(&kb()));

        // split_surface_vertical 기본값(alt+d) — host 가 가져간다.
        let alt = ModifiersState::ALT;
        #[cfg(target_os = "macos")]
        let alt = ModifiersState::SUPER;
        assert!(bridge.capture_key(7, ch("d"), alt));

        // 수식 없는 문자 — 페이지 타이핑이므로 흘린다.
        assert!(!bridge.capture_key(7, ch("d"), ModifiersState::empty()));
        // shift 만 붙은 문자도 타이핑이다.
        assert!(!bridge.capture_key(7, ch("D"), ModifiersState::SHIFT));
        // 페이지가 자체 처리하는 Esc/Enter 는 수식이 없으므로 흘린다.
        assert!(!bridge.capture_key(7, Key::Named(NamedKey::Escape), ModifiersState::empty()));

        // 소비한 1건만 큐에 남는다.
        let pending = bridge.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].surface_id, 7);
        assert!(bridge.take_pending().is_empty());
    }

    /// 페이지 예약 액션(find/copy/cut/paste/select_all)의 콤보는 정책에 오르지
    /// 않는다 — 문서의 find-in-page·복사가 계속 페이지 것이어야 한다.
    #[test]
    fn page_reserved_actions_are_not_claimed() {
        let kb = kb();
        let policy = HostShortcutPolicy::from_keybindings(&kb);
        for field in PAGE_RESERVED_FIELDS {
            for binding in kb.get_bindings(field).unwrap_or(&[]) {
                assert!(
                    !policy.combos.iter().any(|c| c == binding),
                    "page-reserved binding {binding} ({field}) must not be claimed"
                );
            }
        }
        assert!(!policy.combos.is_empty(), "policy should not be empty");
    }

    /// quick-switch 축은 `<modifier>+<slot key>` 로 합성돼 정책에 오른다
    /// (workspace 전환이 "완료 확인 방법" 의 측정 대상 3종 중 하나다).
    #[test]
    fn quick_switch_axis_combos_are_claimed() {
        let kb = kb();
        let policy = HostShortcutPolicy::from_keybindings(&kb);
        let slot = kb.workspace_slot_key(1).expect("slot 1");
        let expected = format!("{}+{}", kb.workspace_switch_modifier, slot);
        assert!(
            policy.combos.contains(&expected),
            "expected {expected} in policy: {:?}",
            policy.combos
        );
    }

    /// 포커스 동기화 요청은 도착 순서대로 비워지고, 연속 중복은 접힌다.
    #[test]
    fn focus_requests_dedupe_consecutive() {
        let bridge = WebViewKeyBridge::new();
        bridge.note_focus(3);
        bridge.note_focus(3);
        bridge.note_focus(5);
        assert_eq!(bridge.take_focus_requests(), vec![3, 5]);
        assert!(bridge.take_focus_requests().is_empty());
    }
}
