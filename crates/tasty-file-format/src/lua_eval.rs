//! host/user의 Lua detector를 새 VM에서 평가한다. 플러그인 Lua는 등록 단계에서 제거한다.
//! 메모리·명령어 수를 제한하고 파일·OS 실행·동적 로더·debug 접근을 제거한다.
//! ChunkMode::Text로 bytecode도 거절한다. VM은 호출마다 만들며 캐시하지 않는다.

use mlua::{ChunkMode, Lua, Table, Value};

use super::evaluator::DeepCtx;
use super::types::FileTarget;

/// 한 평가의 명령어 상한. 초과하면 무한 루프 여부와 무관하게 평가를 중단한다.
pub const INSTRUCTION_BUDGET: u32 = 1_000_000;

/// 메모리 cap (bytes). string.rep 폭발 등 메모리 폭주 차단.
pub const MEMORY_BUDGET: usize = 8 * 1024 * 1024;

/// true일 때만 매칭한다. 파싱·실행·한도 오류와 bool이 아닌 결과는 warn을 남기고 false로 처리한다.
pub fn evaluate_lua(script: &str, target: &FileTarget, ctx: &mut DeepCtx) -> bool {
    match try_evaluate(script, target, ctx) {
        Ok(Value::Boolean(b)) => b,
        Ok(other) => {
            tracing::warn!(
                "file_format lua: script returned non-bool ({}), treating as false",
                other.type_name(),
            );
            false
        }
        Err(e) => {
            tracing::warn!("file_format lua: {e}");
            false
        }
    }
}

/// 초기화·target 생성·실행 중 실패한 단계를 오류에 담는다. evaluate_lua가 한 번 기록한다.
fn try_evaluate(script: &str, target: &FileTarget, ctx: &mut DeepCtx) -> mlua::Result<Value> {
    let entry = ctx.entry(target).clone();
    let lua = build_sandboxed_lua()
        .map_err(|e| mlua::Error::external(format!("sandbox init failed: {e}")))?;
    let target_table = build_target_table(&lua, target, &entry)
        .map_err(|e| mlua::Error::external(format!("target table build failed: {e}")))?;
    lua.globals()
        .set("target", target_table)
        .map_err(|e| mlua::Error::external(format!("failed to set target global: {e}")))?;
    let chunk = lua
        .load(script)
        .set_name("detector-rule")
        .set_mode(ChunkMode::Text);
    chunk
        .eval::<Value>()
        .map_err(|e| mlua::Error::external(format!("eval failed: {e}")))
}

fn build_sandboxed_lua() -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.set_memory_limit(MEMORY_BUDGET)?;

    // 표준 라이브러리에서 위험한 전역을 제거하고 string/math/table 등 계산 기능은 남긴다.
    let g = lua.globals();
    for name in &[
        "dofile",
        "loadfile",
        "load",
        "loadstring",
        "debug",
        "require",
        "io",
        "os",
    ] {
        g.set(*name, Value::Nil)?;
    }
    // require는 이미 제거했고 바로 뒤에서 package도 제거하므로 개별 set 실패가 로더 접근을 남기지 않는다.
    if let Ok(pkg) = g.get::<Table>("package") {
        let _ = pkg.set("loadlib", Value::Nil); // package도 제거하므로 실패해도 로더에 접근할 수 없다.
        let _ = pkg.set("searchers", Value::Nil); // package도 제거하므로 실패해도 로더에 접근할 수 없다.
        let _ = pkg.set("loaders", Value::Nil); // package도 제거하므로 실패해도 로더에 접근할 수 없다.
    }
    g.set("package", Value::Nil)?;

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

    let head_clone = entry.head.clone();
    let has_prefix = lua.create_function(move |_, prefix: mlua::String| {
        let bytes = prefix.as_bytes();
        Ok(head_clone
            .as_ref()
            .map(|h| h.starts_with(&bytes))
            .unwrap_or(false))
    })?;
    t.set("has_prefix", has_prefix)?;

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::DeepCtx;
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
        let script = format!(r#"return target.path == {}"#, quote(&p_str));
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
        assert!(!evaluate_lua(
            "this is not lua",
            &FileTarget::new(p),
            &mut ctx
        ));
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
