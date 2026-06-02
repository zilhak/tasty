# Cargo Workspace 구조 — 현 28 crate 스냅샷

본 문서는 *현재* 워크스페이스 구조를 기록한 스냅샷이다. 옛 *2 crate 분리 후 설계안* 은 *이미 도달한 결과* 이상으로 확장됐다 (28 crate 도달). 옛 분리 의사결정 회고는 [`index.md`](index.md), 옛 실행 절차는 [`execution-plan.md`](execution-plan.md) 참조.

실측 시점: 2026-06-02 (`Cargo.toml` 기준).

---

## 1. 디렉토리 구조

```
tasty/
├── Cargo.toml                     # 워크스페이스 + 본 바이너리 [package]
├── src/                           # 본 바이너리 (479 .rs / ~91k LOC)
│   ├── main.rs
│   ├── app/  core/  adapters/  view/  gfx/  host_api/
│   ├── engine/  store/  intent/  state/  ports/
│   ├── boot/  file/  platform/  db/
│   └── ...                         # 모듈 상세는 ../index.md 의 "본 바이너리 모듈" 절
└── crates/                         # 28 라이브러리 크레이트
    ├── tasty-type-geometry/        # type-* layer (leaf)
    ├── tasty-type-appearance/
    ├── tasty-utils/
    ├── tasty-themes/               # 도메인-IO layer
    ├── tasty-settings/
    ├── tasty-font/
    ├── tasty-terminal/
    ├── tasty-hooks/
    ├── tasty-memory/
    ├── tasty-telemetry/
    ├── tasty-output/
    ├── tasty-approval/
    ├── tasty-agent/
    ├── tasty-presets/
    ├── tasty-shm/
    ├── tasty-portscan/
    ├── tasty-update/
    ├── tasty-lua/
    ├── tasty-plugin-protocol/      # Plugin layer
    ├── tasty-plugin-sdk/
    ├── tasty-plugin-claude/        # 번들 Plugin layer
    ├── tasty-plugin-codex/
    ├── tasty-plugin-explorer/
    ├── tasty-plugin-git-viewer/
    ├── tasty-plugin-clipboard-history/
    ├── tasty-plugin-image/
    ├── tasty-plugin-html/
    └── tasty-tui-simulator/        # 테스트/dev 도구
```

---

## 2. 워크스페이스 Cargo.toml 핵심

```toml
[workspace]
members = ["crates/*"]
resolver = "2"

# 워크스페이스 공통 lint (각 crate 의 [lints] 절에서 workspace = true 로 상속)
[workspace.lints.clippy]
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "warn"

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"

[package]
name = "tasty"
version = "0.6.0"
edition = "2024"
authors = ["zilhak <zilhak1@gmail.com>"]
license = "MIT"
```

본 바이너리 `[dependencies]` 절에서 28 crate 모두 `path = "crates/..."` 형태로 직접 의존:

```toml
[dependencies]
tasty-font            = { path = "crates/tasty-font" }
tasty-hooks           = { path = "crates/tasty-hooks" }
tasty-terminal        = { path = "crates/tasty-terminal" }
tasty-settings        = { path = "crates/tasty-settings" }
tasty-themes          = { path = "crates/tasty-themes" }
tasty-type-appearance = { path = "crates/tasty-type-appearance" }
tasty-type-geometry   = { path = "crates/tasty-type-geometry" }
tasty-utils           = { path = "crates/tasty-utils" }
tasty-memory          = { path = "crates/tasty-memory" }
tasty-telemetry       = { path = "crates/tasty-telemetry" }
tasty-output          = { path = "crates/tasty-output" }
tasty-approval        = { path = "crates/tasty-approval" }
tasty-agent           = { path = "crates/tasty-agent" }
tasty-presets         = { path = "crates/tasty-presets" }
tasty-shm             = { path = "crates/tasty-shm" }
tasty-portscan        = { path = "crates/tasty-portscan" }
tasty-update          = { path = "crates/tasty-update" }
tasty-lua             = { path = "crates/tasty-lua" }
tasty-plugin-protocol = { path = "crates/tasty-plugin-protocol" }
# 번들 plugin 7 종 + tasty-plugin-sdk + tasty-tui-simulator 도 동일 패턴
```

옛 권고였던 `[workspace.dependencies]` 공통 버전 절은 *미도입* — 현재 패턴은 *각 crate 가 자체 dep 버전 명시*. 워크스페이스 전체 통일 대상은 `[workspace.lints]` (clippy / rust) 만.

---

## 3. 카테고리별 Cargo.toml 발췌 (대표 4 종)

### type-\* layer 예: `tasty-type-appearance`

```toml
[package]
name = "tasty-type-appearance"
version = "0.1.0"
edition = "2024"

[features]
default = ["egui-compat"]
egui-compat = ["dep:egui"]   # plugin/headless 에서 default-features = false 로 제외

[dependencies]
tasty-type-geometry = { path = "../tasty-type-geometry" }
serde      = { version = "1", features = ["derive"] }
bytemuck   = { version = "1", features = ["derive"] }
egui       = { version = "0.32", optional = true }

[lints]
workspace = true
```

`egui-compat` feature 가 워크스페이스 전체에서 유일한 *실제 feature flag*. 옛 분석의 *다양한 feature 권고* (read-mark, serde-conversion 등) 는 *미도입* 상태.

### 도메인-IO 예: `tasty-settings` (옛 비권장 → 분리 반전)

```toml
[package]
name = "tasty-settings"
version = "0.1.0"
edition = "2024"

[dependencies]
tasty-themes          = { path = "../tasty-themes" }
tasty-type-appearance = { path = "../tasty-type-appearance", default-features = false }
tasty-type-geometry   = { path = "../tasty-type-geometry" }
tasty-utils           = { path = "../tasty-utils" }

serde  = { version = "1", features = ["derive"] }
toml   = "0.8"
tracing = "0.1"
anyhow  = "1"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[lints]
workspace = true
```

`tasty-type-appearance` 를 `default-features = false` 로 가져와 egui 비의존 — *plugin 에서 settings 도메인을 import 해도 egui 가 따라오지 않게* 격리. 옛 분석에 없던 layering 규칙.

### Plugin 호스트 예: `tasty-plugin-sdk`

```toml
[package]
name = "tasty-plugin-sdk"
version = "0.1.0"
edition = "2024"

[dependencies]
tasty-plugin-protocol = { path = "../tasty-plugin-protocol" }
tasty-shm             = { path = "../tasty-shm" }
serde_json = "1"
toml       = "0.8"
tracing    = "0.1"
anyhow     = "1"
thiserror  = "1"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[lints]
workspace = true
```

도메인-IO layer (`tasty-settings`, `tasty-themes` 등) *직접 의존 0* — plugin sandbox 경계 invariant 준수. 도메인 타입이 필요하면 *wire format* (`tasty-plugin-protocol`) 으로 전달.

### 번들 plugin 예: `tasty-plugin-claude`

```toml
[package]
name = "tasty-plugin-claude"
version = "0.1.0"
edition = "2024"

[dependencies]
tasty-plugin-sdk = { path = "../tasty-plugin-sdk" }
regex        = "1"
directories  = "5"
shell-escape = "0.1"
serde        = "1"
serde_json   = "1"
anyhow       = "1"
tracing      = "0.1"

[lints]
workspace = true
```

`tasty-plugin-sdk` 만 의존 — 번들 plugin 7 종 모두 동일 패턴.

---

## 4. 의존성 그래프 (4 계층)

```
┌─────────────────────────────────────────────────────────────────┐
│  본 바이너리 (tasty, src/)                                      │
└─┬───────────────────────────────────────────────────────────────┘
  │
  ├── Plugin layer
  │     ├─ tasty-plugin-sdk ── tasty-plugin-protocol
  │     │                  └── tasty-shm  (도메인-IO 중 OS 의존 0 항목 한정)
  │     └─ 번들 plugin 7 종 ── tasty-plugin-sdk
  │
  ├── 도메인-IO layer
  │     ├─ tasty-settings ── tasty-themes ── tasty-type-appearance
  │     │                                └── tasty-utils
  │     ├─ tasty-themes ── tasty-type-appearance ── tasty-type-geometry
  │     │              └── tasty-utils
  │     ├─ tasty-agent ── tasty-memory
  │     │              └── tasty-utils
  │     ├─ tasty-telemetry ── tasty-memory
  │     ├─ tasty-presets ── tasty-utils
  │     ├─ tasty-font / tasty-terminal / tasty-hooks / tasty-output /
  │     │  tasty-approval / tasty-portscan / tasty-update / tasty-lua /
  │     │  tasty-shm ── (모두 외부 deps 만, 워크스페이스 내 의존 0)
  │     └─ ...
  │
  └── type-\* layer (leaf)
        ├─ tasty-type-geometry  ← (외부 deps 만: serde)
        ├─ tasty-type-appearance ← tasty-type-geometry (+ optional egui)
        └─ tasty-utils ← directories
```

### 의존 invariant

- **type-\* 끼리만 의존 가능** (type-appearance → type-geometry). 도메인/IO/plugin/본 바이너리 → type-\* OK. 역방향 ❌.
- **plugin → 도메인-IO 직접 의존 금지** — `tasty-plugin-sdk` / `tasty-plugin-protocol` 통과 필수 (sandbox 경계). `tasty-shm` 만 plugin-sdk 가 직접 import (handle 전달용 OS primitive).
- **번들 plugin → tasty-plugin-sdk 만** 의존 — 도메인-IO 직접 의존 금지.
- 본 바이너리 → 모든 layer OK.

순환 의존 0. 4 계층 invariant 위반 0.

---

## 5. Feature flags 현황

| 위치 | feature | 효과 |
|------|---------|------|
| `tasty-type-appearance` | `egui-compat` (default ON) | `HexColor::to_egui()` 등 egui Color32 변환. plugin/headless 에서 `default-features = false` 로 제외 |
| (그 외 워크스페이스 crate) | — | feature flag 미도입 |

옛 분석의 권고였던 `tasty-hooks` 의 `serde` feature / `tasty-terminal` 의 `read-mark` feature 등은 *미도입* 상태. 도메인-IO crate 들이 *대부분 모놀로식 의존* 으로 사용되고 있어 feature gate 필요성 미발생.

---

## 6. 인접 문서

| 문서 | 설명 |
|------|------|
| [`index.md`](index.md) | 28 crate 현황 매트릭스 + 옛 8 후보 도달 상태 |
| [`execution-plan.md`](execution-plan.md) | Phase 별 완료 회고 + 미완 Phase trigger 재정의 |
| [`../index.md`](../index.md) | 워크스페이스 / 모듈 / 의존성 DAG 권위본 |
