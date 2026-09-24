//! webview 자식 창이 받은 키를 host 단축키 계층으로 올리는 **플랫폼 공통 계약**.
//!
//! native webview 는 세 OS 모두 winit 창과 별개의 OS 자식 창/뷰다(X11 child window /
//! WKWebView subview / child HWND). 그 자식이 OS 키보드 포커스를 잡으면 winit 은
//! `WindowEvent::KeyboardInput` 을 받지 못하고, host 단축키 경로
//! (`view/main/keyboard.rs` → `shortcuts::dispatch`)가 통째로 도달 불가능해진다.
//! 이 모듈은 그 구멍을 메우는 단일 계약이다 — 세 백엔드는 자기 native 키 이벤트를
//! [`WebViewKeyEvent`] 로 정규화해 [`WebViewKeySink`] 계약으로만 올리고, **우선순위 판정은
//! 그 구현([`WebViewKeyBridge`]) 한 곳에서만** 한다(백엔드마다 규칙이 갈라지지 않게).
//!
//! 결정 배경·대안·재검토 조건: `docs/adr/0029-webview-host-integration.md`.
//!
//! # 판정은 동기, 실행은 다음 프레임
//!
//! [`WebViewKeySink::capture_key`] 는 백엔드 콜백 안에서 **동기적으로** "host 가
//! 이 키를 가져가는가" 를 답한다 — 백엔드는 그 답으로 페이지 전파를 그 자리에서
//! 막을지 정한다. 실제 액션 실행은 host 가 다음 프레임에 큐를 비우며 수행한다.
//! 판정이 이미 끝났으므로 페이지와 host 가 같은 키를 이중 처리하는 일은 없다.
//!
//! # 무엇을 가져가는가 (우선순위 규칙)
//!
//! [`HostShortcutPolicy`] 는 콤보 목록을 **주입받는다** — 이 모듈은 `KeybindingSettings`
//! 도 plugin 레지스트리도 모른다. 어느 설정 필드가 host 액션이고 어느 것이 페이지 예약
//! 액션인지는 host 단축키 계층이 정해 [`ShortcutSources`] 로 넘긴다
//! (`adapters/ui/input/shortcuts/webview_claims.rs`). 이 파일에 키 콤보 리터럴은 하나도
//! 없다(CLAUDE.md "단축키" 정책 — 하드코딩 금지). 여기서 하는 판정은 두 축이다.
//!
//! 1. **modifier 를 가진 콤보만** 후보다(`ctrl` / `alt` / `option` 중 하나 이상).
//!    수식 없는 키와 `shift` 만 붙은 키는 페이지 소유로 남긴다 — 그래야 문서 안
//!    텍스트 입력(주소창·find 바)의 타이핑, IME 조합, 폼 내비게이션(Tab/Enter/
//!    화살표), 페이지의 Esc 처리가 전부 그대로 살아있다.
//! 2. **페이지 예약 콤보와 동등한 plugin 콤보는 제외**한다. 페이지가 자체로 같은
//!    의미를 구현하는 액션(find-in-page, 복사/잘라내기/붙여넣기/전체선택)은 누가 같은
//!    콤보를 바인딩했든 브라우저와 동일하게 페이지가 갖는다.
//!
//! plugin 명령 바인딩도 같은 두 축을 통과하면 정책에 오른다. 어떤 커맨드가 실제로
//! 발화하는지는 host 가 큐를 비울 때 focused surface 기준으로 다시 좁히므로
//! (`app/plugin_glue/shortcut.rs`), 정책이 담는 plugin 콤보 집합은 의도적으로 **상위집합**이다.
//!
//! # 레이아웃 폴백 (비라틴 키보드)
//!
//! 백엔드가 올리는 "레이아웃이 낸 문자"(GDK keyval / `charactersIgnoringModifiers` /
//! Win32 VK)는 러시아어 같은 비라틴 레이아웃에서 키캡과 다르다. winit 키 경로가 쓰는
//! 규칙(`view/main/keyboard.rs::shortcut_lookup_key`)과 **똑같이**, ctrl/super/alt 중
//! 하나라도 눌려 있으면 물리 키의 US 배열 기준 문자를 우선한다
//! ([`tasty_key_match::physical_key_to_logical`]). 백엔드는 자기 native scancode 를
//! winit [`PhysicalKey`] 로 변환해 넘기고, 변환할 수 없으면
//! [`PhysicalKey::Unidentified`] 를 넘겨 폴백 없이 레이아웃 문자를 그대로 쓴다.
//!
//! key-up / repeat 은 애초에 큐에 오르지 않는다 — 백엔드가 **press·비repeat** 만
//! [`capture_key`](WebViewKeySink::capture_key) 를 호출한다. host 는 modifier 상태를
//! 이벤트에 실려온 값으로만 읽고 자기 `base.modifiers` 를 갱신하지 않으므로,
//! "modifier down 은 webview / up 은 host" 경계에서 상태가 눌린 채 남는 일이 없다.

use std::cell::RefCell;

use winit::keyboard::{Key, ModifiersState, PhysicalKey};

/// 백엔드가 채우는 정규화된 키 이벤트. 세 OS 의 native 키 표현
/// (GDK keyval / NSEvent charactersIgnoringModifiers / Win32 VK)을 winit 타입으로
/// 맞춰 올린다 — host 단축키 매칭이 winit `Key`/`ModifiersState` 기준이라
/// 여기서 한 번만 변환하면 이후 경로가 winit 키 경로와 완전히 같아진다.
#[derive(Debug, Clone)]
pub struct WebViewKeyEvent {
    /// 키를 받은 webview 가 붙어 있는 surface. host 는 큐를 비울 때 이 surface 의
    /// webview 가 아직 살아 있는지 확인하는 데 쓴다(콜백과 drain 사이에 surface 가
    /// 닫히는 레이스). 모델 포커스 이동은 키가 아니라 **클릭**([`WebViewKeySink::note_focus`])
    /// 에만 붙인다 — 이유는 `app/webview_keys.rs` 참조.
    pub surface_id: u32,
    /// 이미 레이아웃 폴백이 적용된 **조회 키**다(원본 레이아웃 문자가 아니다) —
    /// winit 경로가 `shortcut_lookup_key` 로 만드는 값과 같은 것이라, host 는 이 값을
    /// 그대로 `handle_shortcut`/`dispatch_plugin_shortcut_key` 에 넘기면 된다.
    pub key: Key,
    pub mods: ModifiersState,
}

/// winit 키 경로(`view/main/keyboard.rs::shortcut_lookup_key`)와 **같은 규칙**의 조회 키
/// 결정. modifier 조합일 때만 물리 키의 US 배열 문자를 우선한다 — 수식 없는 키까지
/// 물리로 덮으면 페이지 텍스트 입력이 깨진다.
fn shortcut_lookup_key(key: Key, physical: &PhysicalKey, mods: ModifiersState) -> Key {
    if mods.control_key() || mods.super_key() || mods.alt_key() {
        tasty_key_match::physical_key_to_logical(physical).unwrap_or(key)
    } else {
        key
    }
}

/// [`HostShortcutPolicy`] 에 주입하는 콤보 세 묶음. 셋 다 바인딩 문자열 그대로다 —
/// 어느 설정 필드에서 왔는지는 이 모듈이 모른다.
#[derive(Debug, Clone, Default)]
pub struct ShortcutSources {
    /// host 액션의 콤보. **페이지 예약 액션의 콤보는 이미 뺀 것**이어야 한다 — 어느
    /// 액션이 예약인지는 액션 id 의 성질이라 host 쪽이 안다.
    pub host: Vec<String>,
    /// 페이지 예약 액션(찾기·복사·잘라내기·붙여넣기·전체선택)의 콤보. plugin 콤보가
    /// 이것과 **동등**하면 정책에 오르지 않는다.
    pub page_reserved: Vec<String>,
    /// plugin 명령의 effective binding(매니페스트 default + 사용자 override 합성 결과).
    pub plugin: Vec<String>,
}

/// host 가 가져갈 콤보의 스냅샷. `KeybindingSettings` 와 plugin 명령 레지스트리가
/// 바뀐 프레임에만 다시 만들어 브리지에 밀어 넣는다(`view/main/redraw.rs`).
#[derive(Debug, Clone, Default)]
pub struct HostShortcutPolicy {
    combos: Vec<String>,
}

impl HostShortcutPolicy {
    /// 주입받은 콤보를 위 두 축(modifier 보유 / 페이지 예약과 동등한 plugin 콤보 제외)으로
    /// 걸러 정책을 만든다. 순서는 host → plugin, 같은 문자열은 한 번만 담는다.
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

        // plugin 명령 바인딩. 페이지 예약 콤보와 겹치면 넣지 않는다 —
        // find-in-page·복사/붙여넣기는 "누가 같은 콤보를 바인딩했든" 페이지가 갖는다는
        // 것이 규칙이라, plugin 이 그 콤보를 쓴다고 규칙이 뒤집히면 안 된다.
        for combo in plugin {
            // 콤보 **동등성**으로 비교한다 — 원시 문자열 비교면 plugin 매니페스트가
            // `"Ctrl+F"` 로 선언한 콤보가 사용자의 `find`=`"ctrl+f"` 와 같은 의미인데도
            // 필터를 통과해, webview 안에서 페이지의 find-in-page 가 죽는다.
            // 매칭(`matches_binding`)과 같은 파싱 경로를 재사용한다.
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

    /// 정책에 오른 콤보(주입 순서 보존). host 쪽 도출 규칙의 단위 시험이 읽는다.
    #[cfg(test)]
    pub(crate) fn combos(&self) -> &[String] {
        &self.combos
    }

    /// 이 키가 host 가 가져갈 콤보에 해당하는가. 매칭은 winit 키 경로와 **같은**
    /// `matches_binding` 을 쓴다(플랫폼별 modifier 매핑 규칙도 그대로 따른다 —
    /// macOS 의 `alt`→Command / `option`→Option 포함).
    pub fn claims(&self, key: &Key, mods: ModifiersState) -> bool {
        tasty_key_match::matches_any_binding(&self.combos, key, mods)
    }
}

/// 백엔드가 host 에 대해 아는 **전부**. `PlatformWebView::new` 는 구체 브리지가 아니라
/// 이 계약을 받는다 — 백엔드는 "이 키를 host 가 가져가는가" 를 묻고 "이 surface 가
/// 클릭됐다" 를 알리는 것 말고는 정책·큐·설정 어느 것도 모른다. 그래서 정책 판정이나
/// 큐의 모양이 바뀌어도 세 백엔드 본문은 안 바뀐다. 근거·대안:
/// `docs/adr/0029-webview-host-integration.md`.
///
/// **스레드**: 세 백엔드의 콜백(GTK 시그널 / WKWebView 메서드 / WebView2 이벤트)은 모두
/// winit main thread 에서 발화한다. 그래서 구현은 `Send`/`Sync` 를 요구받지 않고 `Rc` 로
/// 공유된다 — 구현이 `RefCell` 을 쓸 수 있는 근거가 그 친화성이다.
pub trait WebViewKeySink {
    /// 이 키를 host 가 가져가면 `true`(백엔드는 페이지 전파를 **그 자리에서** 중단),
    /// 아니면 `false`(페이지가 평소대로 처리). 판정은 동기다.
    ///
    /// 백엔드는 **key press 이고 repeat 이 아닌** 이벤트에서만 호출한다. `key` 는
    /// 레이아웃이 낸 문자, `physical` 은 그 키의 물리 위치다. 물리 위치를 알 수 없는
    /// 백엔드는 [`PhysicalKey::Unidentified`] 를 넘긴다.
    fn capture_key(
        &self,
        surface_id: u32,
        key: Key,
        physical: PhysicalKey,
        mods: ModifiersState,
    ) -> bool;

    /// 백엔드가 native 클릭/포커스 획득을 관측했을 때 호출. 클릭이 winit 에 도달하지
    /// 않아 `try_click_to_activate` 가 실행되지 않는 구조적 공백을 host 가 메우는 입력이다.
    fn note_focus(&self, surface_id: u32);
}

/// [`WebViewKeySink`] 의 host 구현. `MainView` 가 하나를 소유해 그 창의 모든 webview 에
/// `Rc` 로 공유한다.
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

    /// host 가 콤보 스냅샷이 바뀐 프레임에 새 정책을 밀어 넣는다.
    pub fn set_policy(&self, policy: HostShortcutPolicy) {
        *self.policy.borrow_mut() = policy;
    }

    /// host 가 매 프레임 큐를 비운다(도착 순서 보존).
    pub fn take_pending(&self) -> Vec<WebViewKeyEvent> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }

    /// host 가 매 프레임 포커스 동기화 요청을 비운다. host 는 이 값으로 모델
    /// 포커스(`focused_surface`/`focused_pane`)를 그 surface 로 옮긴다.
    pub fn take_focus_requests(&self) -> Vec<u32> {
        std::mem::take(&mut *self.focus_requests.borrow_mut())
    }
}

impl WebViewKeySink for WebViewKeyBridge {
    /// `true` 인 경우에만 큐에 쌓이며, host 가 다음 프레임에 비워 실제 액션을 실행한다.
    /// 판정 직전에 [`shortcut_lookup_key`] 로 레이아웃 문자와 물리 위치를 한 번 합쳐
    /// winit 경로와 같은 조회 키를 만든다.
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

    /// 연속 중복은 접는다 — 같은 surface 를 여러 번 클릭해도 요청은 하나다.
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

    /// 물리 위치를 알 수 없는 백엔드가 넘기는 값.
    fn unknown_physical() -> PhysicalKey {
        PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
    }

    /// 콤보의 modifier 프리픽스 순서를 뒤집는다(`"ctrl+alt+f"` → `"alt+ctrl+f"`).
    /// modifier 가 하나뿐이면 원본을 그대로 돌려준다.
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

    /// 바인딩 토큰 `alt` 의 실제 modifier(macOS 는 Command).
    fn alt_mod() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::ALT
        }
    }

    /// 백엔드에서 올라온 키가 주입된 host 콤보에 매칭되면 소비(`true`), 아니면 페이지로
    /// 흘림(`false`). 키 라우팅 규칙이 단위 테스트로 고정되는 유일한 지점이다
    /// (OS 포커스 자체는 단위 테스트로 재현할 수 없다).
    #[test]
    fn capture_key_consumes_host_shortcut_and_passes_the_rest() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&["alt+d"], &[], &[]));

        assert!(bridge.capture_key(7, ch("d"), unknown_physical(), alt_mod()));

        // 수식 없는 문자 — 페이지 타이핑이므로 흘린다.
        assert!(!bridge.capture_key(7, ch("d"), unknown_physical(), ModifiersState::empty()));
        // shift 만 붙은 문자도 타이핑이다.
        assert!(!bridge.capture_key(7, ch("D"), unknown_physical(), ModifiersState::SHIFT));
        // 주입되지 않은 콤보는 modifier 가 있어도 페이지 것이다.
        assert!(!bridge.capture_key(7, ch("e"), unknown_physical(), alt_mod()));
        // 페이지가 자체 처리하는 Esc/Enter 는 수식이 없으므로 흘린다.
        assert!(!bridge.capture_key(
            7,
            Key::Named(NamedKey::Escape),
            unknown_physical(),
            ModifiersState::empty()
        ));

        // 소비한 1건만 큐에 남고, 비우면 다시 빈다.
        let pending = bridge.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].surface_id, 7);
        assert!(bridge.take_pending().is_empty());
    }

    /// 정책을 한 번도 받지 않은 브리지는 아무것도 가로채지 않는다 — 생성 직후 첫
    /// 프레임 전에 키가 와도 페이지가 받는다.
    #[test]
    fn a_bridge_without_a_policy_claims_nothing() {
        let bridge = WebViewKeyBridge::new();
        assert!(!bridge.capture_key(1, ch("d"), unknown_physical(), alt_mod()));
        assert!(bridge.take_pending().is_empty());
    }

    /// 정책은 교체된다 — 사용자가 콤보를 바꾸면 옛 콤보는 더 이상 가로채지 않는다.
    #[test]
    fn set_policy_replaces_the_previous_policy() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&["alt+d"], &[], &[]));
        bridge.set_policy(policy(&["alt+e"], &[], &[]));
        assert!(!bridge.capture_key(1, ch("d"), unknown_physical(), alt_mod()));
        assert!(bridge.capture_key(1, ch("e"), unknown_physical(), alt_mod()));
    }

    /// 주입된 host 콤보라도 modifier 가 없으면 정책에 오르지 않는다 — host 가 `f1`
    /// 같은 맨키를 바인딩해도 문서 안 타이핑은 페이지 것이다. 같은 콤보는 한 번만 담는다.
    #[test]
    fn host_combos_are_filtered_by_modifier_and_deduplicated() {
        let p = policy(
            &["alt+d", "f", "shift+g", "alt+d", "ctrl+shift+k"],
            &[],
            &[],
        );
        assert_eq!(p.combos(), ["alt+d", "ctrl+shift+k"]);
    }

    /// plugin 명령 바인딩이 정책에 합류한다 — 이게 없으면 webview 위에서 그 콤보를
    /// 페이지가 먹는다. 수식 없는 plugin 바인딩은 host 콤보와 같은 규칙으로 페이지에 남긴다.
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

    /// 페이지 예약 콤보는 plugin 이 같은 콤보를 바인딩해도 페이지가 갖는다. plugin
    /// 매니페스트는 raw 문자열이라 대문자·modifier 순서가 사용자 설정과 다를 수 있으므로
    /// 원본 + 대문자화 + modifier 순서 뒤바꿈을 모두 넣는다.
    #[test]
    fn plugin_binding_cannot_take_a_page_reserved_combo() {
        let reserved = ["ctrl+f", "ctrl+alt+f"];
        let mut variants: Vec<String> = Vec::new();
        for combo in reserved {
            variants.push(combo.to_string());
            variants.push(combo.to_ascii_uppercase());
            variants.push(reorder_modifiers(combo));
        }
        // 표기 변형이 실제로 원본과 다른 문자열인지 — 아니면 이 테스트가 raw 비교
        // 구멍을 못 잡는다.
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

        // 예약과 무관한 plugin 콤보는 같은 주입에서 그대로 오른다(필터가 전부 막는 것이 아님).
        let p = HostShortcutPolicy::new(ShortcutSources {
            host: Vec::new(),
            page_reserved: strings(&reserved),
            plugin: strings(&["ctrl+shift+h"]),
        });
        assert_eq!(p.combos(), ["ctrl+shift+h"]);
    }

    /// 예약 필터는 plugin 콤보에만 걸린다 — host 쪽은 예약 액션을 이미 뺀 목록을 넘기는
    /// 것이 계약이라, 여기서 다시 거르면 host 액션이 예약 콤보를 **공유**하는 설정에서
    /// 그 host 액션이 조용히 죽는다(종전 동작과도 다르다).
    #[test]
    fn the_reserved_filter_applies_to_plugin_combos_only() {
        let p = policy(&["ctrl+f"], &["ctrl+f"], &[]);
        assert_eq!(p.combos(), ["ctrl+f"]);
    }

    /// 비라틴 레이아웃: 러시아어 배열에서 `ctrl+shift+H` 를 누르면 레이아웃이 내는
    /// 문자는 `Р` 이지만 물리 위치는 `KeyH` 다. winit 경로가 modifier 조합에서
    /// physical 폴백을 쓰므로 포워딩 경로도 같은 판정을 내려야 한다.
    #[test]
    fn non_latin_layout_matches_through_the_physical_fallback() {
        let bridge = WebViewKeyBridge::new();
        bridge.set_policy(policy(&[], &[], &["ctrl+shift+h"]));

        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        // 레이아웃 문자만 보면 매칭되지 않는다(폴백이 없던 시절의 동작).
        assert!(!bridge.capture_key(3, ch("Р"), unknown_physical(), mods));
        // 물리 위치가 실리면 winit 경로와 같은 조회 키(`h`)로 매칭된다.
        assert!(bridge.capture_key(3, ch("Р"), PhysicalKey::Code(KeyCode::KeyH), mods));

        // 큐에 오른 키도 폴백이 적용된 조회 키다 — host 가 그대로 디스패치한다.
        let pending = bridge.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].key, ch("h"));
    }

    /// 수식이 없으면 물리 폴백을 쓰지 않는다 — 페이지 텍스트 입력이 US 배열 문자로
    /// 바뀌면 안 된다(winit 경로의 `shortcut_lookup_key` 와 같은 조건).
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
