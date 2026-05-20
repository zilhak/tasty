//! `DetectorRuleKind::Lua` 평가자.
//!
//! 신뢰 모델: Lua detector rule 은 host default TOML 과 user TOML 에만 등장한다.
//! 사용자가 자기 머신에서 자기 권한으로 적은 스크립트이므로 plugin escape 우려는
//! 없고 sandbox 의 목적은 **DoS 보호 + 호스트 무결성** 두 가지:
//!
//! - **메모리 cap** (`mlua` 의 `set_memory_limit`) — 큰 string/table 폭발 차단.
//! - **명령어 cap** (`mlua` 의 `set_hook`) — `while true do end` 같은 무한 루프
//!   를 [`INSTRUCTION_BUDGET`] 명령어 안에 abort.
//! - **위험 글로벌 제거** — `debug`, `package.loadlib`, `dofile`, `loadfile`,
//!   `load`, `loadstring` 제거. `io`/`os.execute` 도 제거 (detector 의 의도는
//!   "바이트 평가" 이지 부수효과가 아님).
//! - **bytecode 차단** — `set_mode(ChunkMode::Text)` 로 binary 청크 거부.
//!
//! 평가 호출: 매 detector 별로 새 `mlua::Lua` 를 만든다 (lifetime 짧고 state 격리
//! 단순). 캐시 미적용 — 한 `identify` 호출 안에서 같은 rule 이 두 번 평가되는
//! 일은 없고, 동일 detector 가 여러 파일에 적용될 때 reuse 가치는 작다. 비용
//! 측정 후 필요해지면 `lua_pool` 로 확장.

use mlua::{ChunkMode, Lua, Table, Value};

use super::evaluator::DeepCtx;
use super::types::FileTarget;

/// 단일 detector rule eval 의 명령어 cap. 평가에 필요한 비용은 보통 수십~수백
/// 명령어. 이 cap 을 넘기는 스크립트는 무한 루프로 간주.
pub const INSTRUCTION_BUDGET: u32 = 1_000_000;

/// 메모리 cap (bytes). string.rep 폭발 등 메모리 폭주 차단.
pub const MEMORY_BUDGET: usize = 8 * 1024 * 1024;

/// 단일 Lua detector rule 평가. 매치되면 `true`. 모든 실패(파싱·런타임·cap)는
/// `tracing::warn!` 로 기록되고 `false` 반환 — observe 안전 (detector 매칭은
/// 추가/탈락 둘 다 안전 fail open 이 아님, 매치 안함이 안전한 fallback).
pub fn evaluate_lua(script: &str, target: &FileTarget, ctx: &mut DeepCtx) -> bool {
    let entry = ctx.entry(target).clone();
    let lua = match build_sandboxed_lua() {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("file_format lua: sandbox init failed: {e}");
            return false;
        }
    };
    let target_table = match build_target_table(&lua, target, &entry) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("file_format lua: target table build failed: {e}");
            return false;
        }
    };
    if let Err(e) = lua.globals().set("target", target_table) {
        tracing::warn!("file_format lua: failed to set target global: {e}");
        return false;
    }
    let chunk = lua
        .load(script)
        .set_name("detector-rule")
        .set_mode(ChunkMode::Text);
    match chunk.eval::<Value>() {
        Ok(Value::Boolean(b)) => b,
        Ok(other) => {
            // 가독성: 다른 타입 (nil, number, table 등) 은 모두 false. 사용자가 자기
            // 스크립트가 bool 안 돌려준다는 걸 알게끔 한 줄 warn.
            tracing::warn!(
                "file_format lua: script returned non-bool ({}), treating as false",
                other.type_name(),
            );
            false
        }
        Err(e) => {
            tracing::warn!("file_format lua: eval failed: {e}");
            false
        }
    }
}

fn build_sandboxed_lua() -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.set_memory_limit(MEMORY_BUDGET)?;

    // 위험 글로벌 제거. mlua 의 표준 lib 는 이미 로드된 상태로 시작하므로 부분
    // 제거. 화이트리스트 방식이 더 안전하나 detector 스크립트가 string/math/table
    // 을 자유롭게 쓰는 게 일반적 use case 이라 현재는 블랙리스트만.
    let g = lua.globals();
    for name in &[
        "dofile",
        "loadfile",
        "load",
        "loadstring",
        "debug",
        "require",
        "io",
        // os 의 위험 메서드만 nil 처리하기 어렵고 detector 에 os.* 가 필요한 use
        // case 도 거의 없으므로 통째로 제거.
        "os",
    ] {
        g.set(*name, Value::Nil)?;
    }
    if let Ok(pkg) = g.get::<Table>("package") {
        let _ = pkg.set("loadlib", Value::Nil);
        let _ = pkg.set("searchers", Value::Nil);
        let _ = pkg.set("loaders", Value::Nil);
    }
    g.set("package", Value::Nil)?;

    // 명령어 cap. 매 N 명령어마다 콜백 호출 → 콜백이 에러 리턴하면 평가 중단.
    let trigger = mlua::HookTriggers::new().every_nth_instruction(INSTRUCTION_BUDGET);
    lua.set_hook(trigger, |_lua, _debug| {
        Err::<mlua::VmState, _>(mlua::Error::external(
            "detector lua: instruction budget exceeded",
        ))
    });

    Ok(lua)
}

fn build_target_table(
    lua: &Lua,
    target: &FileTarget,
    entry: &super::evaluator::DeepCacheEntry,
) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("path", target.display())?;
    t.set("is_directory", target.is_directory())?;
    // bytes_head 는 regular file 이고 read 성공한 경우만. binary safe 하도록 Lua
    // string 으로 raw bytes 그대로 (Lua string 은 8-bit clean).
    if let Some(head) = entry.head.as_ref() {
        t.set("bytes_head", lua.create_string(head)?)?;
    } else {
        t.set("bytes_head", Value::Nil)?;
    }
    match entry.mime.as_ref() {
        Some(m) => t.set("mime", m.as_str())?,
        None => t.set("mime", Value::Nil)?,
    }

    // helper: head 가 특정 prefix 로 시작하는지. Lua string lib 으로도 가능하지만
    // detector 작성자가 자주 쓰는 패턴이라 단축 제공.
    let head_clone = entry.head.clone();
    let has_prefix = lua.create_function(move |_, prefix: mlua::String| {
        let bytes = prefix.as_bytes();
        Ok(head_clone
            .as_ref()
            .map(|h| h.starts_with(&*bytes))
            .unwrap_or(false))
    })?;
    t.set("has_prefix", has_prefix)?;

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_format::evaluator::DeepCtx;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_tmp(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).expect("write tmp");
        p
    }

    #[test]
    fn returns_true_when_script_returns_true() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(evaluate_lua("return true", &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn returns_false_when_script_returns_false() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua("return false", &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn pdf_header_matched_via_bytes_head() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "doc.pdf", b"%PDF-1.4\nrest");
        let mut ctx = DeepCtx::new();
        let script = r#"
            if target.bytes_head and target.bytes_head:sub(1,4) == "%PDF" then
                return true
            end
            return false
        "#;
        assert!(evaluate_lua(script, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn has_prefix_helper_works() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "doc.pdf", b"%PDF-1.4\nrest");
        let mut ctx = DeepCtx::new();
        let script = r#"return target.has_prefix("%PDF")"#;
        assert!(evaluate_lua(script, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn path_field_visible() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "report.weird-ext", b"data");
        let p_str = p.display().to_string();
        let mut ctx = DeepCtx::new();
        let script = format!(
            r#"return target.path == {}"#,
            quote(&p_str)
        );
        assert!(evaluate_lua(&script, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn is_directory_field_set() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path().to_path_buf();
        let mut ctx = DeepCtx::new();
        let script = "return target.is_directory == true";
        assert!(evaluate_lua(script, &FileTarget::new(dir_path), &mut ctx));
    }

    #[test]
    fn non_bool_return_treated_as_false() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua("return 42", &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn syntax_error_returns_false() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua("this is not lua", &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn runtime_error_returns_false() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua(
            "error('boom')",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn infinite_loop_killed_by_instruction_cap() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        // 무한 루프 — cap 가 잡아내야 함. 못 잡으면 이 테스트는 timeout 으로 죽음.
        assert!(!evaluate_lua(
            "while true do end",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn memory_bomb_returns_false() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        // 64MB 문자열 시도 — memory cap 8MB 이므로 차단되어야.
        assert!(!evaluate_lua(
            "local s = string.rep('a', 64 * 1024 * 1024); return true",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn io_library_removed() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        // io 가 nil 이면 io.open 호출 시 attempt to index a nil value → runtime
        // error → false.
        assert!(!evaluate_lua(
            "return io.open('/etc/passwd') ~= nil",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn os_library_removed() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua(
            "os.execute('rm -rf /'); return true",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn debug_library_removed() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_lua(
            "return debug.getinfo(1) ~= nil",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    #[test]
    fn loadstring_removed() {
        let dir = TempDir::new().unwrap();
        let p = write_tmp(&dir, "x.txt", b"hello");
        let mut ctx = DeepCtx::new();
        // load/loadstring 둘 다 제거되어 nil → runtime error → false.
        assert!(!evaluate_lua(
            "return load('return true')() == true",
            &FileTarget::new(p),
            &mut ctx
        ));
    }

    /// Lua string literal escape.
    fn quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }
}
