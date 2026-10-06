//! 사용자 실행 증거(활성화 토큰)를 프로세스 시작 직후 읽어 보관하고 환경에서 지운다.
//!
//! 실행기는 사용자가 앱을 열 때 Wayland `XDG_ACTIVATION_TOKEN`, X11 `DESKTOP_STARTUP_ID`를
//! 환경에 넣어 준다. Tasty가 띄우는 셸·플러그인은 환경을 물려받으므로 남겨 두면 Tasty 터미널
//! 안의 에이전트가 아직 쓰지 않은 토큰을 얻는다. 그래서 스레드가 생기기 전에 두 값을 모두 읽어
//! 보관하고 지운다. 첫 인스턴스는 첫 창을 만들 때, 두 번째 프로세스는 실행 중인 Tasty에 넘길 때 쓴다.

use std::sync::Mutex;

/// Wayland xdg-activation 토큰 환경변수.
pub(crate) const WAYLAND_TOKEN_ENV: &str = "XDG_ACTIVATION_TOKEN";
/// X11 startup-notification id 환경변수.
pub(crate) const X11_STARTUP_ID_ENV: &str = "DESKTOP_STARTUP_ID";

/// `org.freedesktop.Application.Activate`의 `platform_data` 키.
#[cfg(any(target_os = "linux", test))]
pub(crate) const PLATFORM_DATA_ACTIVATION_TOKEN: &str = "activation-token";
#[cfg(any(target_os = "linux", test))]
pub(crate) const PLATFORM_DATA_DESKTOP_STARTUP_ID: &str = "desktop-startup-id";

/// 실행기가 넘긴 활성화 증거. 값 자체는 로그에 남기지 않는다.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LaunchEvidence {
    pub(crate) wayland_token: Option<String>,
    pub(crate) x11_startup_id: Option<String>,
}

impl std::fmt::Debug for LaunchEvidence {
    // 아직 쓰지 않은 토큰이 로그로 새지 않도록 유무만 보인다.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchEvidence")
            .field("wayland_token", &self.wayland_token.is_some())
            .field("x11_startup_id", &self.x11_startup_id.is_some())
            .finish()
    }
}

impl LaunchEvidence {
    /// 빈 문자열은 증거로 보지 않는다.
    pub(crate) fn from_values(
        wayland_token: Option<String>,
        x11_startup_id: Option<String>,
    ) -> Self {
        let present = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
        Self {
            wayland_token: present(wayland_token),
            x11_startup_id: present(x11_startup_id),
        }
    }

    pub(crate) fn is_present(&self) -> bool {
        self.wayland_token.is_some() || self.x11_startup_id.is_some()
    }

    /// 로그에 남길 증거 종류. 값은 넣지 않는다.
    pub(crate) fn kinds(&self) -> Vec<&'static str> {
        let mut kinds = Vec::new();
        if self.wayland_token.is_some() {
            kinds.push(WAYLAND_TOKEN_ENV);
        }
        if self.x11_startup_id.is_some() {
            kinds.push(X11_STARTUP_ID_ENV);
        }
        kinds
    }

    #[cfg(any(target_os = "linux", test))]
    /// `Activate`에 실을 `platform_data` 항목.
    pub(crate) fn platform_data(&self) -> Vec<(&'static str, String)> {
        let mut data = Vec::new();
        if let Some(token) = &self.wayland_token {
            data.push((PLATFORM_DATA_ACTIVATION_TOKEN, token.clone()));
        }
        if let Some(id) = &self.x11_startup_id {
            data.push((PLATFORM_DATA_DESKTOP_STARTUP_ID, id.clone()));
        }
        data
    }

    #[cfg(any(target_os = "linux", test))]
    /// `Activate`로 받은 `platform_data`에서 증거를 꺼낸다. 다른 키는 무시한다.
    pub(crate) fn from_platform_data<'a>(
        entries: impl IntoIterator<Item = (&'a str, Option<String>)>,
    ) -> Self {
        let mut wayland_token = None;
        let mut x11_startup_id = None;
        for (key, value) in entries {
            match key {
                PLATFORM_DATA_ACTIVATION_TOKEN => wayland_token = value,
                PLATFORM_DATA_DESKTOP_STARTUP_ID => x11_startup_id = value,
                _ => {}
            }
        }
        Self::from_values(wayland_token, x11_startup_id)
    }

    #[cfg(any(target_os = "linux", test))]
    /// 현재 표시 백엔드에 맞는 토큰. Wayland 세션이면 Wayland 토큰을, 아니면 X11 startup id를 고른다.
    /// winit은 창 생성 속성에 토큰 하나만 받으므로 백엔드가 읽는 쪽을 넘긴다.
    pub(crate) fn token_for_backend(&self, wayland: bool) -> Option<&str> {
        if wayland {
            self.wayland_token.as_deref()
        } else {
            self.x11_startup_id.as_deref()
        }
    }
}

/// startup id의 `_TIME<타임스탬프>` 부분. X11 `_NET_ACTIVE_WINDOW` 요청의 사용자 조작 시각이다.
pub(crate) fn x11_timestamp(startup_id: &str) -> Option<u32> {
    let (_, rest) = startup_id.rsplit_once("_TIME")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

static CAPTURED: Mutex<Option<LaunchEvidence>> = Mutex::new(None);

/// 두 환경변수를 읽어 보관하고 프로세스 환경에서 지운다. `run()` 맨 앞에서 한 번 부른다.
pub(crate) fn capture_and_clear_env() {
    let evidence = LaunchEvidence::from_values(
        std::env::var(WAYLAND_TOKEN_ENV).ok(),
        std::env::var(X11_STARTUP_ID_ENV).ok(),
    );
    for key in [WAYLAND_TOKEN_ENV, X11_STARTUP_ID_ENV] {
        if std::env::var_os(key).is_some() {
            // SAFETY: run()의 첫 단계에서 부르며 이때는 환경에 접근하는 다른 스레드가 없다.
            unsafe { std::env::remove_var(key) };
        }
    }
    match CAPTURED.lock() {
        Ok(mut slot) => *slot = Some(evidence),
        Err(e) => tracing::warn!("launch evidence slot poisoned: {e}"),
    }
}

/// 보관한 증거를 꺼낸다. 토큰은 한 번만 쓰므로 꺼낸 뒤에는 비어 있다.
pub(crate) fn take() -> LaunchEvidence {
    match CAPTURED.lock() {
        Ok(mut slot) => slot.take().unwrap_or_default(),
        Err(e) => {
            tracing::warn!("launch evidence slot poisoned: {e}");
            LaunchEvidence::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_values_are_not_evidence() {
        let e = LaunchEvidence::from_values(Some(String::new()), Some("  ".into()));
        assert!(!e.is_present());
        assert!(e.platform_data().is_empty());
    }

    #[test]
    fn each_variable_maps_to_its_platform_data_key() {
        let wayland = LaunchEvidence::from_values(Some("tok".into()), None);
        assert_eq!(
            wayland.platform_data(),
            vec![(PLATFORM_DATA_ACTIVATION_TOKEN, "tok".to_string())]
        );
        let x11 = LaunchEvidence::from_values(None, Some("id_TIME5".into()));
        assert_eq!(
            x11.platform_data(),
            vec![(PLATFORM_DATA_DESKTOP_STARTUP_ID, "id_TIME5".to_string())]
        );
        let both = LaunchEvidence::from_values(Some("tok".into()), Some("id".into()));
        assert_eq!(both.platform_data().len(), 2);
        assert_eq!(both.kinds(), vec![WAYLAND_TOKEN_ENV, X11_STARTUP_ID_ENV]);
    }

    #[test]
    fn platform_data_round_trips_and_ignores_other_keys() {
        let e = LaunchEvidence::from_platform_data([
            (
                PLATFORM_DATA_DESKTOP_STARTUP_ID,
                Some("gnome_TIME42".to_string()),
            ),
            ("other", Some("x".to_string())),
        ]);
        assert_eq!(e.x11_startup_id.as_deref(), Some("gnome_TIME42"));
        assert!(e.wayland_token.is_none());
        let none = LaunchEvidence::from_platform_data([("other", Some("x".to_string()))]);
        assert!(!none.is_present());
    }

    #[test]
    fn the_backend_picks_its_own_token() {
        let both = LaunchEvidence::from_values(Some("tok".into()), Some("id".into()));
        assert_eq!(both.token_for_backend(true), Some("tok"));
        assert_eq!(both.token_for_backend(false), Some("id"));
    }

    #[test]
    fn x11_timestamp_is_read_from_the_time_suffix() {
        assert_eq!(
            x11_timestamp("gnome-shell-123-host-tasty-1_TIME123456"),
            Some(123456)
        );
        assert_eq!(x11_timestamp("id_TIME77+extra"), Some(77));
        assert_eq!(x11_timestamp("no-time-here"), None);
        assert_eq!(x11_timestamp("id_TIME"), None);
    }

    #[test]
    fn debug_output_never_contains_the_token() {
        let e = LaunchEvidence::from_values(Some("secret-token".into()), Some("secret-id".into()));
        let shown = format!("{e:?}");
        assert!(!shown.contains("secret"), "{shown}");
    }
}
