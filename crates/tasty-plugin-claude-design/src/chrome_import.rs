//! 로컬 Chromium 계열(Google Chrome) 프로필의 claude.ai 쿠키를 읽어 Playwright storageState
//! 로 변환한다.
//!
//! [`crate::firefox_import`] 와 같은 목적(자동화 로그인이 Google 에 "이 브라우저 또는 앱은
//! 안전하지 않을 수 있습니다"로 막히는 환경의 대안)이지만, Firefox 대신 Chrome 을 쓰는
//! 경우를 위한 경로다. 로그인 자체를 자동화가 수행하지 않고 **이미 사람이 정상 로그인해 둔
//! 세션 쿠키를 재사용**하므로 Google 의 자동화 탐지가 트리거되지 않는다.
//!
//! **구조 — OS 무관 로직은 한 번만, OS별 조각만 분기** ([`firefox_import`] 와 동일 패턴):
//! - 프로필/DB 탐색·쿠키 읽기·AES-128-CBC 복호·storageState 조립은 전부 OS 무관(공통).
//! - 유일한 OS 분기점은 [`safe_storage_key`] — Chromium 이 쿠키를 OS keyring 기반 "Safe
//!   Storage" 로 **암호화**하므로, 그 비밀에서 AES 키를 얻는 방법만 OS마다 다르다. macOS 는
//!   Keychain, Linux 는 gnome-keyring/kwallet, Windows 는 DPAPI. **현재 macOS 만 구현** —
//!   나머지는 `safe_storage_key` 가 명확한 미지원 에러를 반환한다(크로스플랫폼 컴파일 유지).
//!   (프로필 경로도 OS마다 달라 [`chrome_base_dirs`] 한 곳에서 분기.)
//!
//! macOS v10 스킴: key = PBKDF2-HMAC-SHA1(Keychain pw, salt=`saltysalt`, iter=1003, len=16),
//! 각 `encrypted_value` = `v10` 프리픽스(3바이트) + AES-128-CBC(IV=0x20×16) 암호문 → PKCS7
//! unpad. 최신 Chrome(M127+)은 평문 앞에 32바이트 도메인 해시를 덧대므로 UTF-8 로 바로
//! 해석되지 않으면 앞 32바이트를 떼고 재시도한다.
//!
//! Keychain 접근은 첫 실행 시 OS 승인 프롬프트를 띄울 수 있다. 신뢰 경계는 ADR-0018 과 동일
//! (같은 OS user 프로세스는 어차피 서로의 자격증명에 접근 가능).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aes::Aes128;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const IV: [u8; 16] = [0x20; 16];
const KEY_LEN: usize = 16;

/// 로컬 Chrome 프로필에서 claude.ai 세션을 Playwright storageState(JSON 문자열)로 가져온다.
pub fn import_from_chrome() -> Result<String, String> {
    // 1. Chrome 프로필/쿠키 존재 확인 먼저 — 없으면 Keychain 프롬프트 없이 즉시 실패.
    let db = find_cookies_db()
        .ok_or_else(|| "no Chrome profile with claude.ai cookies found".to_string())?;
    // 2. 유일한 OS 분기점: Safe Storage 키 유도(미지원 OS 는 여기서 Err).
    let key = safe_storage_key()?;
    // 3. 이하 공통: 읽기·복호·조립.
    let cookies = read_claude_cookies(&db, &key).map_err(|e| e.to_string())?;
    if cookies.is_empty() {
        return Err(format!(
            "no decryptable claude.ai cookies in Chrome profile {} — log into claude.ai with Chrome first",
            db.display()
        ));
    }
    Ok(json!({ "cookies": cookies, "origins": [] }).to_string())
}

/// Chrome user-data 루트(OS별). [`crate::firefox_import`] 의 `firefox_base_dirs` 와 같은 패턴.
fn chrome_base_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| vec![PathBuf::from(h).join("Library/Application Support/Google/Chrome")])
            .unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("HOME")
            .map(|h| {
                let base = PathBuf::from(h);
                vec![
                    base.join(".config/google-chrome"),
                    base.join(".config/chromium"),
                ]
            })
            .unwrap_or_default()
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|a| vec![PathBuf::from(a).join("Google/Chrome/User Data")])
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// user-data 루트들의 `Default` + `Profile *` 중 claude.ai 쿠키가 든 첫 `Cookies` DB.
fn find_cookies_db() -> Option<PathBuf> {
    for base in chrome_base_dirs() {
        let mut candidates: Vec<PathBuf> = vec![base.join("Default/Cookies")];
        if let Ok(read) = std::fs::read_dir(&base) {
            for entry in read.flatten() {
                if entry.file_name().to_string_lossy().starts_with("Profile ") {
                    candidates.push(entry.path().join("Cookies"));
                }
            }
        }
        if let Some(db) = candidates
            .into_iter()
            .find(|db| db.is_file() && has_claude_cookies(db))
        {
            return Some(db);
        }
    }
    None
}

fn has_claude_cookies(db: &Path) -> bool {
    let Ok(conn) = open_ro(db) else { return false };
    conn.query_row(
        "SELECT count(*) FROM cookies WHERE host_key LIKE '%claude.ai'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// **유일한 OS 분기점** — Chromium "Safe Storage" 비밀에서 AES-128 키를 유도한다.
/// macOS = Keychain generic password + PBKDF2. Linux/Windows 는 각각 keyring / DPAPI 스킴이
/// 필요해 아직 미구현.
#[cfg(target_os = "macos")]
fn safe_storage_key() -> Result<[u8; KEY_LEN], String> {
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;

    let out = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            "Chrome Safe Storage",
            "-a",
            "Chrome",
        ])
        .output()
        .map_err(|e| format!("failed to run `security`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "could not read 'Chrome Safe Storage' from Keychain (denied or missing): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let pw = raw.trim_end_matches('\n');
    if pw.is_empty() {
        return Err("empty 'Chrome Safe Storage' Keychain password".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha1>(pw.as_bytes(), b"saltysalt", 1003, &mut key);
    Ok(key)
}

#[cfg(not(target_os = "macos"))]
fn safe_storage_key() -> Result<[u8; KEY_LEN], String> {
    Err("Chrome session import is currently supported on macOS only".to_string())
}

/// Chrome 실행 중에도 잠금 무시하고 커밋된 내용을 읽도록 immutable 읽기 전용으로 연다.
fn open_ro(db: &Path) -> rusqlite::Result<Connection> {
    let uri = format!("file:{}?immutable=1", db.display());
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
}

fn read_claude_cookies(db: &Path, key: &[u8; KEY_LEN]) -> rusqlite::Result<Vec<Value>> {
    let conn = open_ro(db)?;
    let mut stmt = conn.prepare(
        "SELECT host_key, name, encrypted_value, path, expires_utc, is_secure, is_httponly, samesite \
         FROM cookies WHERE host_key LIKE '%claude.ai' ORDER BY host_key, name",
    )?;
    let rows = stmt.query_map([], |row| {
        let host: String = row.get(0)?;
        let name: String = row.get(1)?;
        let enc: Vec<u8> = row.get(2)?;
        let path: String = row.get(3)?;
        // expires_utc: 1601-01-01 기준 마이크로초(Windows epoch). 세션 쿠키는 0.
        let expires_utc: i64 = row.get(4)?;
        let is_secure: i64 = row.get(5)?;
        let is_http_only: i64 = row.get(6)?;
        let same_site: i64 = row.get(7)?;
        Ok((
            host,
            name,
            enc,
            path,
            expires_utc,
            is_secure,
            is_http_only,
            same_site,
        ))
    })?;

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let (host, name, enc, path, expires_utc, is_secure, is_http_only, same_site) = row?;
        let Some(value) = decrypt_value(&enc, key) else {
            // 복호 실패(스킴 불일치 등) 쿠키는 조용히 건너뛴다 — 부분 세션이라도 유효.
            tracing::warn!(cookie = %name, "chrome cookie decrypt failed — skipped");
            continue;
        };
        if !seen.insert((name.clone(), host.clone(), path.clone())) {
            continue;
        }
        out.push(json!({
            "name": name,
            "value": value,
            "domain": host,
            "path": path,
            "expires": if expires_utc > 0 {
                expires_utc as f64 / 1_000_000.0 - 11_644_473_600.0
            } else {
                -1.0
            },
            "httpOnly": is_http_only != 0,
            "secure": is_secure != 0,
            "sameSite": match same_site { 0 => "None", 2 => "Strict", _ => "Lax" },
        }));
    }
    Ok(out)
}

/// `v10`/`v11` AES-128-CBC 복호. 최신 Chrome 의 32바이트 도메인 해시 프리픽스도 처리.
/// 프리픽스 없는 레거시 평문 쿠키는 그대로 반환.
fn decrypt_value(enc: &[u8], key: &[u8; KEY_LEN]) -> Option<String> {
    if !(enc.starts_with(b"v10") || enc.starts_with(b"v11")) {
        return std::str::from_utf8(enc).ok().map(str::to_string);
    }
    let ct = &enc[3..];
    if ct.is_empty() || !ct.len().is_multiple_of(16) {
        return None;
    }
    let mut buf = ct.to_vec();
    let plain = Aes128CbcDec::new(key.into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    if let Ok(s) = std::str::from_utf8(plain) {
        return Some(s.to_string());
    }
    // 앞 32바이트(도메인 해시) 제거 후 재시도.
    if plain.len() > 32
        && let Ok(s) = std::str::from_utf8(&plain[32..])
    {
        return Some(s.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // 고정 키 `0123456789abcdef` 로 node(crypto)가 독립 생성한 v10 벡터. Rust 복호가 node
    // 암호문을 되돌리는지(크로스검증) + 32바이트 프리픽스 strip 경로를 확인한다.
    const KEY: &[u8; 16] = b"0123456789abcdef";

    #[test]
    fn decrypts_v10_with_32byte_domain_prefix() {
        let enc = from_hex(
            "763130064e043f5f033a0bad70f9a742d652db8ea184ecc1625de6c306c13c5415b509855e7dbe7cdd90a0da403b22302e08babec770c5ec6ae1ca135a7ca281982ea5",
        );
        assert_eq!(
            decrypt_value(&enc, KEY).as_deref(),
            Some("sk-ant-sid02-EXAMPLE")
        );
    }

    #[test]
    fn decrypts_v10_without_prefix() {
        let enc = from_hex("7631300c74ec5f600bd838f4a1bc89f0f1bc0b");
        assert_eq!(decrypt_value(&enc, KEY).as_deref(), Some("plain-value-123"));
    }

    #[test]
    fn legacy_unencrypted_value_passthrough() {
        assert_eq!(
            decrypt_value(b"raw-cookie", KEY).as_deref(),
            Some("raw-cookie")
        );
    }

    #[test]
    fn wrong_key_or_garbage_yields_none() {
        // v10 프리픽스지만 블록 정렬 안 됨 → None.
        assert_eq!(decrypt_value(b"v10short", KEY), None);
    }
}
