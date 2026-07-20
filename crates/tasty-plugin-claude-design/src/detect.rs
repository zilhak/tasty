//! 시스템에 설치된 Playwright 런타임 탐지 (M2).
//!
//! 이 plugin 은 Playwright·chromium·node 를 **번들하거나 설치하지 않는다.** 시스템에
//! 이미 설치된 것을 탐지해 그대로 사용한다 (설계 `.claude-workspace/plans/claude-design-plugin.md`
//! §0 런타임 정책 / §3.1 탐지). 없으면 설치를 대행하지 않고 `runtime_missing` 으로 보고한다.
//!
//! 모든 탐지는 로컬(env / 파일시스템 / `npm root -g`)이라 호스트 IPC 가 필요 없다.
//! 경로 분기는 `#[cfg]` attribute 로 코드를 제거하지 않고 `cfg!()` 런타임 분기 + env
//! 변수로 처리해 3 OS 모두 컴파일된다 (CLAUDE.md 크로스플랫폼 원칙).

use std::ffi::OsStr;
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
            node: find_node(),
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

/// node 실행 파일을 탐지한다. 우선순위:
/// 1. env override `TASTY_DESIGN_NODE` — 유효한 실행 파일이면 그대로, 아니면 warn 후 폴백
///    (`TASTY_DESIGN_PLAYWRIGHT` 의 warn-후-폴백 동작과 일관).
/// 2. PATH → PATH 외 표준 위치(nvm 최고버전 / homebrew) — `find_runtime_exe`.
fn find_node() -> Option<PathBuf> {
    if let Some(path) = node_override_path(std::env::var_os("TASTY_DESIGN_NODE").as_deref()) {
        return Some(path);
    }
    find_runtime_exe("node")
}

/// `TASTY_DESIGN_NODE` override 해석. 유효 실행 파일이면 `Some`, 아니면(미설정/경로 없음)
/// `None` 으로 폴백을 신호한다. env 비의존 단위 테스트를 위해 경로 판정만 분리.
fn node_override_path(raw: Option<&OsStr>) -> Option<PathBuf> {
    let path = PathBuf::from(raw?);
    if path.is_file() {
        return Some(path);
    }
    tracing::warn!(
        path = %path.display(),
        "TASTY_DESIGN_NODE 가 가리키는 실행 파일이 없음 — 자동 탐지로 폴백"
    );
    None
}

/// 실행 파일을 PATH → PATH 외 표준 위치(nvm/homebrew) 순으로 찾는다. PATH 를 앞에 두므로
/// 정상 환경에서는 기존과 동일하게 PATH 우선으로 잡히고, GUI(launchd) 처럼 로그인 셸 PATH
/// 가 결손된 컨텍스트에서만 폴백 디렉토리가 사용된다. node·npm 이 같은 디렉토리 집합을
/// 공유하므로(같은 nvm bin) node 탐지가 성공하면 npm 탐지도 함께 풀린다.
fn find_runtime_exe(exe: &str) -> Option<PathBuf> {
    find_exe_in_dirs(exe, &search_dirs())
}

/// PATH 디렉토리(우선) + PATH 외 표준 설치 위치(폴백) 를 합친 탐색 목록.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(path_var) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path_var));
    }
    dirs.extend(fallback_runtime_dirs());
    dirs
}

/// PATH 외 표준 런타임 설치 위치. nvm bin(버전 내림차순) → homebrew 순.
/// nvm/homebrew 는 unix(특히 macOS) 관습 경로라 `cfg!(unix)` 런타임 분기로 처리한다 —
/// `#[cfg]` 로 코드를 제거하지 않아 3 OS 모두 컴파일되고, Windows 에선 빈 목록을 돌려
/// 컴파일·실행이 깨지지 않는다(상단 주석 크로스플랫폼 원칙). 존재하지 않는 디렉토리는
/// `find_exe_in_dirs` 의 `is_file` 검사에서 자연히 skip 된다.
fn fallback_runtime_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(unix) {
        dirs.extend(nvm_node_bins());
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs
}

/// `~/.nvm/versions/node/*/bin` 디렉토리들을 버전 내림차순(최고 버전 우선)으로 반환.
fn nvm_node_bins() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let versions = PathBuf::from(home)
        .join(".nvm")
        .join("versions")
        .join("node");
    let Ok(read) = std::fs::read_dir(&versions) else {
        return Vec::new();
    };
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let bin = entry.path().join("bin");
        if bin.is_dir() {
            candidates.push((name, bin));
        }
    }
    order_nvm_bins(candidates)
}

/// nvm 버전 bin 후보를 버전 내림차순으로 정렬해 경로만 반환.
/// **다중 버전 선택 규칙 = 최고 semver** (chromium `chromium-<revision>` 최고 revision 선택
/// 로직과 동일 사상). 결정론적이고 nvm `default` 별칭 추적보다 단순해 채택. 파싱 불가한
/// 버전명은 맨 뒤로 밀린다.
fn order_nvm_bins(mut candidates: Vec<(String, PathBuf)>) -> Vec<PathBuf> {
    candidates.sort_by_key(|c| std::cmp::Reverse(parse_node_version(&c.0)));
    candidates.into_iter().map(|(_, bin)| bin).collect()
}

/// nvm node 버전 디렉토리명(예 `v24.4.1`)을 `(major, minor, patch)` 로 파싱. 실패 시 `None`.
fn parse_node_version(name: &str) -> Option<(u64, u64, u64)> {
    let v = name.strip_prefix('v').unwrap_or(name);
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// 주어진 디렉토리 목록을 앞에서부터 순회해 실행 파일을 찾는다(첫 매치 반환).
/// Windows 는 `<exe>.exe` / `<exe>.cmd` 도 시도. 환경 비의존 단위 테스트용으로 분리.
fn find_exe_in_dirs(exe: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{exe}.exe"), format!("{exe}.cmd"), exe.to_string()]
    } else {
        vec![exe.to_string()]
    };
    for dir in dirs {
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
    let npm = find_runtime_exe("npm")?;
    let mut cmd = std::process::Command::new(&npm);
    cmd.args(["root", "-g"]);
    // npm 은 nvm/homebrew 레이아웃에서 `#!/usr/bin/env node` shebang 스크립트다(실제
    // 바이너리가 아님). GUI(launchd)처럼 PATH 에 node 가 없는 컨텍스트에서 실행하면
    // shebang 의 `env node` 가 node 를 못 찾아 exit 127 로 실패한다 — npm 실행 파일을
    // 찾는 것과 npm 을 실행하는 것은 별개다. 탐지한 node 의 디렉토리를 자식 PATH 앞에
    // 얹어, node 탐지 성공이 곧 npm 실행 가능으로 이어지게 한다(§3.1 의도 완결).
    if let Some(node_dir) = find_node().as_deref().and_then(|p| p.parent()) {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![node_dir.to_path_buf()];
        dirs.extend(std::env::split_paths(&existing));
        match std::env::join_paths(dirs) {
            Ok(joined) => {
                cmd.env("PATH", joined);
            }
            Err(e) => tracing::warn!(error = %e, "npm 실행용 PATH 합성 실패 — 기존 PATH 유지"),
        }
    }
    let output = match cmd.output() {
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

    // (1) nvm 다중 버전 중 최고 버전 선택.
    #[test]
    fn nvm_bins_ordered_highest_version_first() {
        let v22 = PathBuf::from("/home/x/.nvm/versions/node/v22.11.0/bin");
        let v24 = PathBuf::from("/home/x/.nvm/versions/node/v24.4.1/bin");
        // 입력 순서와 무관하게 v24.4.1 이 앞에 온다.
        let ordered = order_nvm_bins(vec![
            ("v22.11.0".to_string(), v22.clone()),
            ("v24.4.1".to_string(), v24.clone()),
        ]);
        assert_eq!(ordered.first(), Some(&v24));
        let ordered_rev = order_nvm_bins(vec![
            ("v24.4.1".to_string(), v24.clone()),
            ("v22.11.0".to_string(), v22.clone()),
        ]);
        assert_eq!(ordered_rev.first(), Some(&v24));
    }

    #[test]
    fn parse_node_version_basic() {
        assert_eq!(parse_node_version("v24.4.1"), Some((24, 4, 1)));
        assert_eq!(parse_node_version("v22.11.0"), Some((22, 11, 0)));
        // semver 비교가 문자열이 아닌 수치 기준임을 확인 (v24 > v9).
        assert!(parse_node_version("v24.0.0") > parse_node_version("v9.9.9"));
        assert_eq!(parse_node_version("nonsense"), None);
    }

    // (2) TASTY_DESIGN_NODE override: 유효 실행 파일 우선, 무효 경로/미설정은 폴백 신호.
    #[test]
    fn node_override_valid_and_invalid() {
        let exe = std::env::current_exe().expect("current_exe");
        assert_eq!(node_override_path(Some(exe.as_os_str())), Some(exe.clone()));
        assert_eq!(
            node_override_path(Some(OsStr::new("/no/such/node/binary/xyz"))),
            None
        );
        assert_eq!(node_override_path(None), None);
    }

    // (3) PATH 우선 회귀: 앞선 디렉토리(PATH 자리)가 폴백 디렉토리보다 먼저 매치된다.
    #[test]
    fn earlier_dir_takes_priority() {
        let base = std::env::temp_dir().join("tasty-detect-prio");
        let first = base.join("first");
        let second = base.join("second");
        let _ = std::fs::remove_dir_all(&base); // 테스트 사전 정리 — 디렉토리 부재 시 에러는 정상이라 무시
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("node"), b"").unwrap();
        std::fs::write(second.join("node"), b"").unwrap();
        let found = find_exe_in_dirs("node", &[first.clone(), second.clone()]);
        assert_eq!(found, Some(first.join("node")));
        let _ = std::fs::remove_dir_all(&base); // 테스트 사후 정리 — 디렉토리 부재 시 에러는 정상이라 무시
    }

    // (4) 크로스플랫폼: 존재하지 않는 폴백 디렉토리만 주어지면 panic 없이 None.
    #[test]
    fn missing_dirs_yield_none_without_panic() {
        let found = find_exe_in_dirs(
            "node",
            &[
                PathBuf::from("/no/such/dir/a"),
                PathBuf::from("/no/such/dir/b"),
            ],
        );
        assert_eq!(found, None);
    }
}
