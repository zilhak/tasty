//! 실행 파일의 basename으로 bash/zsh/기타를 구분한다. tasty-settings의 셸 통합 설정에 사용한다.
//! Other는 bashrc/zshenv 자동 주입을 적용하지 않는 계열이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Bash,
    Zsh,
    Other,
}

impl ShellFamily {
    /// 대소문자와 .exe 접미사를 무시하고 basename을 비교한다.
    /// symlink는 해석하지 않으므로 bash를 가리키는 /bin/sh도 Other다.
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
        assert_eq!(ShellFamily::detect("/bin/sh"), ShellFamily::Other);
    }
}
