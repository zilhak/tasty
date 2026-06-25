//! 시스템에 설치된 Playwright 런타임 탐지 (M2).
//!
//! 이 plugin 은 Playwright·chromium·node 를 **번들하거나 설치하지 않는다.** 시스템에
//! 이미 설치된 것을 탐지해 그대로 사용한다 (설계 `.claude-workspace/plans/claude-design-plugin.md`
//! §0 런타임 정책 / §3.1 탐지). 없으면 설치를 대행하지 않고 `runtime_missing` 으로 보고한다.
//!
//! 모든 탐지는 로컬(env / 파일시스템 / `npm root -g`)이라 호스트 IPC 가 필요 없다.
//! 경로 분기는 `#[cfg]` attribute 로 코드를 제거하지 않고 `cfg!()` 런타임 분기 + env
//! 변수로 처리해 3 OS 모두 컴파일된다 (CLAUDE.md 크로스플랫폼 원칙).

use std::path::PathBuf;

use serde_json::{Value, json};

/// 시스템 런타임 탐지 결과. 각 컴포넌트는 발견 시 경로, 미발견 시 `None`.
#[derive(Debug, Clone, Default)]
pub struct RuntimeDetection {
    /// `node` 실행 파일 (Windows 는 `node.exe`).
    pub node: Option<PathBuf>,
    /// Playwright 모듈 디렉토리 (`playwright` 또는 `@playwright/test`).
    pub playwright: Option<PathBuf>,
    /// ms-playwright 캐시의 `chromium-<revision>` 디렉토리 (최고 revision).
    pub chromium: Option<PathBuf>,
}

impl RuntimeDetection {
    /// node → playwright → chromium 순으로 탐지한다.
    pub fn run() -> Self {
        Self {
            node: find_on_path("node"),
            playwright: find_playwright(),
            chromium: find_chromium(),
        }
    }

    /// 무엇이 빠졌는지 첫 결손을 node → playwright → chromium 순으로 반환.
    /// 모두 있으면 `None`.
    pub fn missing(&self) -> Option<&'static str> {
        if self.node.is_none() {
            Some("node")
        } else if self.playwright.is_none() {
            Some("playwright")
        } else if self.chromium.is_none() {
            Some("chromium")
        } else {
            None
        }
    }

    /// `"ok"` 또는 `"runtime_missing: <component>"`. status/detect 응답의 `runtime` 필드.
    pub fn runtime_status(&self) -> String {
        match self.missing() {
            None => "ok".to_string(),
            Some(component) => format!("runtime_missing: {component}"),
        }
    }

    /// 발견 경로를 JSON 으로. 미발견 컴포넌트는 `null`.
    pub fn to_json(&self) -> Value {
        json!({
            "node": path_to_json(&self.node),
            "playwright": path_to_json(&self.playwright),
            "chromium": path_to_json(&self.chromium),
        })
    }
}

fn path_to_json(p: &Option<PathBuf>) -> Value {
    match p {
        Some(path) => Value::String(path.to_string_lossy().into_owned()),
        None => Value::Null,
    }
}

/// `PATH` 에서 실행 파일을 찾는다. Windows 는 `<exe>.exe` / `<exe>.cmd` 도 시도.
fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{exe}.exe"), format!("{exe}.cmd"), exe.to_string()]
    } else {
        vec![exe.to_string()]
    };
    for dir in std::env::split_paths(&path_var) {
        for name in &candidates {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Playwright 모듈을 우선순위대로 탐색 (설계 §3.1).
///
/// 1. (설정값 — settings page PathInput, M2 범위 밖. 후속 §12 에서 활성화)
/// 2. env override `TASTY_DESIGN_PLAYWRIGHT` (명시 경로)
/// 3. 전역 npm root (`npm root -g`) 하위 `playwright` / `@playwright/test`
/// 4. 조사 실측 폴백: `<npm root>/@executeautomation/playwright-mcp-server/node_modules/playwright`
fn find_playwright() -> Option<PathBuf> {
    // 2. env override.
    if let Some(raw) = std::env::var_os("TASTY_DESIGN_PLAYWRIGHT") {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "TASTY_DESIGN_PLAYWRIGHT 가 가리키는 디렉토리가 없음 — 자동 탐지로 폴백"
        );
    }

    // 3·4. 전역 npm root 하위.
    let npm_root = npm_global_root()?;
    let direct = [
        npm_root.join("playwright"),
        npm_root.join("@playwright").join("test"),
    ];
    for candidate in direct {
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    // 4. playwright-mcp-server 번들 안의 playwright (조사 §10 실측 경로).
    let bundled = npm_root
        .join("@executeautomation")
        .join("playwright-mcp-server")
        .join("node_modules")
        .join("playwright");
    if bundled.is_dir() {
        return Some(bundled);
    }
    None
}

/// `npm root -g` 결과 디렉토리. npm 미설치/실패 시 `None`.
fn npm_global_root() -> Option<PathBuf> {
    let npm = find_on_path("npm")?;
    let output = match std::process::Command::new(&npm)
        .args(["root", "-g"])
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!(error = %e, "`npm root -g` 실행 실패");
            return None;
        }
    };
    if !output.status.success() {
        tracing::warn!(status = ?output.status, "`npm root -g` 비정상 종료");
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return None;
    }
    let path = PathBuf::from(root);
    if path.is_dir() { Some(path) } else { None }
}

/// ms-playwright 캐시에서 `chromium-<revision>` 디렉토리(최고 revision)를 찾는다.
/// revision 은 설치본을 따르므로 하드코딩하지 않고 디렉토리를 스캔한다 (설계 §3.1).
fn find_chromium() -> Option<PathBuf> {
    let cache = ms_playwright_cache_dir()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(&cache).ok()?.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let Some(rev_raw) = name.strip_prefix("chromium-") else {
            continue;
        };
        let Ok(rev) = rev_raw.parse::<u64>() else {
            continue;
        };
        if best.as_ref().is_none_or(|(best_rev, _)| rev > *best_rev) {
            best = Some((rev, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

/// Playwright 브라우저 캐시 디렉토리. `PLAYWRIGHT_BROWSERS_PATH` override 우선,
/// 아니면 OS 기본 위치 (env 기반 런타임 분기).
fn ms_playwright_cache_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
    }
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("ms-playwright"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library")
                .join("Caches")
                .join("ms-playwright")
        })
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache").join("ms-playwright"))
    }
    .filter(|p: &PathBuf| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_reports_first_gap_in_order() {
        let none = RuntimeDetection::default();
        assert_eq!(none.missing(), Some("node"));

        let only_node = RuntimeDetection {
            node: Some(PathBuf::from("/usr/bin/node")),
            ..Default::default()
        };
        assert_eq!(only_node.missing(), Some("playwright"));

        let node_pw = RuntimeDetection {
            node: Some(PathBuf::from("/usr/bin/node")),
            playwright: Some(PathBuf::from("/pw")),
            ..Default::default()
        };
        assert_eq!(node_pw.missing(), Some("chromium"));

        let all = RuntimeDetection {
            node: Some(PathBuf::from("/usr/bin/node")),
            playwright: Some(PathBuf::from("/pw")),
            chromium: Some(PathBuf::from("/chromium-1179")),
        };
        assert_eq!(all.missing(), None);
        assert_eq!(all.runtime_status(), "ok");
    }

    #[test]
    fn runtime_status_formats_missing() {
        let none = RuntimeDetection::default();
        assert_eq!(none.runtime_status(), "runtime_missing: node");
    }

    #[test]
    fn to_json_uses_null_for_absent() {
        let det = RuntimeDetection {
            node: Some(PathBuf::from("/usr/bin/node")),
            ..Default::default()
        };
        let v = det.to_json();
        assert_eq!(v["node"], json!("/usr/bin/node"));
        assert_eq!(v["playwright"], Value::Null);
        assert_eq!(v["chromium"], Value::Null);
    }
}
