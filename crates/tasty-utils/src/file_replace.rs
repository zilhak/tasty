//! 사용자 설정 파일을 원자적으로 교체하고 Tasty writer끼리 읽기-변경-교체를 직렬화한다.
//!
//! - [`replace`]: 같은 디렉터리의 고유 임시 파일에 쓰고 대상 이름으로 바꾼다. 다른 프로세스는 옛
//!   내용이나 새 내용 중 하나만 본다. 실패하면 원본은 그대로이고 임시 파일은 지운다.
//!   교체 성공은 전원 장애에서의 내구성 보장이 아니다(`sync`를 켠 경우만 내용을 디스크에 내린다).
//! - 대상이 symlink면 링크를 따라가 실제 파일을 교체한다. 링크는 남는다. 끊긴 링크는 오류다.
//! - 기존 파일의 권한을 새 파일에 옮긴다. 소유자·ACL·확장 속성은 옮기지 않는다. 새 파일은
//!   `new_mode`(Unix) 또는 기본 권한으로 만든다.
//! - [`lock`]: `<파일>.lock` 옆 파일에 배타 잠금을 잡는다. 교체되는 대상 inode가 아니라 고정된 잠금
//!   파일을 쓰므로 교체 뒤에도 같은 잠금이다. 참여하지 않는 외부 편집기는 막지 못한다.
//! - [`Fingerprint`]: 길이와 내용 해시. 읽은 뒤 교체 직전까지 외부 변경을 확인할 때 쓴다. 마지막
//!   확인과 교체 사이의 변경(TOCTOU)까지 막지는 않는다.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// symlink를 따라가는 최대 단계.
const MAX_LINKS: usize = 40;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 교체 선택 사항.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// 대상이 없을 때 새 파일의 Unix 권한. None이면 umask를 따른 기본 권한이다. 임시 파일도 처음부터
    /// 이 권한으로 만든다(비밀 값이 기본 권한으로 잠시라도 노출되지 않게).
    pub new_mode: Option<u32>,
    /// 교체 전에 임시 파일 내용을 디스크에 내린다.
    pub sync: bool,
}

/// `path`를 `bytes`로 원자적으로 교체한다. 기본 선택 사항을 쓴다.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, bytes, Options::default())
}

/// `path`를 `bytes`로 원자적으로 교체한다.
pub fn replace_with(path: &Path, bytes: &[u8], options: Options) -> io::Result<()> {
    let target = resolve_link(path)?;
    let existing = match fs::metadata(&target) {
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let temp = temp_path(&target)?;
    let mut file = create_temp(
        &temp,
        existing.is_none().then_some(options.new_mode).flatten(),
    )?;
    let written = write_temp(&mut file, bytes, options.sync).and_then(|()| {
        drop(file);
        match existing {
            Some(permissions) => fs::set_permissions(&temp, permissions),
            None => Ok(()),
        }
    });
    let result = written.and_then(|()| fs::rename(&temp, &target));
    if result.is_err()
        && let Err(e) = fs::remove_file(&temp)
        && e.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(temp = %temp.display(), error = %e, "could not remove a temporary file");
    }
    result
}

fn write_temp(file: &mut File, bytes: &[u8], sync: bool) -> io::Result<()> {
    file.write_all(bytes)?;
    file.flush()?;
    if sync {
        file.sync_all()?;
    }
    Ok(())
}

/// 링크를 따라간 실제 대상 경로. 대상이 없으면(새 파일) 마지막 경로를 그대로 돌려준다.
/// 링크가 가리키는 파일이 없으면 오류다.
pub fn resolve_link(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound && current == path => {
                return Ok(current);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "{} is a link to {}, which does not exist",
                        path.display(),
                        current.display()
                    ),
                ));
            }
            Err(e) => return Err(e),
        };
        if !meta.file_type().is_symlink() {
            return Ok(current);
        }
        let link = fs::read_link(&current)?;
        current = match current.parent() {
            Some(parent) if link.is_relative() => parent.join(link),
            _ => link,
        };
    }
    Err(io::Error::other(format!(
        "{} has too many levels of links",
        path.display()
    )))
}

fn temp_path(target: &Path) -> io::Result<PathBuf> {
    let name = target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no file name", target.display()),
        )
    })?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut temp = std::ffi::OsString::from(".");
    temp.push(name);
    temp.push(format!(".{}.{seq}.tmp", std::process::id()));
    Ok(target.with_file_name(temp))
}

#[cfg(unix)]
fn create_temp(path: &Path, mode: Option<u32>) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if let Some(mode) = mode {
        options.mode(mode);
    }
    options.open(path)
}

/// Windows에는 Unix 권한 비트가 없다. 새 파일은 상위 디렉터리의 ACL을 받는다.
#[cfg(not(unix))]
fn create_temp(path: &Path, _mode: Option<u32>) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// `<path>.lock` 파일의 배타 잠금. 놓으면(drop) 풀린다.
#[derive(Debug)]
pub struct WriteLock {
    _file: File,
}

/// `path`의 쓰기 잠금을 잡는다. 다른 Tasty 프로세스가 쥐고 있으면 풀릴 때까지 기다린다.
/// 메인 루프에서 부르지 않는다.
pub fn lock(path: &Path) -> io::Result<WriteLock> {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(PathBuf::from(name))?;
    file.lock()?;
    Ok(WriteLock { _file: file })
}

/// 파일 내용의 관측값. 없는 파일도 하나의 관측값이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fingerprint {
    Missing,
    Content { len: u64, hash: u64 },
}

impl Fingerprint {
    pub fn of_bytes(bytes: &[u8]) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self::Content {
            len: bytes.len() as u64,
            hash: hasher.finish(),
        }
    }

    /// 지금 파일의 관측값. 링크는 따라간다.
    pub fn read(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(Self::of_bytes(&bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests;
