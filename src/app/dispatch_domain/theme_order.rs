//! 설정 적용 때 전역 Theme 설치와 `theme.changed` 발행의 순서를 고정한다.
//! 구독자는 payload에 색이 없어 수신 시점에 `theme.query`로 전역 Theme를 되읽는다
//! ([markdown Theme parity](../../../docs/plugins/markdown/index.md)).
//! 발행이 설치보다 앞서면 구독자는 직전 테마를 읽고 한 단계씩 밀린다.

use tasty_settings::Settings;

/// 새 설정의 전역 Theme를 설치한 뒤 `announce`를 실행한다.
/// `theme.changed` 발행은 `announce` 안에서만 한다.
pub(super) fn install_theme_then<R>(settings: &Settings, announce: impl FnOnce() -> R) -> R {
    tasty_themes::install_global_with_runtime(&settings.appearance, settings.theme_runtime());
    announce()
}

#[cfg(test)]
mod tests {
    use super::*;

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
