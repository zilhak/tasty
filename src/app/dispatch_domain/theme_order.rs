//! 설정 적용 때 전역 Theme 설치와 `theme.changed` 발행의 순서를 고정한다.
//! 구독자는 payload에 색이 없어 수신 시점에 `theme.query`로 전역 Theme를 되읽는다
//! ([markdown Theme parity](../../../docs/plugins/markdown/index.md)).
//! 발행이 설치보다 앞서면 구독자는 직전 테마를 읽고 한 단계씩 밀린다.

use tasty_settings::{AppearanceSettings, Settings};

/// 새 설정의 전역 Theme를 설치한 뒤 `announce`를 실행한다.
/// `theme.changed` 발행은 `announce` 안에서만 한다.
pub(super) fn install_theme_then<R>(settings: &Settings, announce: impl FnOnce() -> R) -> R {
    tasty_themes::install_global_with_runtime(&settings.appearance, settings.theme_runtime());
    announce()
}

/// 전역 Theme 의 색 세트나 UI 배율을 정하는 외관 값이 하나라도 바뀌었는지.
/// 테마 ID·기본 색(`theme_base`)·색 override·라이트 여부·UI 배율을 본다. 이전 설정이 없으면
/// 바뀐 것으로 본다. 창·터미널 색 갱신과 `theme.changed` 발행이 이 판정 하나를 함께 쓴다.
/// 구독자가 `theme.query`로 되읽는 색·`is_light`·zoom 이 이 값들에서 나온다.
/// `accessibility.reduced_motion` 도 전역 Theme 에 들어가지만 `theme.query` 응답(`ThemeWire`)에
/// 없어 구독자가 다시 읽을 값이 없고 색이 아니므로 보지 않는다.
pub(super) fn appearance_changed(
    prev: Option<&AppearanceSettings>,
    new: &AppearanceSettings,
) -> bool {
    prev.is_none_or(|prev| {
        prev.theme != new.theme
            || prev.theme_base != new.theme_base
            || prev.theme_overrides != new.theme_overrides
            || prev.theme_is_light != new.theme_is_light
            || prev.ui_scale != new.ui_scale
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 색 세트·배율을 정하는 값 하나만 바뀌어도 theme.changed 대상이고, 다른 외관 값만 바뀌면 아니다.
    #[test]
    fn theme_changed_covers_every_value_behind_the_colours_and_zoom() {
        let base = Settings::default().appearance;
        assert!(appearance_changed(None, &base), "no previous settings");
        assert!(
            !appearance_changed(Some(&base), &base.clone()),
            "nothing changed"
        );

        let mut theme = base.clone();
        theme.theme = if base.theme == "latte" {
            "mocha"
        } else {
            "latte"
        }
        .to_string();
        assert!(appearance_changed(Some(&base), &theme), "theme id");

        let mut overrides = base.clone();
        overrides.theme_overrides.crust = Some(base.theme_base.text);
        assert!(
            appearance_changed(Some(&base), &overrides),
            "colour override"
        );

        let mut theme_base = base.clone();
        theme_base.theme_base.crust = base.theme_base.text;
        assert_ne!(theme_base.theme_base, base.theme_base, "sample must differ");
        assert!(appearance_changed(Some(&base), &theme_base), "theme base");

        let mut light = base.clone();
        light.theme_is_light = !base.theme_is_light;
        assert!(appearance_changed(Some(&base), &light), "light flag");

        let mut scale = base.clone();
        scale.ui_scale = if base.ui_scale == "large" {
            "small"
        } else {
            "large"
        }
        .to_string();
        assert!(appearance_changed(Some(&base), &scale), "ui scale");

        let mut other = base.clone();
        other.ligatures = !base.ligatures;
        assert!(
            !appearance_changed(Some(&base), &other),
            "unrelated appearance field"
        );
    }

    /// 자식 프로세스에서만 probe가 실행되도록 표시하는 환경변수. 값은 완료 표지 파일 경로다.
    const PROBE_ENV: &str = "TASTY_THEME_ORDER_PROBE";
    /// probe가 실제로 끝까지 실행됐음을 부모에 알리는 파일 내용.
    const PROBE_DONE: &str = "theme-order-probe-done";

    /// 테마를 여러 번 왕복해도 announce 시점의 전역 Theme 밝기가 매번 새 설정과 같다.
    /// 전역 Theme는 프로세스 전역이라 같은 테스트 바이너리의 다른 시험에 영향을 주지 않도록
    /// 자식 프로세스에서 probe를 실행한다.
    #[test]
    fn theme_changed_announcer_reads_the_newly_installed_theme() {
        let probe = format!(
            "{}::announce_sees_the_new_theme_probe",
            module_path!()
                .split_once("::")
                .map_or(module_path!(), |(_, rest)| rest)
        );
        let exe = std::env::current_exe().expect("current_exe");
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("probe-done");
        let output = std::process::Command::new(exe)
            .args([probe.as_str(), "--exact", "--test-threads=1"])
            .env(PROBE_ENV, &marker)
            .output()
            .expect("spawn probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "probe failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let done = std::fs::read_to_string(&marker).unwrap_or_default();
        assert_eq!(
            done, PROBE_DONE,
            "probe did not run to the end\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    /// 부모 시험이 자식 프로세스에서 실행한다. 환경변수가 없으면 아무것도 하지 않는다.
    #[test]
    fn announce_sees_the_new_theme_probe() {
        let Some(marker) = std::env::var_os(PROBE_ENV) else {
            return;
        };
        let mut settings = Settings::default();
        for round in 0..6 {
            let target = !crate::theme::theme().is_light;
            settings.appearance.theme_is_light = target;
            settings.appearance.theme = if target { "latte" } else { "mocha" }.to_string();
            let seen = install_theme_then(&settings, || crate::theme::theme().is_light);
            assert_eq!(
                seen, target,
                "round {round}: theme.changed would carry the previous theme"
            );
        }
        std::fs::write(marker, PROBE_DONE).expect("write probe marker");
    }
}
