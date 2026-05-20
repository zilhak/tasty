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
    pub fn entry_command_path(&self) -> PathBuf {
        match &self.manifest.entry {
            Entry::Process { command, .. } => {
                let p = Path::new(command);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    let candidate = self.dir.join(command);
                    if candidate.exists() {
                        candidate
                    } else {
                        p.to_path_buf()
                    }
                }
            }
        }
    }

    pub fn entry_args(&self) -> Vec<String> {
        match &self.manifest.entry {
            Entry::Process { args, .. } => args.clone(),
        }
    }
}
