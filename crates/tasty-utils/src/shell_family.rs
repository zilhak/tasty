//! 셸 바이너리 경로에서 계열(bash/zsh/기타)을 판정한다.
//!
//! `GeneralSettings::shell`(바이너리 경로 문자열) 하나만으로 bash 전용 rcfile
//! 주입(`--rcfile`)이나 zsh 전용 `ZDOTDIR` 스왑 같은 계열별 셸 통합 주입을 적용할지
//! 결정해야 하는 tasty-settings/tasty-terminal 양쪽이 공유하는 하위 crate
//! (`tasty-utils`)에 둔다 — 어느 한쪽 crate 전용으로 두면 반대쪽이 재사용할 수 없다.

/// 판정된 셸 계열. `Other` 는 미지원(fish/nu/pwsh 등 이번 범위 밖) — OSC133 자동
/// 주입 없이 조용히 넘어간다(기존 미지원 셸과 동일한 동작, 회귀 아님).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Bash,
    Zsh,
    Other,
}

impl ShellFamily {
    /// `shell_path`(바이너리 경로 문자열)의 basename 으로 판정한다(`.exe` 접미사
    /// 제거, 대소문자 무시).
    ///
    /// **알려진 한계(의도적 범위 제한)**: symlink 대상을 resolve 하지 않는다 —
    /// 예를 들어 `/usr/bin/sh` 가 bash 로 symlink 되어 있어도 basename 이 `sh` 면
    /// `Other` 로 판정한다. POSIX 호환 모드(`sh` 로 invoke 된 bash)도 마찬가지로
    /// 감지하지 않는다. 오판정의 결과는 "자동 주입을 안 함"(현재 미지원 셸과 동일한
    /// 동작)일 뿐 — 잘못된 셸에 bash/zsh 전용 인자를 주입해 셸이 즉사하는 실패
    /// 모드보다 안전한 쪽으로 치우친 설계다.
    pub fn detect(shell_path: &str) -> Self {
        let stem = std::path::Path::new(shell_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
        match stem {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bash_by_basename() {
        assert_eq!(ShellFamily::detect("/bin/bash"), ShellFamily::Bash);
        assert_eq!(
            ShellFamily::detect("C:/Program Files/Git/bin/bash.exe"),
            ShellFamily::Bash
        );
        assert_eq!(ShellFamily::detect("BASH.EXE"), ShellFamily::Bash);
    }

    #[test]
    fn detects_zsh_by_basename() {
        assert_eq!(ShellFamily::detect("/bin/zsh"), ShellFamily::Zsh);
        assert_eq!(ShellFamily::detect("/usr/local/bin/zsh"), ShellFamily::Zsh);
    }

    #[test]
    fn other_shells_and_empty_fall_back_to_other() {
        assert_eq!(ShellFamily::detect("/bin/fish"), ShellFamily::Other);
        assert_eq!(ShellFamily::detect("/usr/bin/sh"), ShellFamily::Other);
        assert_eq!(ShellFamily::detect(""), ShellFamily::Other);
        assert_eq!(ShellFamily::detect("cmd.exe"), ShellFamily::Other);
    }

    #[test]
    fn symlinked_or_sh_compat_bash_is_not_detected() {
        // 알려진 한계 문서화 — basename 이 bash/zsh 가 아니면 실제 바이너리가
        // 무엇이든 Other 로 떨어진다.
        assert_eq!(ShellFamily::detect("/bin/sh"), ShellFamily::Other);
    }
}
