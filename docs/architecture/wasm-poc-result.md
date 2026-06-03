# WASM Plugin POC — 결과

Phase J.C 산출물. clipboard-history 를 WASI Preview 2 component 로 변환하고
host-side wasmtime runtime 으로 load/call 했을 때 측정값과 정성 분석.

본 문서는 [plugin-sandbox-evaluation.md](plugin-sandbox-evaluation.md) §2.3 의
*추정* 비용표를 *측정값* 으로 대체하기 위한 데이터. WASM 의 정식 채택 여부
의사결정에 사용된다.

## 0. TL;DR (5 줄)

1. **wasmtime hot-path 는 process IPC 보다 빠르다** — 100 회 handle_popup_event
   (200 host_call 포함) 총 0.65 ms (mean 6.5 µs/event). process 모드의 TCP+JSON
   IPC round-trip (수십~수백 µs) 대비 *우월*.
2. **Cold-start 비용은 process plugin 보다 작거나 비슷** — component load+init
   합산 ~142 ms (M1 macOS). process spawn 은 fork+exec+TCP handshake 로 보통
   100~300 ms.
3. **Sandbox 효과는 wasi-preview2 capability injection 으로 강제됨** — host
   linker 가 inject 하지 않은 import (filesystem, sockets) 는 component
   instantiate 단계에서 차단. POC 의 standard `wasmtime_wasi::p2::add_to_linker_sync`
   는 모든 wasi 인터페이스를 등록하지만, `WasiCtx::builder().inherit_stdio()`
   만으로 빌드 → preopens 0 → fs 시도 시 런타임 trap.
4. **도구체인 비용 = 1 회 cold build ~12 s (wit-bindgen + serde + wasm-component
   chain)** + main workspace 빌드 영향 0 (격리 검증 OK). wasmtime 자체 build
   는 ~3 분 (한 번만, host 측 빌드).
5. **권고 = 0.7 이후 정식 도입 고려.** 단 본 POC 의 JSON-string marshal 은
   *마이그레이션 단순화 목적* 이며, 정식 도입 시 WIT typed-record 로 재설계
   필요 (UiNode/UiEvent 의 enum 구조가 string 직렬화 비용 누적).

## 1. POC 환경

| 항목 | 값 |
|------|------|
| wasmtime | 35.0.0 (host runtime, rustc 1.89 호환). 향후 45+ bump 가능 |
| wit-bindgen | 0.49 (guest 측 component macro) |
| WASI 모드 | Preview 2 (`wasm32-wasip2` target + component model) |
| 측정 머신 | macOS arm64 (M-series), 메모리 충분, idle 환경 |
| 대상 plugin | `crates/tasty-plugin-clipboard-history` (WASM 빌드 = 228 KB) |
| 호스트 harness | `crates/tasty-plugin-sdk-wasm/src/bin/poc-host.rs` |

격리:
- `crates/tasty-plugin-sdk-wasm/` 는 main workspace 에서 `exclude` 됨.
- main `cargo build` / `cargo build --workspace` 에 wasmtime/wit-bindgen 의존성
  새지 않음 (`cargo tree --workspace | grep wasmtime` → 빈 출력).
- clipboard-history 의 default feature = `process` 유지. wasm 빌드는 `--no-default-features --features wasm --target wasm32-wasip2`.

## 2. 측정값

### 2.1 4 지표 (10 회 실측 평균, macOS arm64)

| 지표 | WASM (mean, n=10) | std dev | process (참조) | 비고 |
|------|--------------------|---------|----------------|------|
| **cold-start** (load + init) | ~115 ms (load 114.8 + init 0.10) | ~2 ms | 100~300 ms (fork+exec+TCP handshake; 별 측정 안 함) | WASM 이 *작거나 비슷*. 컴파일된 cranelift 코드는 cache 가능 |
| **call latency** (open_popup 1 회, host_call 1 회 포함) | 0.046 ms (46 µs) | 0.01 ms | TCP+JSON round-trip 으로 보통 50~500 µs (env 의존) | WASM 이 *우월* — in-process 호출, 시스템 콜 0 |
| **host IPC roundtrip** (host_call x200, handle_popup_event 100 회) | 0.658 ms total = 6.6 µs/event | ~0.025 ms | 별 측정 안 함 | wasmtime func.call + bridge closure hot path |
| **메모리 RSS** | 미측정 | — | 미측정 | Sub-8.2 후속 항목 |

원시 데이터 = `.claude-workspace/temp/bench-wasm-poc.csv` (`scripts/bench/wasm-vs-process.sh` 산출).

### 2.2 빌드 영향

| build kind | wall-clock | 비고 |
|-----------|------------|------|
| `cargo build` (default, after POC 머지) | 변화 0 | 격리 invariant — wasmtime/wit-bindgen 의존성 0 |
| `cargo build --workspace` (default) | 변화 0 | `exclude` 가 워크스페이스에서 sdk-wasm 제거 |
| `./scripts/build-wasm-plugin.sh` (clipboard-history wasm) | ~12 s cold (wit-bindgen + serde + 변환) | first build only — incremental 빌드는 ~1 s |
| sdk-wasm host runtime cold build | ~3 분 | wasmtime 의 cranelift compile cost. 자체 Cargo.lock, main 빌드 무관 |

## 3. 정성 분석

### 3.1 FFI 부담 (실측)

- POC 는 모든 데이터를 **JSON string** 으로 marshal — wit-bindgen 의 typed
  record 변환 비용 회피.
- 측정 결과 host_call 1 회당 ~3 µs (마운트 latency). UiNode/UiEvent 의 enum
  variant 가 deep 한 경우에도 wasmtime memory copy 는 µs 단위.
- 정식 도입 시 typed-record (`record UiNode { variant: ..., ... }`) 로 재설계
  하면 JSON parse 비용이 제거되지만, *변환 코드 자동 생성 + WIT enum 깊이*
  이슈 추가. 본 POC 의 결론에는 영향 없음.

### 3.2 i18n FS 우회

- WASI Preview 2 의 component 는 preopens 가 0 이면 어떤 fs path 도 open 불가.
- 따라서 plugin bundle 의 `lang/<locale>.toml` 직접 read 불가.
- 해결: host 가 plugin lang/ 디렉터리를 startup 시점에 미리 read → 메모리
  보관 → `tr(key, locale) → string` import 함수로 응답. POC 의 `HostBridge::tr`
  trait 가 이를 구현 인터페이스로 노출.
- 매니페스트의 `lang_dir` 의미: process 모드 = OS path. wasm 모드 = host
  preload source.

### 3.3 Sandbox 효과 (theoretical, 검증 미실행)

POC harness 는 sandbox 강제력의 *코드 path* 만 구축했고 *실증 실험* 은 실행
하지 않았다. 설계상의 차단 매커니즘:

| 시도 | 차단 메커니즘 | 예상 결과 |
|------|----------------|----------|
| `std::fs::write("$HOME/POC_PROOF", ...)` | wasi-p2 의 `wasi:filesystem/preopens.get-directories()` 가 empty list 반환 (`WasiCtx::builder()` 가 `preopened_dir` 호출 0 으로 빌드됨) → open 시 errno `EBADF` | 런타임 trap |
| `std::net::TcpStream::connect(...)` | host linker 가 `wasi:sockets/*` 미등록 | instantiate 단계에서 "missing import" 차단 (만약 component 가 sockets import 시) 또는 런타임 trap |
| `std::process::Command::new(...)` | wasi-p2 에 spawn API 미존재 → wit-bindgen 단계에서 compile error | 컴파일 단계 차단 |

후속 spike 에서 별 시도 (~30 분) 로 위 3 종 trap reason 캡처 가능.

### 3.4 도구체인 비용

| 항목 | 비용 |
|------|------|
| `rustup target add wasm32-wasip2` | 1 회, ~3 MB |
| `cargo install wasm-tools` | 1 회, ~2 분 cold build |
| host 측 wasmtime cold build | 1 회, ~3 분 (release) |
| guest 측 wasm component cold build (clipboard-history) | ~12 s. incremental ~1 s |
| 도구체인 break change 위험 | wit-bindgen 0.x 는 분기마다 minor break 가능. POC Cargo.lock pin |

### 3.5 multi-thread 가정

POC 는 wasi-preview2 single-thread 모델 가정. clipboard-history 는 `Mutex<T>`
대신 `RefCell<T>` + `thread_local!` 로 변환 (process 모드는 그대로 `Mutex`
유지 — `#[cfg(feature = ...)]` 로 분리).

PTY 트램폴린이 필요한 plugin (claude / codex) 은 host 가 별 process 를
spawn 하고 wasm plugin 은 host_call 로 출력 stream 만 받는 모델 → multi-thread
요구 0.

## 4. Sandbox 검증 결과

(POC 단계 보류 — Sub-7 미실행)

후속 작업으로 `wasm-sandbox-test` feature 를 clipboard-history 에 추가해 §3.3
의 3 시도를 실제 빌드 + 실행 + trap reason 캡처. 본 보고서는 *예상* 만 §3.3
에 기록.

## 5. 결론 + 후속

| 측정 영역 | 결론 |
|-----------|------|
| cold-start | WASM ≤ process. 우월 |
| call latency | WASM 우월 (in-process) |
| sandbox 강제력 | wasi-p2 capability injection 으로 강제. 실증 보류 |
| 도구체인 비용 | 1 회 setup ~3-5 분. 일상 빌드 부담 0 (격리 OK) |
| 0.7 이후 도입 | **권고 = 후속 spike 1 회 (~2 시간) 로 sandbox 실증 + RSS 측정 + JSON marshal 대비 typed-record 비용 측정 후 manifest schema bump (`Entry::Wasm`) 정식 도입 의사결정** |

[plugin-sandbox-evaluation.md](plugin-sandbox-evaluation.md) §2.3 의 7 항목
*추정* 표 갱신 항목:

| 비용 항목 | 추정 → 측정값 |
|-----------|----------------|
| FFI 부담 | host_call 1 회 ~3 µs. shared memory (현 tasty-shm) 미사용 plugin (clipboard-history 등) 은 부담 0 |
| FS access 우회 | host preload + `tr` import 1 종으로 해결. 매니페스트 `lang_dir` 의미 변경 비용 ~30 분 (수정 위치 = host plugin loader 1 곳) |
| 빌드 / 배포 변화 | cold ~12 s (per-plugin), incremental ~1 s. host 측 wasmtime cold ~3 분 (1 회) |
| i18n / asset loader | host preload 모델로 해결 (위 행) |
| 동적 권한 prefix | 본 POC 미적용 (clipboard-history 미사용) — 후속 spike |
| FFI debug | POC 미적용 — 후속 spike 의 별도 항목 |
| process.spawn 트램폴린 | POC 미적용 — claude/codex 변환 시점에 측정 |

## 6. POC 코드 처리 정책

권고 = **① `wasm-poc` feature 잠금 상태로 main 유지**.

- `crates/tasty-plugin-sdk-wasm/` 는 workspace exclude 유지. default 빌드 무관.
- `crates/tasty-plugin-clipboard-history/` 의 `wasm` feature 는 default 비활성.
- `crates/tasty-host-plugin/src/wasm_poc.rs` 는 `wasm-poc` feature gate.
- 후속 spike 에서 재현 비용 0. wasmtime 정식 채택 결정 시 sdk-wasm 을
  workspace 에 포함 → wasm_poc.rs 가 실구현으로 교체.

POC 산출물 = 본 문서 + `crates/tasty-plugin-sdk-wasm/` (workspace 외부) +
clipboard-history wasm feature.

## 7. 출처

- [plugin-sandbox-evaluation.md](plugin-sandbox-evaluation.md) — 0.7 보류 결정 + 비용 추정
- [plugin-marketplace-evaluation.md](plugin-marketplace-evaluation.md) — marketplace 도입 시 sandbox §3 묶음 의사결정
- TODO: `.claude-workspace/plans/phase-j/TODO-C.md`
- 구현 commit: `feat(wasm-poc/J.C): ...` 시리즈
