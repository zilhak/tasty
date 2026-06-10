use serde::{Deserialize, Serialize};

/// Tasty 쉘 모드에서 항상 앞단에 prepend되는 빌트인 bashrc. UI에서 노출되지 않으며
/// 사용자가 편집할 수 없다. 파생 `~/.tasty/bashrc`는 저장 시마다 `BUILTIN + "\n" + user`로
/// 재생성되므로 템플릿 업데이트가 기존 사용자에게도 자동 반영된다.
pub const BUILTIN_BASHRC: &str = r#"# === tasty built-in (auto-generated, do not edit) ===
# This section is regenerated every time settings are saved.
# Put your customizations in ~/.tasty/bashrc.user instead.

# UTF-8
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8

# Inherit Windows PATH
ORIGINAL_PATH="${ORIGINAL_PATH:-${PATH}}"
export PATH="/usr/local/bin:/usr/bin:/bin:${ORIGINAL_PATH}"

# Emit OSC 7 so Tasty inherits cwd when opening new tabs/splits.
# Only re-emits when PWD actually changes, and avoids a cygpath fork by
# converting MSYS drive paths (/c/...) to Windows form (/C:/...) in pure bash.
# Virtual MSYS mounts (/tmp, /usr, ...) fall back to cygpath, which is rare.
__tasty_osc7() {
    [[ "$PWD" == "$__TASTY_LAST_PWD" ]] && return
    __TASTY_LAST_PWD="$PWD"
    local pwd_emit="$PWD"
    if [[ "$pwd_emit" =~ ^/([a-zA-Z])/(.*)$ ]]; then
        pwd_emit="/${BASH_REMATCH[1]^^}:/${BASH_REMATCH[2]}"
    elif [[ "$pwd_emit" =~ ^/([a-zA-Z])$ ]]; then
        pwd_emit="/${BASH_REMATCH[1]^^}:"
    elif command -v cygpath >/dev/null 2>&1; then
        pwd_emit=$(cygpath -w "$PWD" 2>/dev/null || printf '%s' "$PWD")
        pwd_emit=${pwd_emit//\\//}
        [[ "$pwd_emit" == [A-Za-z]:* ]] && pwd_emit="/${pwd_emit}"
    fi
    printf '\033]7;file://%s%s\033\\' "${HOSTNAME:-localhost}" "$pwd_emit"
}
PROMPT_COMMAND="__tasty_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# === end tasty built-in ===
"#;

/// `~/.tasty/bashrc.user`의 초기 시드. 사용자가 자유롭게 수정/리셋할 수 있는 기본값.
pub const INITIAL_USER_BASHRC: &str = r#"# Tasty user bashrc — edit freely.
# Tasty prepends a built-in block (OSC 7 emission etc.) automatically; no need
# to include those here.

# Prompt
PS1='\[\033[32m\]\u@\h\[\033[0m\] \[\033[33m\]\w\[\033[0m\]\n\$ '

# Common aliases
alias ls='ls --color=auto'
alias ll='ls -la'
alias grep='grep --color=auto'
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub shell: String,
    /// Shell startup mode: "default" or "tasty". Windows 전용 — OSC7/MSYS PATH
    /// 빌트인을 prepend 할지 여부를 결정한다. 비-Windows 에서는 셸 모드 자체가
    /// 의미가 없어 노출하지 않는다.
    #[cfg(windows)]
    pub shell_mode: String,
    pub startup_command: String,
    pub language: String,
    /// Number of scrollback lines to keep.
    pub scrollback_lines: usize,
    /// Show confirmation dialog when closing a surface with a running process.
    pub confirm_close_running: bool,
    /// Enable click-to-move-cursor: clicking on the editable region moves the
    /// shell cursor to that position.
    pub click_to_move_cursor: bool,
    /// When creating a new pane/surface/workspace, inherit the working directory
    /// from the source surface.
    pub inherit_cwd: bool,
    /// Behavior when closing the last window: "ask", "minimize", "quit".
    pub close_behavior: String,
    /// Save and restore layout (workspaces, panes, tabs) on restart.
    pub restore_layout: bool,
    /// Restore terminal scrollback content on restart (requires `restore_layout`).
    pub restore_terminal_content: bool,
    /// 터미널 내 링크 클릭 시 요구되는 수식키. "ctrl" | "alt" | "none".
    /// "none"이면 평범한 클릭으로 링크가 열리므로 텍스트 선택과 구분되지 않는 점에 유의.
    pub link_click_modifier: String,
}

/// 파싱된 링크 클릭 수식키.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkModifier {
    Ctrl,
    Alt,
    None,
}

impl LinkModifier {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Self::Ctrl,
            "alt" | "option" => Self::Alt,
            "none" | "off" | "" => Self::None,
            _ => Self::Ctrl,
        }
    }

    /// 현재 modifier 상태가 링크 클릭 트리거 조건을 만족하는지.
    /// 호출자가 winit/macos 등 GUI 레이어에서 추출한 boolean을 그대로 넘긴다.
    pub fn matches(&self, ctrl: bool, alt: bool, super_key: bool) -> bool {
        match self {
            Self::Ctrl => ctrl && !alt && !super_key,
            Self::Alt => alt && !ctrl && !super_key,
            Self::None => true,
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        let shell = Self::detect_shell();
        Self {
            shell,
            #[cfg(windows)]
            shell_mode: "default".to_string(),
            startup_command: String::new(),
            language: "en".to_string(),
            scrollback_lines: 10000,
            confirm_close_running: true,
            click_to_move_cursor: true,
            inherit_cwd: true,
            close_behavior: "ask".to_string(),
            restore_layout: true,
            restore_terminal_content: true,
            link_click_modifier: "ctrl".to_string(),
        }
    }
}

impl GeneralSettings {
    /// Detect bash (Git Bash on Windows, system bash on Unix).
    /// Returns the path if found, or an empty string if not.
    pub fn detect_shell() -> String {
        Self::detect_bash().unwrap_or_default()
    }

    /// Try to find bash. On Windows this means Git Bash.
    pub fn detect_bash() -> Option<String> {
        #[cfg(windows)]
        {
            let candidates = [
                std::env::var("ProgramFiles")
                    .map(|p| format!("{}/Git/bin/bash.exe", p))
                    .unwrap_or_default(),
                "C:/Program Files/Git/bin/bash.exe".to_string(),
                "C:/Program Files (x86)/Git/bin/bash.exe".to_string(),
            ];
            for path in &candidates {
                if !path.is_empty() && std::path::Path::new(path).exists() {
                    return Some(path.clone());
                }
            }
            None
        }
        #[cfg(not(windows))]
        {
            // 1. Check /etc/passwd for the user's login shell (most authoritative after chsh)
            if let Some(login_shell) = Self::login_shell_from_passwd()
                && std::path::Path::new(&login_shell).exists()
            {
                return Some(login_shell);
            }
            // 2. Fall back to $SHELL env var
            if let Ok(shell) = std::env::var("SHELL")
                && std::path::Path::new(&shell).exists()
            {
                return Some(shell);
            }
            // 3. Common paths
            for path in &["/bin/zsh", "/bin/bash", "/bin/sh"] {
                if std::path::Path::new(path).exists() {
                    return Some(path.to_string());
                }
            }
            None
        }
    }

    /// Read the user's login shell from /etc/passwd.
    #[cfg(not(windows))]
    fn login_shell_from_passwd() -> Option<String> {
        use std::io::BufRead;
        // SAFETY: getuid는 POSIX thread-safe 시스템콜, errno도 안 set한다.
        let uid = unsafe { libc::getuid() };
        let file = std::fs::File::open("/etc/passwd").ok()?;
        for line in std::io::BufReader::new(file).lines() {
            let line = line.ok()?;
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 7
                && let Ok(entry_uid) = fields[2].parse::<u32>()
                && entry_uid == uid
            {
                return Some(fields[6].to_string());
            }
        }
        None
    }

    /// Returns true if the configured shell path points to an existing bash-compatible shell.
    /// On Windows, the filename must contain "bash" (e.g. bash.exe).
    /// On Unix, any existing shell is accepted (zsh, bash, fish, sh).
    pub fn is_shell_valid(&self) -> bool {
        if self.shell.is_empty() {
            return false;
        }
        let path = std::path::Path::new(&self.shell);
        if !path.exists() {
            return false;
        }
        #[cfg(windows)]
        {
            // On Windows, only accept bash-compatible shells
            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            filename.contains("bash") || filename.contains("zsh")
        }
        #[cfg(not(windows))]
        {
            true
        }
    }

    /// Resolve effective shell arguments based on shell_mode.
    ///
    /// 시그니처는 OS 공통 — 호출자가 cfg 분기를 하지 않도록 유지한다. 비-Windows
    /// 에서는 셸 모드 개념 자체가 없어 항상 빈 vec 을 반환한다.
    pub fn effective_shell_args(&self) -> Vec<String> {
        // tasty 빌트인(OSC7 cwd emit, MSYS PATH 주입)은 **Windows 전용 가치**다.
        //
        // - Windows: 다른 프로세스의 cwd 를 조회하는 표준 API 가 없어 새 탭 cwd
        //   상속을 OSC7 에 의존한다. 그래서 bash 를 `--rcfile <bashrc>` 로 띄워
        //   빌트인을 startup 에 *직접* source 시킨다. (예전처럼 `--norc` 로 띄운 뒤
        //   `. <bashrc>` 를 PTY 입력으로 보내면 화면 echo / 복원 시 claude 입력창
        //   오염 문제가 있었다. forward slash 경로여야 Git Bash 가 올바로 읽는다.)
        //
        // - 비-Windows: zsh 는 `--rcfile`/`--norc` 를 *모르고* (옵션 거부 → 셸
        //   즉사), bash login 셸은 `--rcfile` 을 *무시*한다. 게다가 새 탭 cwd 는
        //   `get_cwd_of_pid`(macOS `proc_pidinfo` / Linux `/proc/<pid>/cwd`)로
        //   이미 상속되므로 OSC7 이 불필요하다. 셸 모드 UI 자체를 노출하지 않고
        //   사용자 셸을 그대로 띄운다.
        #[cfg(windows)]
        {
            if self.shell_mode.as_str() == "tasty" {
                // 파생 bashrc가 없으면 현재 user 파일 내용으로 재생성.
                ensure_compiled_bashrc();
                let rcfile = tasty_bashrc_path().replace('\\', "/");
                return vec!["--rcfile".to_string(), rcfile];
            }
            Vec::new()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }
}

/// Path to Tasty's compiled bashrc (builtin + user).
pub fn tasty_bashrc_path() -> String {
    tasty_dir().join("bashrc").to_string_lossy().to_string()
}

/// Path to the user-editable bashrc fragment.
pub fn tasty_bashrc_user_path() -> String {
    tasty_dir()
        .join("bashrc.user")
        .to_string_lossy()
        .to_string()
}

fn tasty_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    home.join(".tasty")
}

/// 사용자 편집 파일을 로드. 파일이 없으면 `INITIAL_USER_BASHRC`를 반환(파일은 생성하지 않음).
pub fn load_user_bashrc() -> String {
    let path = tasty_bashrc_user_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => INITIAL_USER_BASHRC.to_string(),
    }
}

/// 사용자 편집 내용을 디스크에 쓰고, 파생 bashrc(builtin + user)를 재생성한다.
pub fn save_user_bashrc(user_content: &str) {
    let user_path = tasty_bashrc_user_path();
    let compiled_path = tasty_bashrc_path();
    let p = std::path::Path::new(&user_path);
    if let Some(parent) = p.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("create_dir_all for bashrc failed: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&user_path, user_content) {
        tracing::warn!("write bashrc.user failed: {e}");
        return;
    }
    let compiled = format!("{}\n{}", BUILTIN_BASHRC, user_content);
    if let Err(e) = std::fs::write(&compiled_path, compiled) {
        tracing::warn!("write compiled bashrc failed: {e}");
    }
}

/// tasty 모드 진입 시 파생 bashrc가 없으면 현재 user 파일(또는 기본값)로 생성.
/// tasty 빌트인은 Windows 전용이라 호출자(`effective_shell_args`의 windows 분기)도
/// Windows 에만 존재한다. 비-Windows 에서 dead_code 경고가 나지 않도록 함께 격리한다.
#[cfg(windows)]
fn ensure_compiled_bashrc() {
    let compiled_path = tasty_bashrc_path();
    if std::path::Path::new(&compiled_path).exists() {
        return;
    }
    let user = load_user_bashrc();
    save_user_bashrc(&user);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn settings_with_mode(mode: &str) -> GeneralSettings {
        GeneralSettings {
            shell_mode: mode.to_string(),
            ..GeneralSettings::default()
        }
    }

    #[cfg(windows)]
    #[test]
    fn default_mode_has_no_args() {
        assert!(
            settings_with_mode("default")
                .effective_shell_args()
                .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn unknown_mode_falls_back_to_no_args() {
        assert!(settings_with_mode("fast").effective_shell_args().is_empty());
    }

    // tasty 모드: Windows 는 `--rcfile <path>` 로 빌트인을 source 한다(S-2 픽스 보존).
    #[cfg(windows)]
    #[test]
    fn tasty_mode_windows_uses_rcfile() {
        let args = settings_with_mode("tasty").effective_shell_args();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "--rcfile");
    }

    // 비-Windows: 셸 모드 필드 자체가 없으므로 effective_shell_args 는 항상 빈 vec.
    #[cfg(not(windows))]
    #[test]
    fn effective_shell_args_empty_on_unix() {
        let s = GeneralSettings::default();
        assert!(s.effective_shell_args().is_empty());
    }
}
