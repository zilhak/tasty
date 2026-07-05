use serde::{Deserialize, Serialize};

/// 빌트인 bashrc 의 *전반부* — LANG/LC_ALL, MSYS PATH, `__tasty_osc7` 함수 정의.
/// 모드와 무관하게 합성 rc 의 *맨 앞* 에 prepend 된다. 사용자 rc 가 이후에 source 된다.
pub const BUILTIN_BASHRC_PRE: &str = r#"# === tasty built-in (auto-generated, do not edit) ===
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
"#;

/// 빌트인 bashrc 의 *후반부* — `PROMPT_COMMAND` 설정. 합성 rc 의 *맨 뒤* 에 append
/// 되어 사용자 rc 가 PROMPT_COMMAND 를 덮어쓰더라도 `__tasty_osc7` 이 마지막에 prepend.
pub const BUILTIN_BASHRC_PROMPT: &str = r#"PROMPT_COMMAND="__tasty_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

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
    /// Restore surface content on restart (requires `restore_layout`). 현재 보존되는
    /// "내용"은 터미널 scrollback 이며, 옵션명만 surface 일반으로 넓힌 것이다(메커니즘 동일).
    /// `alias`: 구버전 settings.toml 의 `restore_terminal_content` 키를 계속 읽는다.
    #[serde(alias = "restore_terminal_content")]
    pub restore_surface_content: bool,
    /// 터미널 내 링크 클릭 시 요구되는 수식키. "ctrl" | "alt" | "none".
    /// "none"이면 평범한 클릭으로 링크가 열리므로 텍스트 선택과 구분되지 않는 점에 유의.
    pub link_click_modifier: String,
    /// Allow programs in the terminal to read the system clipboard via OSC 52
    /// (`OSC 52 ; c ; ? ST`). Off by default: clipboard read lets any program —
    /// including untrusted remote/SSH processes — silently exfiltrate clipboard
    /// contents (passwords, tokens). When off, read queries get no reply.
    pub allow_clipboard_read: bool,
    /// Render DECSCNM (DEC private mode 5, "reverse screen"): when a program
    /// sends `\e[?5h`, the whole viewport's default fg/bg are swapped. Some
    /// setups abuse this as a visible bell (readline `bell-style visible` emits
    /// the terminfo `flash` = a DECSCNM toggle), producing a jarring full-screen
    /// flash. When this is off, the mode flag is still tracked (queries answer
    /// correctly) but the renderer does NOT apply the swap, so the flash is
    /// suppressed. On by default (spec-compliant). See `screen_reverse()` on the
    /// terminal and the render gate in `render_terminals`.
    pub reverse_screen_enabled: bool,
    /// Show a notification (and play the notification sound, subject to the
    /// global notification gates) when the terminal receives a BEL (`\a`). When
    /// off, BEL no longer raises the "Bell" toast — but user-registered `bell`
    /// hooks STILL fire, since a hook is explicit automation the user opted into,
    /// not a passive reaction. On by default. Gated in `cascade_terminal_bell_ring`
    /// on top of the global `notification.enabled`.
    pub bell_notification: bool,
    /// 마우스 트래킹 앱(vim/htop 등) 위에서 처음 좌/우 클릭할 때, 마우스가 앱에 캡처
    /// 중이며 텍스트 선택은 Shift+드래그, tasty 메뉴는 Shift+우클릭으로 띄울 수 있음을
    /// 안내하는 toast 를 트래킹 세션당 1회 표시한다(발견성, ADR-0022 ②). off 면 안내하지
    /// 않는다. `alias`: 구버전 settings.toml 의 `right_click_capture_hint` 키를 계속 읽는다.
    #[serde(alias = "right_click_capture_hint")]
    pub mouse_capture_hint: bool,
    /// 마우스 캡처 비활성화 블랙리스트 — foreground 프로세스 이름 패턴 목록.
    /// 여기에 매칭되는 TUI 가 surface 의 foreground 일 때, 그 surface 에서는 클릭/
    /// 드래그/버튼 캡처를 끄고(좌클릭=선택, 우클릭=tasty 메뉴 등 로컬 처리) 앱에
    /// 버튼 보고를 보내지 않는다. **휠은 예외로 계속 앱에 보고된다.** 패턴은 `.exe`
    /// 제거·소문자화 후 substring 매칭(또는 `*` 와일드카드)이며, 빈 목록(기본값)이면
    /// 어떤 surface 도 비활성화하지 않는다.
    pub mouse_capture_blacklist: Vec<String>,
    /// 워크스페이스 카테고리(사이드바 폴더) 계층을 활성화한다. off(기본)면 사이드바·
    /// 단축키·영속이 현행 평면 동작과 동일하다. on 으로 켜면 normal 외 사용자
    /// 카테고리를 만들 수 있고, off 로 끄면 모든 워크스페이스를 normal 로 귀속한다.
    pub workspace_categories_enabled: bool,
    /// Explorer 의 마지막으로 선택한 view mode ("grid" | "list" | "detail"). 사용자가
    /// 툴바 segmented 로 형태를 바꾸면 여기에 기록되고, 새로 생성되는 explorer
    /// surface·내부 탭의 기본 표시 형태로 쓰인다. 알 수 없는 값은
    /// `ExplorerViewMode::from_str` 이 detail 로 fallback (normalize 도 detail 로 교정).
    pub explorer_view_mode: String,
    /// macOS 전용 — Option 키를 Meta(Alt-prefix) 키로 해석한다. on 이면 Option+문자가
    /// 특수문자(compose) 대신 `ESC` + base 문자 시퀀스로 PTY 에 전달돼 readline/Emacs/
    /// vim 의 Meta 바인딩(`Alt+f`/`Alt+b` 등)이 동작한다. 기본 off — 기존 Option=특수문자
    /// 동작을 보존한다(iTerm2 도 기본 off). 다른 OS 에는 Option 키가 없어 노출하지 않는다.
    #[cfg(target_os = "macos")]
    pub option_as_meta: bool,
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
            restore_surface_content: true,
            link_click_modifier: "ctrl".to_string(),
            allow_clipboard_read: false,
            reverse_screen_enabled: true,
            bell_notification: true,
            mouse_capture_hint: true,
            mouse_capture_blacklist: Vec::new(),
            workspace_categories_enabled: false,
            explorer_view_mode: "detail".to_string(),
            #[cfg(target_os = "macos")]
            option_as_meta: false,
        }
    }
}

/// `*` 와일드카드만 지원하는 소형 glob 매칭 (정규식/`?` 미지원). 패턴·대상 모두
/// 호출 전 소문자/`.exe` 정규화돼 있다고 가정한다. `*` 는 0개 이상의 임의 문자에
/// 대응하며, 앞/뒤/중간 어디에 와도 된다.
fn glob_match(pattern: &str, text: &str) -> bool {
    // 패턴을 `*` 로 분할한 리터럴 조각들을 순서대로 text 에서 소비한다.
    let parts: Vec<&str> = pattern.split('*').collect();
    let starts_wild = pattern.starts_with('*');
    let ends_wild = pattern.ends_with('*');

    let mut cursor = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let is_first = i == 0;
        let is_last = i == parts.len() - 1;
        if is_first && !starts_wild {
            // 첫 리터럴은 text 시작에 고정.
            if !text[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if is_last && !ends_wild {
            // 마지막 리터럴은 text 끝에 고정.
            if !text[cursor..].ends_with(part) {
                return false;
            }
            cursor = text.len();
        } else {
            // 중간 리터럴: 다음 출현 위치를 찾아 소비.
            match text[cursor..].find(part) {
                Some(off) => cursor += off + part.len(),
                None => return false,
            }
        }
    }
    true
}

impl GeneralSettings {
    /// foreground 프로세스 이름이 마우스 캡처 블랙리스트에 걸리면 true → 그 surface
    /// 의 클릭/드래그 캡처를 끈다(휠은 별도로 계속 보고됨, 결정 ②).
    ///
    /// 매칭: 인자 이름을 소문자화하고 `.exe` 접미사를 제거한 stem 을, 각 블랙리스트
    /// 패턴(소문자·trim, 빈 줄은 무시)에 대해 — 패턴에 `*` 가 있으면 glob, 없으면
    /// substring — 으로 비교한다(결정 ①). 빈 블랙리스트면 항상 false.
    pub fn mouse_capture_disabled_for(&self, fg_name: &str) -> bool {
        if self.mouse_capture_blacklist.is_empty() {
            return false;
        }
        let lower = fg_name.to_ascii_lowercase();
        let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
        self.mouse_capture_blacklist.iter().any(|pat| {
            let pat = pat.trim().to_ascii_lowercase();
            if pat.is_empty() {
                return false;
            }
            if pat.contains('*') {
                glob_match(&pat, stem)
            } else {
                stem.contains(&pat)
            }
        })
    }

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
            // OSC7 빌트인은 셸 모드와 무관하게 강제 주입한다 — Windows 는 다른 프로세스
            // cwd 조회 API 가 없어 새 탭 cwd 상속이 OSC7 emit 에만 의존하기 때문.
            // 모드는 "어떤 사용자 rc 를 source 하느냐" 만 결정한다:
            //   - tasty: ~/.tasty/bashrc          (BUILTIN_PRE + ~/.tasty/bashrc.user + BUILTIN_PROMPT)
            //   - default (or unknown): ~/.tasty/bashrc.default (BUILTIN_PRE + source ~/.bashrc + BUILTIN_PROMPT)
            ensure_compiled_bashrc();
            let rcfile = match self.shell_mode.as_str() {
                "tasty" => tasty_bashrc_path().replace('\\', "/"),
                _ => tasty_bashrc_default_path().replace('\\', "/"),
            };
            vec!["--rcfile".to_string(), rcfile]
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }
}

/// Path to Tasty's compiled bashrc for **tasty 모드** (BUILTIN_PRE + ~/.tasty/bashrc.user + BUILTIN_PROMPT).
pub fn tasty_bashrc_path() -> String {
    tasty_dir().join("bashrc").to_string_lossy().to_string()
}

/// Path to Tasty's compiled bashrc for **default 모드** (BUILTIN_PRE + `source ~/.bashrc` + BUILTIN_PROMPT).
pub fn tasty_bashrc_default_path() -> String {
    tasty_dir()
        .join("bashrc.default")
        .to_string_lossy()
        .to_string()
}

/// Path to the user-editable bashrc fragment.
pub fn tasty_bashrc_user_path() -> String {
    tasty_dir()
        .join("bashrc.user")
        .to_string_lossy()
        .to_string()
}

/// default 모드용 합성 rc 본문. BUILTIN PRE → 사용자 `~/.bashrc` source → BUILTIN PROMPT.
/// 사용자 rc 가 PROMPT_COMMAND 를 덮어써도 마지막에 `__tasty_osc7` 이 prepend.
pub fn compose_default_mode_bashrc() -> String {
    format!(
        "{}\n[ -f ~/.bashrc ] && source ~/.bashrc\n{}",
        BUILTIN_BASHRC_PRE, BUILTIN_BASHRC_PROMPT,
    )
}

/// tasty 모드용 합성 rc 본문. BUILTIN PRE → tasty user rc → BUILTIN PROMPT.
/// 사용자 영역 (`~/.tasty/bashrc.user`) 이 PROMPT_COMMAND 를 덮어써도 마지막에 prepend.
pub fn compose_tasty_mode_bashrc(user_content: &str) -> String {
    format!(
        "{}\n{}\n{}",
        BUILTIN_BASHRC_PRE, user_content, BUILTIN_BASHRC_PROMPT,
    )
}

fn tasty_dir() -> std::path::PathBuf {
    // 루트는 SoT 인 tasty_home() 으로 통일 (debug/release 격리 + TASTY_HOME override).
    // 홈 해석 실패 시 기존 동작과 동일하게 빈 경로로 폴백.
    tasty_utils::path::tasty_home().unwrap_or_default()
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
    let compiled = compose_tasty_mode_bashrc(user_content);
    if let Err(e) = std::fs::write(&compiled_path, compiled) {
        tracing::warn!("write compiled bashrc failed: {e}");
    }
}

/// Windows 셸 시작 시 두 합성 rc (`~/.tasty/bashrc`, `~/.tasty/bashrc.default`) 가 존재하도록
/// 보장한다. 빠진 파일만 채우며, 이미 있는 파일은 건드리지 않는다 (사용자가 tastyrc 를
/// 저장한 결과를 `save_user_bashrc` 가 별도로 재생성).
#[cfg(windows)]
fn ensure_compiled_bashrc() {
    // tasty 모드 합성 rc.
    let tasty_path = tasty_bashrc_path();
    if !std::path::Path::new(&tasty_path).exists() {
        let user = load_user_bashrc();
        save_user_bashrc(&user);
    }
    // default 모드 합성 rc.
    let default_path = tasty_bashrc_default_path();
    if !std::path::Path::new(&default_path).exists() {
        let compiled = compose_default_mode_bashrc();
        if let Some(parent) = std::path::Path::new(&default_path).parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("create_dir_all for bashrc.default failed: {e}");
            return;
        }
        if let Err(e) = std::fs::write(&default_path, compiled) {
            tracing::warn!("write bashrc.default failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_blacklist(patterns: &[&str]) -> GeneralSettings {
        GeneralSettings {
            mouse_capture_blacklist: patterns.iter().map(|s| s.to_string()).collect(),
            ..GeneralSettings::default()
        }
    }

    #[test]
    fn mouse_capture_substring_hit_and_miss() {
        let g = settings_with_blacklist(&["htop"]);
        assert!(g.mouse_capture_disabled_for("htop"));
        assert!(g.mouse_capture_disabled_for("htop-vim")); // substring
        assert!(!g.mouse_capture_disabled_for("vim"));
    }

    #[test]
    fn mouse_capture_case_insensitive() {
        let g = settings_with_blacklist(&["HTOP"]);
        assert!(g.mouse_capture_disabled_for("htop"));
        assert!(g.mouse_capture_disabled_for("HTOP"));
    }

    #[test]
    fn mouse_capture_strips_exe_suffix() {
        let g = settings_with_blacklist(&["htop"]);
        assert!(g.mouse_capture_disabled_for("htop.exe"));
        assert!(g.mouse_capture_disabled_for("HTOP.EXE"));
    }

    #[test]
    fn mouse_capture_empty_blacklist_never_matches() {
        let g = settings_with_blacklist(&[]);
        assert!(!g.mouse_capture_disabled_for("htop"));
    }

    #[test]
    fn mouse_capture_blank_pattern_ignored() {
        // 공백/빈 줄 패턴이 전체 매칭으로 새지 않아야 한다.
        let g = settings_with_blacklist(&["   ", ""]);
        assert!(!g.mouse_capture_disabled_for("htop"));
        assert!(!g.mouse_capture_disabled_for("anything"));
    }

    #[test]
    fn mouse_capture_glob_wildcard() {
        let g = settings_with_blacklist(&["ht*"]);
        assert!(g.mouse_capture_disabled_for("htop"));
        assert!(!g.mouse_capture_disabled_for("vim"));

        let g = settings_with_blacklist(&["*top"]);
        assert!(g.mouse_capture_disabled_for("htop"));
        assert!(!g.mouse_capture_disabled_for("htopx"));

        let g = settings_with_blacklist(&["h*p"]);
        assert!(g.mouse_capture_disabled_for("htop"));
        assert!(!g.mouse_capture_disabled_for("hat"));
    }

    #[test]
    fn mouse_capture_blacklist_defaults_empty() {
        let g = GeneralSettings::default();
        assert!(g.mouse_capture_blacklist.is_empty());
    }

    #[test]
    fn legacy_restore_terminal_content_key_still_loads() {
        // 구버전 키(`restore_terminal_content`)로 저장된 값이 새 필드로 매핑되어야 한다
        // (serde alias). 구버전 settings 호환. (settings 는 toml 직렬화.)
        let g: GeneralSettings = toml::from_str("restore_terminal_content = false").unwrap();
        assert!(!g.restore_surface_content);
    }

    #[test]
    fn new_restore_surface_content_key_loads() {
        let g: GeneralSettings = toml::from_str("restore_surface_content = false").unwrap();
        assert!(!g.restore_surface_content);
    }

    #[test]
    fn reverse_screen_enabled_defaults_true() {
        // 기본값은 현행 스펙 유지(DECSCNM 정상 렌더).
        let g = GeneralSettings::default();
        assert!(g.reverse_screen_enabled);
    }

    #[test]
    fn reverse_screen_enabled_missing_key_uses_default() {
        // 구 config.toml 마이그레이션 안전: 키가 없으면 기본 true.
        let g: GeneralSettings = toml::from_str("scrollback_lines = 5000").unwrap();
        assert!(g.reverse_screen_enabled);
    }

    #[test]
    fn reverse_screen_enabled_round_trips() {
        let g: GeneralSettings = toml::from_str("reverse_screen_enabled = false").unwrap();
        assert!(!g.reverse_screen_enabled);
        let out = toml::to_string(&g).unwrap();
        let g2: GeneralSettings = toml::from_str(&out).unwrap();
        assert!(!g2.reverse_screen_enabled);
    }

    #[test]
    fn bell_notification_defaults_true() {
        let g = GeneralSettings::default();
        assert!(g.bell_notification);
    }

    #[test]
    fn bell_notification_missing_key_uses_default() {
        // 구 config.toml 마이그레이션 안전: 키가 없으면 기본 true.
        let g: GeneralSettings = toml::from_str("scrollback_lines = 5000").unwrap();
        assert!(g.bell_notification);
    }

    #[test]
    fn bell_notification_round_trips() {
        let g: GeneralSettings = toml::from_str("bell_notification = false").unwrap();
        assert!(!g.bell_notification);
        let out = toml::to_string(&g).unwrap();
        let g2: GeneralSettings = toml::from_str(&out).unwrap();
        assert!(!g2.bell_notification);
    }

    #[test]
    fn explorer_view_mode_defaults_to_detail() {
        let g = GeneralSettings::default();
        assert_eq!(g.explorer_view_mode, "detail");
    }

    #[test]
    fn explorer_view_mode_round_trips() {
        let g: GeneralSettings = toml::from_str("explorer_view_mode = \"list\"").unwrap();
        assert_eq!(g.explorer_view_mode, "list");
        let out = toml::to_string(&g).unwrap();
        let g2: GeneralSettings = toml::from_str(&out).unwrap();
        assert_eq!(g2.explorer_view_mode, "list");
    }

    #[test]
    fn explorer_view_mode_missing_key_uses_default() {
        // 구 config.toml 마이그레이션 안전: 키가 없으면 기본값 detail.
        let g: GeneralSettings = toml::from_str("scrollback_lines = 5000").unwrap();
        assert_eq!(g.explorer_view_mode, "detail");
    }

    #[cfg(windows)]
    fn settings_with_mode(mode: &str) -> GeneralSettings {
        GeneralSettings {
            shell_mode: mode.to_string(),
            ..GeneralSettings::default()
        }
    }

    // default 모드: Windows 는 `--rcfile ~/.tasty/bashrc.default` 로 OSC7 강제 주입.
    #[cfg(windows)]
    #[test]
    fn default_mode_uses_default_rc_file() {
        let args = settings_with_mode("default").effective_shell_args();
        assert!(args.iter().any(|a| a == "--rcfile"));
        assert!(
            args.iter()
                .any(|a| a.contains(".tasty") && a.ends_with("bashrc.default"))
        );
    }

    // unknown 모드도 default 와 동일하게 fallback.
    #[cfg(windows)]
    #[test]
    fn unknown_mode_falls_back_to_default_rc_file() {
        let args = settings_with_mode("fast").effective_shell_args();
        assert!(args.iter().any(|a| a == "--rcfile"));
        assert!(
            args.iter()
                .any(|a| a.contains(".tasty") && a.ends_with("bashrc.default"))
        );
    }

    // tasty 모드: Windows 는 `--rcfile ~/.tasty/bashrc` 로 빌트인을 source 한다(S-2 픽스 보존).
    #[cfg(windows)]
    #[test]
    fn tasty_mode_uses_tasty_rc_file() {
        let args = settings_with_mode("tasty").effective_shell_args();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "--rcfile");
        assert!(
            args.iter()
                .any(|a| a.contains(".tasty") && a.ends_with("bashrc"))
        );
        assert!(args.iter().all(|a| !a.ends_with("bashrc.default")));
    }

    // default 모드 합성 rc 는 BUILTIN PRE / 사용자 ~/.bashrc source / BUILTIN PROMPT 셋 다 포함.
    #[test]
    fn default_mode_compiled_rc_sources_user_bashrc() {
        let compiled = compose_default_mode_bashrc();
        assert!(compiled.contains("__tasty_osc7")); // BUILTIN PRE 적용
        assert!(compiled.contains("source ~/.bashrc")); // 사용자 rc 호출
        assert!(compiled.contains("PROMPT_COMMAND=")); // BUILTIN PROMPT 적용
    }

    // tasty 모드 합성 rc 는 BUILTIN PRE / 사용자 본문 / BUILTIN PROMPT 셋 다 포함.
    #[test]
    fn tasty_mode_compiled_rc_wraps_user_content() {
        let compiled = compose_tasty_mode_bashrc("alias hi='echo hi'\n");
        assert!(compiled.contains("__tasty_osc7"));
        assert!(compiled.contains("alias hi='echo hi'"));
        assert!(compiled.contains("PROMPT_COMMAND="));
        // PROMPT_COMMAND 는 사용자 본문 *뒤* 에 와야 한다.
        let user_pos = compiled.find("alias hi").unwrap();
        let prompt_pos = compiled.find("PROMPT_COMMAND=").unwrap();
        assert!(prompt_pos > user_pos);
    }

    // 비-Windows: 셸 모드 필드 자체가 없으므로 effective_shell_args 는 항상 빈 vec.
    #[cfg(not(windows))]
    #[test]
    fn effective_shell_args_empty_on_unix() {
        let s = GeneralSettings::default();
        assert!(s.effective_shell_args().is_empty());
    }
}
