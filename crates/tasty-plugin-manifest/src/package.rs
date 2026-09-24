//! 매니페스트와 설치 디렉터리.

use std::path::{Path, PathBuf};

use super::types::{Entry, Manifest};

#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl PluginPackage {
    /// 설치 디렉터리의 실행 파일을 우선한다. Windows에서는 .exe를 붙여 다시 찾는다.
    /// 그래야 PATH의 다른 빌드보다 이 설치본을 실행할 수 있다.
    /// 절대 경로는 그대로 쓰고, 설치본을 찾지 못하면 원래 명령을 반환한다.
    pub fn entry_command_path(&self) -> PathBuf {
        match &self.manifest.entry {
            Entry::Process { command, .. } => {
                let p = Path::new(command);
                if p.is_absolute() {
                    return p.to_path_buf();
                }
                let candidate = self.dir.join(command);
                if candidate.exists() {
                    return candidate;
                }
                #[cfg(windows)]
                if p.extension().is_none() {
                    let exe = self.dir.join(format!("{command}.exe"));
                    if exe.exists() {
                        return exe;
                    }
                }
                p.to_path_buf()
            }
        }
    }

    pub fn entry_args(&self) -> Vec<String> {
        match &self.manifest.entry {
            Entry::Process { args, .. } => args.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실행 명령 외에는 최소 필드만 채운 시험용 매니페스트.
    fn pkg(dir: PathBuf, command: &str) -> PluginPackage {
        let toml_str = format!(
            "manifest_version = 1\n\
             id = \"com.example.foo\"\n\
             name = \"Foo\"\n\
             version = \"1.0.0\"\n\
             api_version = \"1\"\n\
             [entry]\n\
             type = \"process\"\n\
             command = '{command}'\n"
        );
        let manifest: Manifest = toml::from_str(&toml_str).expect("manifest parse");
        PluginPackage { dir, manifest }
    }

    /// Windows에서는 확장자를 생략해도 설치 디렉터리의 .exe를 찾아야 한다.
    #[cfg(windows)]
    #[test]
    fn windows_resolves_installed_exe_not_bare_name() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("tasty-plugin-foo.exe");
        std::fs::write(&exe, b"stub").unwrap();
        let p = pkg(tmp.path().to_path_buf(), "tasty-plugin-foo");
        assert_eq!(p.entry_command_path(), exe);
    }

    /// Unix: 설치본은 확장자 없는 `foo` — command 와 정확히 일치해 절대경로로 고정.
    #[cfg(not(windows))]
    #[test]
    fn unix_resolves_installed_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("tasty-plugin-foo");
        std::fs::write(&bin, b"stub").unwrap();
        let p = pkg(tmp.path().to_path_buf(), "tasty-plugin-foo");
        assert_eq!(p.entry_command_path(), bin);
    }

    /// 설치 파일을 찾지 못하면 OS가 탐색할 수 있도록 원래 명령을 반환한다.
    #[test]
    fn missing_binary_falls_back_to_bare_command() {
        let tmp = tempfile::tempdir().unwrap();
        let p = pkg(tmp.path().to_path_buf(), "tasty-plugin-foo");
        assert_eq!(p.entry_command_path(), PathBuf::from("tasty-plugin-foo"));
    }

    /// 절대 경로 command 는 디렉토리와 무관하게 그대로 사용.
    #[test]
    fn absolute_command_is_returned_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let abs = "C:\\opt\\custom-plugin.exe";
        #[cfg(not(windows))]
        let abs = "/opt/custom-plugin";
        let p = pkg(tmp.path().to_path_buf(), abs);
        assert_eq!(p.entry_command_path(), PathBuf::from(abs));
    }
}
