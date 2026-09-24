//! native WebView 키를 공통 정책으로 판정해 host 요청 큐에 넣는다. 실제 실행은 큐를 읽는 호출자가 맡는다.
//! modifier 없는 입력과 shift만 있는 입력은 페이지에 남긴다. plugin 조합은 페이지 예약 키와도 비교한다.
//! modifier 조합은 물리 US 배열 문자를 우선하고 물리 위치를 모르면 레이아웃 문자를 사용한다.
//! repeat 제외는 플랫폼별로 다르며 Linux의 키 press는 반복 이벤트도 전달한다.

use std::cell::RefCell;

use winit::keyboard::{Key, ModifiersState, PhysicalKey};

#[derive(Debug, Clone)]
pub struct WebViewKeyEvent {
    /// 입력을 받은 surface. 큐를 처리할 때 대상이 남아 있는지 확인하는 데 쓴다.
    pub surface_id: u32,
    /// 물리 위치 폴백을 적용한 조회 키.
    pub key: Key,
    pub mods: ModifiersState,
}

fn shortcut_lookup_key(key: Key, physical: &PhysicalKey, mods: ModifiersState) -> Key {
    if mods.control_key() || mods.super_key() || mods.alt_key() {
        tasty_key_match::physical_key_to_logical(physical).unwrap_or(key)
    } else {
        key
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShortcutSources {
    /// 페이지 예약 액션을 호출자가 미리 제외한 host 조합.
    pub host: Vec<String>,
    /// 같은 의미의 plugin 조합을 제외할 페이지 예약 목록.
    pub page_reserved: Vec<String>,
    /// 매니페스트와 사용자 설정을 합친 plugin 조합.
    pub plugin: Vec<String>,
}

/// 설정에서 만든 host 키 조합의 사본. 호출자가 변경 시 교체해야 한다.
#[derive(Debug, Clone, Default)]
pub struct HostShortcutPolicy {
    combos: Vec<String>,
}

impl HostShortcutPolicy {
    /// modifier 있는 조합만 담고 같은 문자열은 중복 제거한다. plugin만 예약 조합과 의미를 비교한다.
    pub fn new(sources: ShortcutSources) -> Self {
        let ShortcutSources {
            host,
            page_reserved,
            plugin,
        } = sources;
        let mut combos: Vec<String> = Vec::new();
        let mut push = |combo: String| {
            if tasty_key_match::binding_has_modifier(&combo) && !combos.contains(&combo) {
                combos.push(combo);
            }
        };

        for combo in host {
            push(combo);
        }

        for combo in plugin {
            // 표기 대소문자·modifier 순서가 달라도 같은 단축키이면 제외한다.
            if page_reserved
                .iter()
                .any(|r| tasty_key_match::bindings_equivalent(r, &combo))
            {
                continue;
            }
            push(combo);
        }

        Self { combos }
    }

    #[cfg(test)]
    pub(crate) fn combos(&self) -> &[String] {
        &self.combos
    }

    pub fn claims(&self, key: &Key, mods: ModifiersState) -> bool {
        tasty_key_match::matches_any_binding(&self.combos, key, mods)
    }
}

/// native 콜백에서 동기 판정·포커스 관측을 전달하는 계약.
/// 구현과 callback은 같은 GUI 스레드에서 사용하며 Send/Sync를 요구하지 않는다.
pub trait WebViewKeySink {
    /// true이면 backend가 페이지 전달을 막는다. 큐 등록과 실제 액션 성공은 별개다.
    /// key는 레이아웃 문자, physical은 위치이며 알 수 없으면 Unidentified다.
    fn capture_key(
        &self,
        surface_id: u32,
        key: Key,
        physical: PhysicalKey,
        mods: ModifiersState,
    ) -> bool;

    /// native 클릭 또는 포커스 관측을 알린다. 실제 모델 선택 변경은 host가 판단한다.
    fn note_focus(&self, surface_id: u32);
}

/// 창의 WebView들이 Rc로 공유한다. RefCell 상태는 GUI 스레드에서만 접근해야 한다.
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

    pub fn set_policy(&self, policy: HostShortcutPolicy) {
        *self.policy.borrow_mut() = policy;
    }

    pub fn take_pending(&self) -> Vec<WebViewKeyEvent> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }

    pub fn take_focus_requests(&self) -> Vec<u32> {
        std::mem::take(&mut *self.focus_requests.borrow_mut())
    }
}

impl WebViewKeySink for WebViewKeyBridge {
    /// 정책이 선택한 키만 큐에 넣는다. 실행 시점이나 최종 동작 성공을 여기서 보장하지 않는다.
    fn capture_key(
        &self,
        surface_id: u32,
        key: Key,
        physical: PhysicalKey,
        mods: ModifiersState,
    ) -> bool {
        let key = shortcut_lookup_key(key, &physical, mods);
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

    fn note_focus(&self, surface_id: u32) {
        let mut q = self.focus_requests.borrow_mut();
        if q.last() != Some(&surface_id) {
            q.push(surface_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode, NamedKey, NativeKeyCode};

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn policy(host: &[&str], page_reserved: &[&str], plugin: &[&str]) -> HostShortcutPolicy {
        HostShortcutPolicy::new(ShortcutSources {
            host: strings(host),
            page_reserved: strings(page_reserved),
            plugin: strings(plugin),
        })
    }

    fn unknown_physical() -> PhysicalKey {
        PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
    }

    fn reorder_modifiers(combo: &str) -> String {
        let parts: Vec<&str> = combo.split('+').collect();
        if parts.len() < 3 {
            return combo.to_string();
        }
        let key = parts[parts.len() - 1];
        let mut mods: Vec<&str> = parts[..parts.len() - 1].to_vec();
        mods.reverse();
        format!("{}+{}", mods.join("+"), key)
    }

    fn alt_mod() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::ALT
        }
    }

    /// 공통 정책·큐 동작을 확인한다. OS의 실제 포커스 전달까지 검사하지는 않는다.
    #[test]
    fn capture_key_consumes_host_shortcut_and_passes_the_rest() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&["alt+d"], &[], &[]));

        assert!(bridge.capture_key(7, ch("d"), unknown_physical(), alt_mod()));

        assert!(!bridge.capture_key(7, ch("d"), unknown_physical(), ModifiersState::empty()));
        assert!(!bridge.capture_key(7, ch("D"), unknown_physical(), ModifiersState::SHIFT));
        assert!(!bridge.capture_key(7, ch("e"), unknown_physical(), alt_mod()));
        assert!(!bridge.capture_key(
            7,
            Key::Named(NamedKey::Escape),
            unknown_physical(),
            ModifiersState::empty()
        ));

        let pending = bridge.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].surface_id, 7);
        assert!(bridge.take_pending().is_empty());
    }

    #[test]
    fn a_bridge_without_a_policy_claims_nothing() {
        let bridge = WebViewKeyBridge::new();
        assert!(!bridge.capture_key(1, ch("d"), unknown_physical(), alt_mod()));
        assert!(bridge.take_pending().is_empty());
    }

    #[test]
    fn set_policy_replaces_the_previous_policy() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&["alt+d"], &[], &[]));
        bridge.set_policy(policy(&["alt+e"], &[], &[]));
        assert!(!bridge.capture_key(1, ch("d"), unknown_physical(), alt_mod()));
        assert!(bridge.capture_key(1, ch("e"), unknown_physical(), alt_mod()));
    }

    #[test]
    fn host_combos_are_filtered_by_modifier_and_deduplicated() {
        let p = policy(
            &["alt+d", "f", "shift+g", "alt+d", "ctrl+shift+k"],
            &[],
            &[],
        );
        assert_eq!(p.combos(), ["alt+d", "ctrl+shift+k"]);
    }

    #[test]
    fn plugin_command_bindings_join_the_policy() {
        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert!(
            !policy(&[], &[], &[]).claims(&ch("h"), mods),
            "sanity: 빈 정책은 ctrl+shift+h 를 안 가져간다"
        );

        let p = policy(&[], &[], &["ctrl+shift+h", "f"]);
        assert!(
            p.claims(&ch("h"), mods),
            "plugin 콤보가 정책에 없다: {:?}",
            p.combos()
        );
        assert!(!p.claims(&ch("f"), ModifiersState::empty()));
    }

    #[test]
    fn plugin_binding_cannot_take_a_page_reserved_combo() {
        let reserved = ["ctrl+f", "ctrl+alt+f"];
        let mut variants: Vec<String> = Vec::new();
        for combo in reserved {
            variants.push(combo.to_string());
            variants.push(combo.to_ascii_uppercase());
            variants.push(reorder_modifiers(combo));
        }
        // 표기 변형이 원문과 달라야 문자열 비교와 의미 비교를 구분할 수 있다.
        assert!(
            variants.iter().any(|v| !reserved.contains(&v.as_str())),
            "sanity: 표기 변형이 원본과 다른 문자열이어야 한다: {variants:?}"
        );

        let p = HostShortcutPolicy::new(ShortcutSources {
            host: Vec::new(),
            page_reserved: strings(&reserved),
            plugin: variants.clone(),
        });
        assert!(
            p.combos().is_empty(),
            "page-reserved combo (표기 변형 포함) must stay with the page: {:?}",
            p.combos()
        );

        let p = HostShortcutPolicy::new(ShortcutSources {
            host: Vec::new(),
            page_reserved: strings(&reserved),
            plugin: strings(&["ctrl+shift+h"]),
        });
        assert_eq!(p.combos(), ["ctrl+shift+h"]);
    }

    /// host 목록은 호출자가 예약 액션을 제외하므로 여기서 예약 필터를 다시 적용하지 않는다.
    #[test]
    fn the_reserved_filter_applies_to_plugin_combos_only() {
        let p = policy(&["ctrl+f"], &["ctrl+f"], &[]);
        assert_eq!(p.combos(), ["ctrl+f"]);
    }

    #[test]
    fn non_latin_layout_matches_through_the_physical_fallback() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&[], &[], &["ctrl+shift+h"]));

        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert!(!bridge.capture_key(3, ch("Р"), unknown_physical(), mods));
        assert!(bridge.capture_key(3, ch("Р"), PhysicalKey::Code(KeyCode::KeyH), mods));

        let pending = bridge.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].key, ch("h"));
    }

    #[test]
    fn physical_fallback_only_applies_with_modifiers() {
        let key = shortcut_lookup_key(
            ch("Р"),
            &PhysicalKey::Code(KeyCode::KeyH),
            ModifiersState::SHIFT,
        );
        assert_eq!(key, ch("Р"));
        let key = shortcut_lookup_key(
            ch("Р"),
            &PhysicalKey::Code(KeyCode::KeyH),
            ModifiersState::CONTROL,
        );
        assert_eq!(key, ch("h"));
    }

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
