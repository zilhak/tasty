//! POC harness: standalone host process that loads clipboard-history.component.wasm
//! and exercises the lifecycle exports.
//!
//! 실행:
//!   ./scripts/build-wasm-plugin.sh
//!   cargo run --release --manifest-path crates/tasty-plugin-sdk-wasm/Cargo.toml \
//!       --bin poc-host -- target/poc/clipboard-history.component.wasm
//!
//! 본 바이너리는 *bench harness* 라 출력 라인이 spec. tracing 대신 writeln! 로
//! 직접 stdout 작성 (bench script 가 grep 으로 파싱).

use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use tasty_plugin_sdk_wasm::{WasmPluginRuntime, bridge::StubBridge};

fn main() -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();

    let args: Vec<String> = env::args().collect();
    let component_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/poc/clipboard-history.component.wasm".into());
    let path = PathBuf::from(&component_path);
    writeln!(out, "=== Tasty WASM POC host ===")?;
    writeln!(out, "component: {}", path.display())?;
    if !path.exists() {
        anyhow::bail!("component not found. run ./scripts/build-wasm-plugin.sh first");
    }

    let bridge = Arc::new(StubBridge::default());
    let bridge_dyn: Arc<dyn tasty_plugin_sdk_wasm::HostBridge + Send + Sync> = bridge.clone();

    let t_load = std::time::Instant::now();
    let mut rt = WasmPluginRuntime::load(&path, bridge_dyn)?;
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;
    writeln!(out, "load: {load_ms:.2} ms")?;

    let t_init = std::time::Instant::now();
    rt.init("com.tasty.clipboard-history", "ko-KR")?;
    writeln!(
        out,
        "init: {:.2} ms",
        t_init.elapsed().as_secs_f64() * 1000.0
    )?;

    let ctx = r#"{"instance_id": 1, "popup_id": "viewer"}"#;
    let t_open = std::time::Instant::now();
    let open_out = rt.open_popup(ctx)?;
    writeln!(
        out,
        "open_popup (first, host_call 포함): {:.3} ms",
        t_open.elapsed().as_secs_f64() * 1000.0
    )?;
    writeln!(
        out,
        "  tree (truncated): {}",
        &open_out.chars().take(200).collect::<String>()
    )?;

    // remove 이벤트로 host_call → fetch_and_render 트리거. host_call 2 회 / event.
    let remove_evt = r#"{"instance_id": 1, "event": {"type": "Click", "node_id": "remove-0"}}"#;
    let n = 100;
    let t = std::time::Instant::now();
    for _ in 0..n {
        // 반환 트리는 bench 측정 목적상 폐기 — 시간만 측정.
        rt.handle_popup_event(remove_evt)?;
    }
    let total_ms = t.elapsed().as_secs_f64() * 1000.0;
    writeln!(
        out,
        "handle_popup_event x{n} (host_call x{}회): total {total_ms:.3} ms, mean {:.4} ms",
        n * 2,
        total_ms / n as f64
    )?;

    writeln!(out)?;
    writeln!(out, "=== StubBridge logs (host_call / log) ===")?;
    // 이유: 단일 스레드 poc 호스트라 poison 이 생길 수 없다 — 조용히 버려도 안전하다.
    if let Ok(g) = bridge.logs.lock() {
        for (i, (lvl, msg)) in g.iter().take(10).enumerate() {
            writeln!(out, "  [{i}] {lvl} {msg}")?;
        }
        if g.len() > 10 {
            writeln!(out, "  ... ({} more)", g.len() - 10)?;
        }
    }

    Ok(())
}
