//! 로컬 Firefox 프로필의 claude.ai 쿠키를 읽어 Playwright storageState 로 변환한다.
//!
//! Google 은 자동화(CDP)로 구동되는 Chromium 계열 브라우저의 OAuth 로그인을 "이 브라우저
//! 또는 앱은 안전하지 않을 수 있습니다"로 차단한다(`runner.rs`/`design-runner.js` 의
//! `launchLoginBrowser` 주석 참조). 공식 Chrome 이 없는 아키텍처(예: arm64 Linux — Chrome
//! 리눅스 빌드는 amd64 전용)에서는 `channel:'chrome'` 우회조차 쓸 수 없어 이 차단을 피할
//! 방법이 없다. Firefox 는 이 차단 대상이 아니므로, 사용자가 평소 쓰는 Firefox 에서 이미
//! 정상적으로 로그인해 둔 세션 쿠키를 그대로 재사용한다 — 로그인 자체를 자동화가 수행하지
//! 않으므로 Google 의 자동화 탐지가 아예 트리거되지 않는다.
//!
//! Firefox 는 쿠키 값을 암호화하지 않고 `cookies.sqlite` 에 평문 저장한다(Chromium 의
//! OS keyring 기반 "Safe Storage" 암호화와 다름) — 그래서 별도 복호화 없이 그대로 읽을
//! 수 있다. 대상 프로필 소유자와 동일한 OS user 권한만 있으면 되고, 이는 ADR-0018 이 이미
//! 채택한 신뢰 경계(같은 user 프로세스는 어차피 서로의 평문 자격증명에 접근 가능)와 동일하다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

pub fn import_from_firefox() -> Result<String, String> {
    let profile_dir =
        find_default_profile().ok_or_else(|| "no Firefox profile found".to_string())?;
    let cookies_db = profile_dir.join("cookies.sqlite");
    if !cookies_db.is_file() {
        return Err(format!(
            "cookies.sqlite not found under {}",
            profile_dir.display()
        ));
    }
    let cookies = read_claude_cookies(&cookies_db).map_err(|e| e.to_string())?;
    if cookies.is_empty() {
        return Err(format!(
            "no claude.ai cookies in Firefox profile {} — log into claude.ai with Firefox first",
            profile_dir.display()
        ));
    }
    let state = json!({ "cookies": cookies, "origins": [] });
    Ok(state.to_string())
}

fn firefox_base_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    #[cfg(target_os = "linux")]
    {
        vec![
            // 전통 경로.
            home.join(".mozilla/firefox"),
            // 최근 Ubuntu apt 패키지가 쓰는 XDG 경로(실측: Ubuntu 24.04 firefox 149).
            home.join(".config/mozilla/firefox"),
            // snap 컨파인먼트.
            home.join("snap/firefox/common/.mozilla/firefox"),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![home.join("Library/Application Support/Firefox")]
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|a| vec![PathBuf::from(a).join("Mozilla/Firefox")])
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

fn find_default_profile() -> Option<PathBuf> {
    find_default_profile_in(&firefox_base_dirs())
}

/// 후보 base dir 들을 순서대로 시도한다. base 분리는 순수 테스트 목적 — 실제 후보 목록은
/// [`firefox_base_dirs`], 실제 판정 로직은 여기.
fn find_default_profile_in(bases: &[PathBuf]) -> Option<PathBuf> {
    for base in bases {
        // 앞선 후보에 profiles.ini 가 없어도(가장 흔한 케이스 — 설치 방식별로 후보 중
        // 하나만 존재) 다음 후보를 계속 시도해야 한다.
        let Ok(ini) = std::fs::read_to_string(base.join("profiles.ini")) else {
            continue;
        };
        if let Some(rel) = default_profile_path(&ini) {
            let resolved = if rel.is_relative {
                base.join(&rel.path)
            } else {
                PathBuf::from(&rel.path)
            };
            if resolved.is_dir() {
                return Some(resolved);
            }
        }
    }
    None
}

struct ProfilePath {
    path: String,
    is_relative: bool,
}

/// 최소 INI 파서 기반 기본 프로필 판정. 우선순위:
/// 1) `[InstallXXXX]` 의 `Default=`(현재 설치가 실제로 쓰는 프로필 — 실측상 가장 신뢰도 높음)
/// 2) `Default=1` 인 `[ProfileN]`
/// 3) 첫 `[ProfileN]`
fn default_profile_path(ini: &str) -> Option<ProfilePath> {
    let sections = parse_ini_sections(ini);

    for (name, kv) in &sections {
        if name.starts_with("Install")
            && let Some(target) = kv.get("Default")
        {
            if let Some(p) = profile_by_path(&sections, target) {
                return Some(p);
            }
            // [ProfileN] 매칭이 안 돼도 Default 값 자체를 상대경로로 시도.
            return Some(ProfilePath {
                path: target.clone(),
                is_relative: true,
            });
        }
    }

    for (name, kv) in &sections {
        if name.starts_with("Profile")
            && kv.get("Default").map(String::as_str) == Some("1")
            && let Some(path) = kv.get("Path")
        {
            return Some(ProfilePath {
                path: path.clone(),
                is_relative: kv.get("IsRelative").map(String::as_str) != Some("0"),
            });
        }
    }

    sections.iter().find_map(|(name, kv)| {
        if name.starts_with("Profile") {
            kv.get("Path").map(|path| ProfilePath {
                path: path.clone(),
                is_relative: kv.get("IsRelative").map(String::as_str) != Some("0"),
            })
        } else {
            None
        }
    })
}

fn profile_by_path(
    sections: &[(String, HashMap<String, String>)],
    target: &str,
) -> Option<ProfilePath> {
    sections.iter().find_map(|(name, kv)| {
        if name.starts_with("Profile") && kv.get("Path").map(String::as_str) == Some(target) {
            Some(ProfilePath {
                path: target.to_string(),
                is_relative: kv.get("IsRelative").map(String::as_str) != Some("0"),
            })
        } else {
            None
        }
    })
}

fn parse_ini_sections(ini: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut sections = Vec::new();
    let mut current: Option<(String, HashMap<String, String>)> = None;
    for line in ini.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Some(done) = current.take() {
                sections.push(done);
            }
            current = Some((name.to_string(), HashMap::new()));
        } else if let Some((k, v)) = line.split_once('=')
            && let Some((_, kv)) = current.as_mut()
        {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    if let Some(done) = current {
        sections.push(done);
    }
    sections
}

/// Firefox 는 실행 중이면 `cookies.sqlite` 를 잠그므로 읽기 전용 + immutable 로 연다
/// (WAL 파일이 있어도 별도 체크포인트 없이 커밋된 내용을 읽는다).
fn read_claude_cookies(db_path: &Path) -> rusqlite::Result<Vec<Value>> {
    let uri = format!("file:{}?immutable=1", db_path.display());
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut stmt = conn.prepare(
        "SELECT name, value, host, path, expiry, isSecure, isHttpOnly, sameSite \
         FROM moz_cookies WHERE host LIKE '%claude.ai' ORDER BY host, name",
    )?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let value: String = row.get(1)?;
        let host: String = row.get(2)?;
        let path: String = row.get(3)?;
        // Firefox 실측(이 스키마 버전): expiry 는 밀리초 단위로 저장된다. Playwright
        // storageState 의 expires 는 초 단위 float(세션 쿠키는 -1).
        let expiry_ms: i64 = row.get(4)?;
        let is_secure: i64 = row.get(5)?;
        let is_http_only: i64 = row.get(6)?;
        let same_site: i64 = row.get(7)?;
        Ok(json!({
            "name": name,
            "value": value,
            "domain": host,
            "path": path,
            "expires": if expiry_ms > 0 { expiry_ms as f64 / 1000.0 } else { -1.0 },
            "httpOnly": is_http_only != 0,
            "secure": is_secure != 0,
            "sameSite": match same_site { 0 => "None", 2 => "Strict", _ => "Lax" },
        }))
    })?;

    // Firefox 는 컨테이너(originAttributes) 별로 같은 (name, host, path) 쿠키가 중복될 수
    // 있는데, Playwright storageState 는 중복 키를 기대하지 않는다 — 처음 값(기본 정렬상
    // 대개 default 컨테이너)을 유지하고 나머지는 버린다.
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let cookie = row?;
        let key = (
            cookie["name"].as_str().unwrap_or_default().to_string(),
            cookie["domain"].as_str().unwrap_or_default().to_string(),
            cookie["path"].as_str().unwrap_or_default().to_string(),
        );
        if seen.insert(key) {
            out.push(cookie);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_section_default() {
        let ini = "[Install4F96D1932A9F858E]\nDefault=eocqn8gw.default-release\nLocked=1\n\n\
                   [Profile1]\nName=default\nIsRelative=1\nPath=tcl43j6q.default\nDefault=1\n\n\
                   [Profile0]\nName=default-release\nIsRelative=1\nPath=eocqn8gw.default-release\n";
        let p = default_profile_path(ini).expect("profile found");
        assert_eq!(p.path, "eocqn8gw.default-release");
        assert!(p.is_relative);
    }

    #[test]
    fn falls_back_to_default_flag_without_install_section() {
        let ini =
            "[Profile0]\nName=default-release\nIsRelative=1\nPath=abc.default-release\nDefault=1\n";
        let p = default_profile_path(ini).expect("profile found");
        assert_eq!(p.path, "abc.default-release");
    }

    #[test]
    fn falls_back_to_first_profile() {
        let ini = "[Profile0]\nName=only\nIsRelative=0\nPath=/abs/path\n";
        let p = default_profile_path(ini).expect("profile found");
        assert_eq!(p.path, "/abs/path");
        assert!(!p.is_relative);
    }

    #[test]
    fn skips_missing_base_dir_and_tries_next() {
        // 회귀 테스트: 첫 후보에 profiles.ini 가 없으면(가장 흔한 케이스 — 설치 방식별로
        // 후보 중 하나만 존재) 나머지 후보를 계속 시도해야 한다. `?` 로 첫 실패에서 함수
        // 전체를 조기 반환하던 버그가 있었다.
        let tmp = std::env::temp_dir().join(format!(
            "tasty-firefox-import-test-{}-{}",
            std::process::id(),
            "skips_missing_base_dir_and_tries_next"
        ));
        let missing_base = tmp.join("no-such-dir");
        let real_base = tmp.join("real");
        let profile_dir = real_base.join("abc.default-release");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            real_base.join("profiles.ini"),
            "[Profile0]\nName=default-release\nIsRelative=1\nPath=abc.default-release\nDefault=1\n",
        )
        .unwrap();

        let found = find_default_profile_in(&[missing_base, real_base.clone()]);
        assert_eq!(found, Some(profile_dir));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn returns_none_for_empty_ini() {
        assert!(default_profile_path("").is_none());
    }
}
