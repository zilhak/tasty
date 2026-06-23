//! 매니페스트가 들어 있는 디렉터리 핸들 — 디렉터리 + 파싱된 매니페스트 묶음.

use std::path::{Path, PathBuf};

use super::types::{Entry, Manifest};

#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl PluginPackage {
    /// 실행할 entry binary의 경로. 매니페스트 디렉터리 기준 상대 경로면
    /// 디렉터리에 합쳐서 반환. 절대 경로 또는 PATH 의존이면 그대로.
    ///
    /// **프로필 격리의 핵심.** 매니페스트의 `command` 는 크로스 플랫폼이라 실행
    /// 확장자 없이 적힌다 (`tasty-plugin-foo`). Unix 설치본은 같은 이름이라 아래
    /// `candidate` 분기에서 바로 절대경로로 고정되지만, Windows 설치본은
    /// `foo.exe` 라 확장자 없는 join 이 실패한다. 그 경우 `.exe` 를 붙여 설치본을
    /// 재탐색한다 — 이 보정이 없으면 절대경로로 고정되지 못하고
    /// `Command::new("foo")` 가 PATH/cwd 탐색으로 빠져 엉뚱한 빌드 산출물
    /// (`target/<profile>/foo.exe`) 을 실행하게 되어 debug/release 프로세스
    /// 격리가 깨진다 (각 프로필은 자기 데이터루트의 설치본을 실행해야 한다).
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
                // Windows: 확장자를 붙여 설치본을 재탐색 (위 주석 참조).
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

    /// 최소 매니페스트로 PluginPackage 구성. `command` 만 검증 대상이므로 나머지
    /// 필드는 스키마 필수값만 채운다.
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

    /// Windows: 설치본은 `foo.exe` 인데 매니페스트 command 는 확장자가 없다.
    /// `.exe` 보정으로 설치 디렉토리의 절대경로를 반환해야 한다 — 이게 깨지면
    /// PATH 탐색으로 빠져 빌드 산출물(target/<profile>/foo.exe)을 실행하게 되어
    /// debug/release 프로세스 격리가 무너진다.
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

    /// 설치본 바이너리가 디렉토리에 없으면 (예: 누락) command 이름을 그대로 반환.
    /// 절대경로로 고정할 근거가 없으므로 OS 탐색에 맡기는 기존 동작 유지.
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
