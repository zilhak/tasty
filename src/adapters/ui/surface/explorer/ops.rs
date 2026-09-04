//! Explorer 파일 조작 (T11): 붙여넣기(copy/move)·삭제·이름변경의 순수 fs 헬퍼.
//!
//! UI/state 비의존 — 호출부(redraw)가 결과(개수/에러)를 toast/로그로 옮긴다.
//! 충돌은 `unique_dest` 로 "(copy)" 사본을 만들어 회피하고, 디렉토리 자기 자신/
//! 하위로의 붙여넣기는 무한 재귀 방지를 위해 거부한다.

use std::path::{Path, PathBuf};

/// `dest_dir` 안에서 `name` 과 충돌하지 않는 경로. 충돌 시 "name (copy)",
/// "name (copy 2)", … (확장자 보존).
pub fn unique_dest(dest_dir: &Path, name: &str) -> PathBuf {
    let candidate = dest_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str());
    let mut i = 1u32;
    loop {
        let suffix = if i == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {i})")
        };
        let fname = match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = dest_dir.join(&fname);
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

/// 디렉토리 재귀 복사 (파일이면 단순 복사).
pub fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

/// 파일/디렉토리 삭제 (재귀).
pub fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// 한 항목을 `dest_dir` 로 복사 또는 이동. 충돌은 `unique_dest` 로 회피.
/// `cut` 이고 같은 디렉토리면 no-op. 자기 자신/하위로의 이동은 거부.
pub fn transfer(src: &Path, dest_dir: &Path, cut: bool) -> std::io::Result<PathBuf> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    // 자기 자신 또는 그 하위로의 붙여넣기 거부 (무한 재귀/모순 방지).
    if dest_dir == src || dest_dir.starts_with(src) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cannot paste into itself",
        ));
    }
    // cut 인데 이미 dest_dir 안에 있으면 변화 없음.
    if cut && src.parent() == Some(dest_dir) {
        return Ok(src.to_path_buf());
    }
    let dst = unique_dest(dest_dir, &name);
    if cut {
        // 같은 볼륨이면 rename, 실패(예: 볼륨 교차)면 copy + remove 폴백.
        match std::fs::rename(src, &dst) {
            Ok(()) => Ok(dst),
            Err(_) => {
                copy_recursive(src, &dst)?;
                remove_path(src)?;
                Ok(dst)
            }
        }
    } else {
        copy_recursive(src, &dst)?;
        Ok(dst)
    }
}

/// 여러 항목을 `dest_dir` 로 붙여넣기. `(성공 수, 첫 에러 메시지)` 반환.
pub fn paste_all(paths: &[PathBuf], dest_dir: &Path, cut: bool) -> (usize, Option<String>) {
    let mut ok = 0usize;
    let mut err = None;
    for p in paths {
        match transfer(p, dest_dir, cut) {
            Ok(_) => ok += 1,
            Err(e) => {
                if err.is_none() {
                    err = Some(e.to_string());
                }
            }
        }
    }
    (ok, err)
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    #[test]
    fn unique_dest_appends_copy_suffix() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_uniq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, b"x").unwrap();
        let d1 = unique_dest(&dir, "a.txt");
        assert_eq!(d1, dir.join("a (copy).txt"));
        std::fs::write(&d1, b"x").unwrap();
        let d2 = unique_dest(&dir, "a.txt");
        assert_eq!(d2, dir.join("a (copy 2).txt"));
        // 충돌 없으면 그대로.
        assert_eq!(unique_dest(&dir, "z.txt"), dir.join("z.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transfer_rejects_paste_into_self() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_self_{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        // dir → dir/sub (하위) 거부.
        assert!(transfer(&dir, &sub, true).is_err());
        // dir → dir (자기 자신) 거부.
        assert!(transfer(&dir, &dir, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_then_delete() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_copy_{}", std::process::id()));
        let dst = dir.join("dst");
        std::fs::create_dir_all(&dst).unwrap();
        let f = dir.join("f.txt");
        std::fs::write(&f, b"hello").unwrap();
        let out = transfer(&f, &dst, false).unwrap();
        assert!(out.exists());
        assert!(f.exists()); // 복사라 원본 유지.
        remove_path(&out).unwrap();
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
