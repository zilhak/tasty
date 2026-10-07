//! journal 시험이 기본 터미널로 띄우는 셸.
//!
//! 시험은 셸 출력을 보지 않고 터미널 자원이 만들어지는지만 본다. Unix 는 아무것도 출력하지 않고
//! 기다리는 `sleep` 으로 바꾼다. Windows 에는 `/bin/sh` 가 없어 그 셸로는 터미널이 만들어지지 않으므로
//! 기본 셸인 `cmd.exe` 를 그대로 띄운다.

pub(super) fn use_quiet_shell(settings: &mut crate::settings::Settings) {
    #[cfg(unix)]
    {
        settings.general.shell = "/bin/sh".into();
        settings.general.startup_command = "exec sleep 60".into();
    }
    #[cfg(windows)]
    {
        settings.general.shell = "cmd.exe".into();
    }
}
