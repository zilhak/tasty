use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub shell: String,
    /// Shell startup mode: "default", "fast", or "custom".
    pub shell_mode: String,
    /// Extra arguments passed to the shell (used when shell_mode is "custom").
    pub shell_args: String,
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
}

impl Default for GeneralSettings {
    fn default() -> Self {
        let shell = Self::detect_shell();
        Self {
            shell,
            shell_mode: "default".to_string(),
            shell_args: String::new(),
            startup_command: String::new(),
            language: "en".to_string(),
            scrollback_lines: 10000,
            confirm_close_running: true,
            click_to_move_cursor: true,
            inherit_cwd: true,
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
            if let Some(login_shell) = Self::login_shell_from_passwd() {
                if std::path::Path::new(&login_shell).exists() {
                    return Some(login_shell);
                }
            }
            // 2. Fall back to $SHELL env var
            if let Ok(shell) = std::env::var("SHELL") {
                if std::path::Path::new(&shell).exists() {
                    return Some(shell);
                }
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
        let uid = unsafe { libc::getuid() };
        let file = std::fs::File::open("/etc/passwd").ok()?;
        for line in std::io::BufReader::new(file).lines() {
            let line = line.ok()?;
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 7 {
                if let Ok(entry_uid) = fields[2].parse::<u32>() {
                    if entry_uid == uid {
                        return Some(fields[6].to_string());
                    }
                }
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
            let filename = path.file_name()
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
    pub fn effective_shell_args(&self) -> Vec<String> {
        match self.shell_mode.as_str() {
            "fast" => {
                Self::ensure_tasty_bashrc(&Self::tasty_bashrc_path());
                vec![
                    "--norc".to_string(),
                    "--noprofile".to_string(),
                ]
            }
            "custom" => self
                .shell_args
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            _ => vec![], // "default"
        }
    }

    /// Returns a command to source the Tasty bashrc (sent to PTY after shell starts).
    /// Returns None if not in fast mode.
    pub fn fast_mode_init_command(&self) -> Option<String> {
        if self.shell_mode == "fast" {
            let rcfile = Self::tasty_bashrc_path();
            // Use printf to avoid issues with echo interpretation
            Some(format!(". '{}'\n", rcfile.replace('\\', "/")))
        } else {
            None
        }
    }

    /// Path to Tasty's lightweight bashrc.
    fn tasty_bashrc_path() -> String {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        home.join(".tasty").join("bashrc").to_string_lossy().to_string()
    }

    /// Create ~/.tasty/bashrc if it doesn't exist.
    fn ensure_tasty_bashrc(path: &str) {
        let p = std::path::Path::new(path);
        if p.exists() {
            return;
        }
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let contents = r#"# Tasty fast-start bashrc
# Minimal shell configuration for fast pane/tab creation.
# Edit this file to customize. Tasty will not overwrite it.

# UTF-8
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8

# Inherit Windows PATH
ORIGINAL_PATH="${ORIGINAL_PATH:-${PATH}}"
export PATH="/usr/local/bin:/usr/bin:/bin:${ORIGINAL_PATH}"

# Prompt
PS1='\[\033[32m\]\u@\h\[\033[0m\] \[\033[33m\]\w\[\033[0m\]\n\$ '

# Common aliases
alias ls='ls --color=auto'
alias ll='ls -la'
alias grep='grep --color=auto'
"#;
        let _ = std::fs::write(p, contents);
    }
}
