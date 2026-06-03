# Plugin Sandbox 평가

이 문서는 Tasty plugin 의 sandbox 옵션 (WASM / OS-level / 현 상태) 을 비교 평가한
결정 근거다. 1.0 까지는 현 상태 유지가 결론이며, 본문은 그 trade-off 의 *현재 상태* 와
*재검토 trigger* 를 한 곳에 기록해 미래에 trigger 발동 시 어떤 비용을 감수했는지를
참조할 수 있게 한다.

본문은 *현재 상태만* 기술한다. 비용 추정은 *측정 없는 추정* 임을 본문 내에서 표기한다.
외부 URL 은 적지 않는다 (`wasmtime` / `landlock` 등 명칭만).

## 0. TL;DR

- **WASM 채택은 1.0 보류 (재확인).** `dev-guide/plugin-ecosystem.md §1` 결정과 일치.
- **OS-level sandbox** (macOS `sandbox-exec` / Linux `seccomp` + `landlock` /
  Windows `AppContainer`) 가 cost-effective 한 대안. 단 *opt-in* 모델 (매니페스트 토글)
  로만 가능 — plugin 작성자가 자기 plugin 의 sandbox 호환성을 검증한 뒤에만 활성화.
- 매니페스트의 `sandbox = "process" | "os-strict" | "wasm"` enum 은 *제안만*. 추가 X.
  기본값 = `"process"` 로 기존 매니페스트 무변경. 정식 도입은 schema bump (예:
  `manifest_version = 2`) 시점으로 미룬다.
- 재검토 trigger 4 항목 명문화 (§2.4).
- POC (구현 spike) 는 수행하지 않는다. spike 비용 추정만 남긴다.

> WASM 의 가치는 *경량성* 이 아니라 *강제 가능한 sandbox*. 비용 (FFI 부담 + debug
> 인프라 + FS access 우회 모델 + 동적 권한 prefix 의 정적 capability 매핑) 이 1.0 전
> 부담 한도를 초과하므로 보류한다.

## 1. 현재 한계 (factual)

### 1.1 권한 게이트의 범위

`crates/tasty-plugin-manifest/src/types.rs:92 pub enum Permission` 의 모든 variant 는
*호스트가 제공하는 IPC method 단위 게이트* 다. plugin 이 자기 프로세스 내부에서
`std::fs::*` / `std::net::*` / `Command::new` 를 직접 호출하는 것은 OS process
privilege 로만 결정되며 호스트는 알 수 없다. `dev-guide/plugin-permissions.md` 의
「한계」 절과 동일한 명제.

권한 enum 의 *정적 variant* (현재 단순 토큰):

```
surface.read/write, clipboard.read/write, fs.read/write,
terminal.spawn/write/read, process.spawn, window.spawn,
memory.read/write/secret, notification, network,
ui.popup, ui.tool_item, approval, telemetry, agent, file_handler.define
```

권한 enum 의 *동적 prefix variant* (install 시 매니페스트 id 로 token 이 결정됨):

```
file_handler.extend:<id>      # 기존 detector 에 rule 추가
file_handler.handle:<id>      # 특정 detector 에 handler attach
```

(`types.rs` L155 `WindowSpawn`, L162 `FileHandlerExtend(String)`, L165
`FileHandlerHandle(String)`.)

이 enum 의 모든 항목은 IPC 게이트용이므로 *OS resource 직접 접근* 에 대한 enum
variant 는 없다 — 있을 수 없다 (OS 권한이 아니므로).

### 1.2 plugin spawn 실제 구현

`crates/tasty-host-plugin/src/process.rs::PluginProcess::spawn` (L83~156) 은
`std::process::Command::new(entry_path).spawn()` 으로 plugin 을 OS process 로 직접
띄운다. env 주입 (`TASTY_PLUGIN_ID`, `TASTY_HOST_IPC_PORT`, `TASTY_PLUGIN_TOKEN` 등,
L97~114) 외 sandbox wrapper 는 없다. UID/GID drop / seccomp / sandbox-exec / Job
Object 모두 없다.

호스트는 `~/.tasty/plugin-data/<id>/` 와 `~/.tasty/plugin-config/<id>.toml` 디렉터리
*생성* 만 미리 보장한다 (L130~146) — plugin 의 FS access 자체는 미제한.

### 1.3 실제 신뢰 모델

현재 동봉 plugin 7 개 (`tasty-plugin-claude`, `tasty-plugin-codex`,
`tasty-plugin-image`, `tasty-plugin-explorer`, `tasty-plugin-clipboard-history`,
`tasty-plugin-html`, `tasty-plugin-git-viewer`) 는 모두 first-party 다 (저자 = Tasty
팀). 외부 plugin 0 개. 적대적 plugin scenario 가 *현실화되지 않음*. 즉 sandbox 가
*지금* 필요한 정도는 낮다.

### 1.4 기존 sandbox 가 *있는* 영역 (참고)

| 영역 | sandbox 종류 | 강도 |
|------|--------------|------|
| Lua hooks (`crates/tasty-lua`, `docs/design/lua-hooks.md`) | mlua + 메모리 cap + 위험 글로벌 제거 | 약 (사용자 자기 머신, DoS 보호 한정) |
| Lua plugin 출처 | install 시 drop + warn (`system` 사용 금지 serde reject) | 거부 |

즉 *plugin 본체 sandbox 는 0* 이며 Lua 만 약한 격리를 갖는다.

## 2. WASM 옵션 평가

### 2.1 후보 런타임

| 런타임 | 상태 | 비고 |
|--------|-----|------|
| wasmtime (Bytecode Alliance) | 활성, Rust-native, WASI Preview 2 | host embedding API 성숙 |
| wasmer | 활성, multi-backend | Rust API 변경 잦음 |
| wasmi | 인터프리터, no JIT | 임베디드용. plugin host 부적합 |

### 2.2 가치

- **강제 가능한 sandbox**: capability-based. plugin 이 host 가 명시 inject 한 import
  외에는 자원 접근 0.
- FS access / network / process spawn 등 OS resource 를 모두 import 경유 → 권한 모델
  을 *시스템 권한* 까지 확장 가능.

### 2.3 비용 — 1.0 전 부담 한도 초과 근거

| 비용 항목 | 추정 (측정 없음) |
|-----------|------|
| **FFI 부담** | wasmtime ↔ host 데이터 전송이 *복사 기반* (linear memory). 현재 `tasty-shm` shared memory + handle channel 모델과 충돌. 큰 surface buffer / 이미지 transfer 가 비효율적 |
| **debug 인프라** | wasm32 target rustc cross-compile + source map + DWARF in wasm. plugin 디버깅 도구체인 (`tasty plugin logs` 등) 재설계 필요 |
| **FS access 우회** | WASI preopen 디렉터리 패러다임은 `~/.tasty/plugin-data/<id>/` 같은 정해진 경로엔 적합하나, 임의 경로 (사용자 home, project root) 가 필요한 explorer/git-viewer 류는 preopen 명시 + 사용자 grant 흐름 신설 필요 |
| **process.spawn / window.spawn** | WASM 안에서 OS process spawn / winit window 생성 불가능 → host import 로 우회. claude / codex plugin 의 PTY child 모델이 *모두* trampoline |
| **빌드 / 배포 변화** | 모든 plugin 이 `cargo build --target wasm32-wasi-preview2` 필요. binary size 증가, cold-start 증가 |
| **i18n / asset loader** | 현재 매니페스트 `lang_dir` 는 OS FS path. WASM 에서는 plugin bundle 내장 또는 host inject 로 재설계 필요 |
| **동적 권한 prefix → 정적 capability 매핑** | `file_handler.extend:<id>` / `file_handler.handle:<id>` 는 install 시 매니페스트 id 로 token 이 동적 결정됨. WASM capability inject 는 *정적* (host 가 startup 에 import 결정). 변환 layer 또는 schema 재설계 필요 |

위 비용은 모두 *추정* 이다. 실측은 POC spike (수행하지 않음) 에서만 가능.

### 2.4 1.0 이후 재검토 trigger

`dev-guide/plugin-ecosystem.md §1` 재검토 trigger 재확인 + 보강:

- 외부에서 WASM 또는 비-Rust plugin 작성 요청 2 건 이상.
- 권한 게이트 한계에서 비롯한 보안 이슈 1 건.
- 첫 외부 plugin 출시 직후 1 년 운영 데이터.
- **신규**: marketplace 도입 (비-trusted plugin 일상화) — `plugin-ecosystem.md §2`
  marketplace trigger 와 연동.

## 3. OS-level sandbox 옵션 평가 (대안)

### 3.1 플랫폼별 매핑

| OS | 메커니즘 | 강도 | 비용 |
|----|----------|------|------|
| macOS | `sandbox-exec` (Apple 이 deprecated 표시했으나 실행 가능) / Seatbelt profile | FS deny + network deny + mach port deny | profile DSL 작성 부담 中. endpoint security framework 로 교체 가능성 모니터 필요 |
| Linux | `seccomp-bpf` + `landlock` + namespace unshare | syscall filter + FS access policy | `landlock` 이 path-level 격리의 현대 메커니즘. `seccomp` 만으로는 path 격리 불가 → 둘 결합 필수 |
| Windows | `AppContainer` + `Job Object` (`UI restrictions`) | capability SID + token restriction | install + spawn 시 capability SID 부여. 권한 grant 흐름 신설 필요. WinSDK 의존도 ↑ |

`sandbox-exec` 의 deprecation 상태는 시점 의존 정보다 (Apple 의 표시일 뿐 실행은 됨).
연도/macOS 버전 단정은 stale 되므로 본문에 적지 않는다.

### 3.2 가치

- **FFI 부담 0** — plugin 은 여전히 native OS process. host ↔ plugin IPC 는 현재 TCP +
  `tasty-shm` 그대로.
- **기존 plugin 코드 무변경** — sandbox profile 만 호스트가 spawn 시 wrapping.
- **opt-in 가능** — plugin 작성자가 자기 plugin 동작 검증 후 매니페스트에 `sandbox =
  "os-strict"` 선언.

### 3.3 비용

| 항목 | 추정 (측정 없음) |
|------|------|
| 플랫폼별 분기 코드 | `tasty-host-plugin/src/process.rs::spawn` 에 3 OS 분기 추가 (~50 LOC + util) |
| sandbox profile DSL | macOS SBPL / Linux syscall list / Windows capability SID list — 3 종 메인테인 부담 |
| 권한 ↔ syscall 매핑 | 매니페스트 권한 (`fs.read`, `network` 등) → 각 OS sandbox primitive 매핑 테이블 필요 |
| plugin 작성자 가이드 | "내 plugin 이 sandbox 호환인지 검증" 절차 신설 (CI matrix, dry-run 등) |
| 정책 진화 추적 | macOS `sandbox-exec` 의 Apple deprecation 표시 — endpoint security API 교체 시점 모니터 필요 |

### 3.4 매니페스트 제안 (구현 X — 평가만)

```toml
[entry]
type = "process"
command = "tasty-plugin-foo"

# (제안) sandbox 모드 — 기본 "process" (현재 동작). 추가 X.
# sandbox = "process"        # 기본값 (현재 동작, sandbox 0, OS user 권한)
# sandbox = "os-strict"      # macOS sandbox-exec / Linux landlock+seccomp / Windows AppContainer
# sandbox = "wasm"           # 장기 옵션 — 별도 entry type 으로 분리될 가능성 있음
```

이 필드는 *제안만* 이다. Phase H 에서 추가하지 않는다. 기본값 = `"process"` 로 기존
매니페스트 무변경 호환. 정식 도입 시 schema bump 가 필요하며, 그 시점은 `manifest_version
= 2` 로 미룬다. 정식 도입 시 `plugin-ecosystem.md §1` 갱신 + manifest schema 확장.

## 4. 비교 표 (의사결정용)

| 항목 | WASM (wasmtime) | OS-level sandbox | 현 상태 (process, no sandbox) |
|------|-----------------|------------------|------------------------------|
| FS deny | 강제 가능 (capability) | 강제 가능 (profile) | 불가 (OS user 권한) |
| network deny | 강제 가능 | 강제 가능 (대체로) | 불가 |
| process spawn | 강제 deny | 강제 deny | 가능 (OS user 권한) |
| FFI 부담 | 中 ~ 大 (linear memory 복사) | 0 (native process 유지) | 0 |
| debug 도구체인 변경 | 필요 (wasm32 target) | 필요 없음 | 필요 없음 |
| 1.0 까지 도입 비용 | 大 | 中 | 0 |
| 1.0 *이후* 점진 도입 | 별도 entry type 필요 | opt-in 토글 가능 | 기본값 유지 |
| plugin 코드 호환성 | wasm32 재컴파일 필요 | 무변경 (sandbox 호환 검증 필요) | 현 상태 |

이 표는 단독 열람으로도 *yes/no/maybe* 결정이 가능하도록 self-contained 하게 구성했다.
TL;DR (§0) + 비교표 (§4) + 재검토 trigger (§2.4) 셋만 보면 미래에 sandbox 가 필요한지
판단 가능.

## 5. 권고 (재확인)

- **1.0 까지**: 현 상태 유지. `dev-guide/plugin-ecosystem.md §1` 결정 재확인.
- **1.0 이후 (조건부)**: OS-level sandbox 를 *opt-in* 으로 우선 (FFI 부담 0). WASM 은
  marketplace / 외부 plugin 자생 이후 별도 entry type 으로.
- **명문화 추가**: 본 평가 문서가 그 자체로 *결정 근거의 기록*. 향후 trigger 발동
  시 어떤 비용 trade-off 를 평가했는지의 참조점.

## 6. 출처

- `docs/dev-guide/plugin-ecosystem.md` §1 (WASM 보류 결정).
- `docs/dev-guide/plugin-permissions.md` 「한계」 절 (권한 게이트 = IPC 호출만).
- `docs/agent-guide/plugins.md` (entry type 정의 + 「한계」 절).
- `docs/design/memory-system.md` (Plugin sandbox 부재 + 미래 경로).
- `docs/architecture/index.md` (sandbox 경계 invariant, plugin layer 의존 규칙).
- 코드: `crates/tasty-host-plugin/src/process.rs::PluginProcess::spawn` (실제 spawn).
- 코드: `crates/tasty-plugin-manifest/src/types.rs::Permission` (권한 enum + 동적
  prefix variant).
- 참고: `crates/tasty-lua` (Lua sandbox — mlua + 메모리 cap).
