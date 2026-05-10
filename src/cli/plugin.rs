//! Plugin CLI 명령 중 호스트 IPC를 거치지 않는 local-only 처리.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use anyhow::Result;

fn log_dir() -> Result<PathBuf> {
    tasty_core::paths::tasty_home()
        .map(|d| d.join("plugins-logs"))
        .ok_or_else(|| anyhow::anyhow!("could not determine tasty home directory"))
}

pub fn run_plugin_logs(plugin_id: &str, follow: bool) -> Result<()> {
    let path = log_dir()?.join(format!("{plugin_id}.log"));
    if !path.exists() {
        anyhow::bail!(
            "no log file for plugin '{}' at {}",
            plugin_id,
            path.display()
        );
    }
    if !follow {
        let s = std::fs::read_to_string(&path)?;
        print!("{s}");
        return Ok(());
    }
    let mut file = std::fs::File::open(&path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    print!("{buf}");
    let mut pos = file.metadata()?.len();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        file.seek(SeekFrom::Start(pos))?;
        let mut chunk = String::new();
        let n = file.read_to_string(&mut chunk)? as u64;
        if n > 0 {
            print!("{chunk}");
            pos += n;
        }
    }
}
