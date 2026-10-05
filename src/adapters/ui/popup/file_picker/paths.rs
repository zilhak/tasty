//! 파일 선택기의 경로 조작과 확장자 필터.

use super::*;

/// 파일명이 확장자 필터에 매치하는지 — 대소문자 무시, 점 없는 확장자 비교.
/// `filters` 가 비면 항상 통과(필터 없음).
pub(crate) fn matches_filters(filters: &[String], name: &str) -> bool {
    if filters.is_empty() {
        return true;
    }
    let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) else {
        return false;
    };
    filters.iter().any(|f| f.eq_ignore_ascii_case(ext))
}

/// 원격 OS를 알 수 없어 경로에 역슬래시가 있으면 Windows 형식으로 추정한다.
/// 역슬래시를 이름에 포함한 POSIX 경로는 이 방식으로 구별하지 못한다.
pub(super) fn is_windows_style_remote_path(p: &str) -> bool {
    p.contains('\\')
}

pub(super) fn join_dir(is_remote: bool, dir: &str, name: &str) -> String {
    if is_remote {
        if is_windows_style_remote_path(dir) {
            if dir.ends_with('\\') {
                format!("{dir}{name}")
            } else {
                format!("{dir}\\{name}")
            }
        } else if dir.ends_with('/') {
            format!("{dir}{name}")
        } else {
            format!("{dir}/{name}")
        }
    } else {
        Path::new(dir).join(name).to_string_lossy().to_string()
    }
}

/// root 부터 현재 경로까지의 조상 목록(문자열 형태) — 브레드크럼 라벨/내비게이션
/// 타깃 둘 다 이 목록에서 유도한다. 로컬은 `Path` 컴포넌트 기반이라 Windows 드라이브
/// 루트도 정확히 다룬다. 원격은 문자열 분해인데, `is_windows_style_remote_path` 로
/// POSIX(`/`)와 Windows(`\`, 드라이브 루트 보존) 를 분기한다.
pub(crate) fn path_ancestors(is_remote: bool, current_dir: &str) -> Vec<String> {
    if is_remote {
        if is_windows_style_remote_path(current_dir) {
            let mut segs = current_dir.split('\\').filter(|s| !s.is_empty());
            let Some(drive) = segs.next() else {
                return vec![current_dir.to_string()];
            };
            let root = format!("{drive}\\");
            let mut out = vec![root.clone()];
            let mut acc = root;
            for seg in segs {
                if !acc.ends_with('\\') {
                    acc.push('\\');
                }
                acc.push_str(seg);
                out.push(acc.clone());
            }
            out
        } else {
            let mut out = vec!["/".to_string()];
            let mut acc = String::new();
            for seg in current_dir.split('/').filter(|s| !s.is_empty()) {
                acc.push('/');
                acc.push_str(seg);
                out.push(acc.clone());
            }
            out
        }
    } else {
        let mut v: Vec<PathBuf> = Path::new(current_dir)
            .ancestors()
            .map(Path::to_path_buf)
            .collect();
        v.reverse();
        v.into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }
}

pub(crate) fn crumb_label(is_remote: bool, full_path: &str) -> String {
    if full_path == "/" {
        return "/".to_string();
    }
    if is_remote {
        if is_windows_style_remote_path(full_path) {
            if full_path.ends_with('\\') {
                return full_path.to_string(); // 드라이브 루트 — 그대로("C:\\").
            }
            return full_path
                .rsplit('\\')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(full_path)
                .to_string();
        }
        full_path
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(full_path)
            .to_string()
    } else {
        Path::new(full_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| full_path.to_string())
    }
}
