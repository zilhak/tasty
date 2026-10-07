//! release 단일 실행. 같은 데이터 홈에서 Tasty 프로세스는 하나만 둔다.
//!
//! 다시 실행한 두 번째 프로세스는 창 없이 실행 중인 Tasty에 요청을 넘기고 끝난다. 사용자가 앱 목록·
//! Dock·시작 메뉴로 열어 OS가 활성화 증거를 붙여 준 경우에만 기존 View를 OS가 올리게 하고, 증거가
//! 없으면 `tasty new window`와 같은 새 View 하나로 끝난다. Tasty에는 포커스를 주는 API가 없다
//! (원칙 3). 결정과 한계는 ADR-0059의 "단일 실행" 절에 있다. debug는 "홈 사용 중" 오류로 끝난다.

pub(crate) mod evidence;
pub(crate) mod instance_file;
pub(crate) mod launch_log;
pub(crate) mod second;

#[cfg(target_os = "linux")]
pub(crate) mod dbus;

/// 실행 중인 쪽의 활성화 서비스(Linux D-Bus 이름, Windows 등록 메시지)를 등록할지. release는 항상,
/// debug는 검증용 환경변수 `TASTY_DEBUG_SINGLE_INSTANCE=1`일 때만 등록한다. macOS 는 받는 경로가 없다.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn service_enabled() -> bool {
    !cfg!(debug_assertions) || std::env::var("TASTY_DEBUG_SINGLE_INSTANCE").is_ok_and(|v| v == "1")
}

/// 실행 중인 Tasty가 받은 외부 활성화 요청.
#[derive(Debug, Clone)]
pub(crate) struct ExternalActivation {
    pub(crate) evidence: evidence::LaunchEvidence,
    /// OS가 포그라운드 권한을 넘겨 준 요청(Windows 등록 메시지). 메시지에는 증거가 실리지 않는다.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) os_granted: bool,
}

impl ExternalActivation {
    /// 증거가 없는 요청은 아무것도 바꾸지 않는다.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn has_evidence(&self) -> bool {
        self.os_granted || self.evidence.is_present()
    }

    pub(crate) fn platform_evidence(
        &self,
    ) -> crate::platform::window_activation::ActivationEvidence {
        crate::platform::window_activation::ActivationEvidence {
            wayland_token: self.evidence.wayland_token.clone(),
            x11_startup_id: self.evidence.x11_startup_id.clone(),
            x11_timestamp: self
                .evidence
                .x11_startup_id
                .as_deref()
                .and_then(evidence::x11_timestamp),
        }
    }
}

/// 창 생성 속성에 활성화 토큰을 넣는다. 첫 인스턴스의 첫 창은 실행기의 대기 표시를 끝내고,
/// 증거가 있는 요청으로 여는 새 창은 OS가 활성화를 판단할 근거가 된다.
pub(crate) fn with_activation_token(
    attrs: winit::window::WindowAttributes,
    evidence: &evidence::LaunchEvidence,
    wayland: bool,
) -> winit::window::WindowAttributes {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::startup_notify::WindowAttributesExtStartupNotify;
        if let Some(token) = evidence.token_for_backend(wayland) {
            return attrs.with_activation_token(winit::window::ActivationToken::from_raw(
                token.to_string(),
            ));
        }
        attrs
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Linux 밖에서는 실행기 토큰이 없다.
        let _unused = (evidence, wayland);
        attrs
    }
}

/// 이벤트 루프가 Wayland 백엔드인지.
pub(crate) fn is_wayland(event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        event_loop.is_wayland()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Linux 밖에는 Wayland가 없다.
        let _unused = event_loop;
        false
    }
}
