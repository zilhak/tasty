# 실행 계획 — 완료 회고 + 미완 권고

본 문서는 옛 *분리 계획 실행 단계 명세* (2026-03 시점) 를 **완료/미완 회고** + **미분리 항목 trigger 재정의** 로 재작성한 것이다. 현황 매트릭스는 [`index.md`](index.md), 워크스페이스 구조는 [`workspace-design.md`](workspace-design.md) 참조.

---

## Phase 1: tasty-hooks 분리 — **완료**

`src/hooks.rs` (옛 290 LOC) → `crates/tasty-hooks/src/lib.rs` 이동 완료.

- **현재 상태**: 344 LOC. `regex` / `serde` / `tracing` 외부 의존만. 워크스페이스 내 다른 crate 의존 0.
- **회고**: 옛 분석의 *5 분 분리* 예측보다 시간은 더 걸렸지만 (cross-platform CI 추가 등) 본질적 커플링 0 예측은 적중. 분리 후 *외부 plugin 이 `tasty-hooks` 를 직접 의존* 하는 use case 는 아직 0 (검증: `grep -r "tasty-hooks" crates/tasty-plugin-*` = 0). 이는 옛 *AI 에이전트 생태계 고유 가치* 가설이 *외부 plugin 사용* 으로는 실현 안 됐음을 의미하지만, 본 바이너리 내 응집은 그대로 유지.

---

## Phase 2: tasty-terminal 분리 — **완료**

`src/terminal.rs` (옛 1,358 LOC) → `crates/tasty-terminal/src/lib.rs` 이동 완료.

- **현재 상태**: 4,824 LOC (3.5× 성장). cross-platform pty 흡수: `termwiz` / `portable-pty` / `unicode-width` + `cfg(target_os = "macos")` libc / `cfg(windows)` `windows` crate.
- **회고**: 옛 *model.rs 와의 단방향 의존* (model → terminal) 분석 정확. PTY/VTE 엔진 응집이 분리 후에도 깨지지 않음. *다른 프로젝트 재사용* 시나리오는 아직 0 이지만 분리 자체로 본 바이너리의 `cfg(windows)` ConPTY 분기를 본 빌드와 격리한 가치 큼.

---

## Phase 3: tasty-ipc 분리 — **비권장 유지 (분리 안 됨)**

옛 분석: `tasty-ipc-protocol` (131 LOC), `tasty-ipc-server` (196 LOC) 둘 다 비권장.

- **현재 위치**: `src/app/ipc/`, `src/adapters/ipc/`, `src/ports/ipc_server.rs` 에 분산.
- **현재 LOC**: 옛 327 LOC 대비 *수천 LOC 로 성장* (handler 다수 추가) 했지만 분리 trigger 도달 안 함.
- **회고**: 옛 비권장 판정 사유 (*tasty 고유 로직 (포트 파일 경로 / handler / approval) 의 응집*, *외부 재사용 가치 미미*) 가 현재도 그대로. 오히려 plugin 시스템 추가로 `host_api::ipc::handler::plugin` 같은 *본 바이너리 전용* handler 가 더 증가 — 분리는 추가 부담 발생.
- **재검토 trigger**: 외부 도구가 *tasty IPC wire format 만* 사용해 host 와 통신할 use case 가 발생할 때. 현재까지 0.

---

## Phase 4: tasty-renderer 분리 — **장기 과제 유지 (분리 안 됨)**

옛 분석: `font.rs + renderer.rs` (옛 1,108 LOC) → 장기 과제.

- **현재 위치**: `src/gfx/renderer/` + `src/gfx/gpu/` (본 바이너리), `tasty-font` 만 별도 crate (1,186 LOC).
- **회고**: 옛 *trigger* 였던 "코드베이스 15,000줄 이상 성장 시 재검토" — 본 바이너리는 ~91k LOC 로 6× 초과했음에도 *분리 안 됨*. **이는 LOC 기반 trigger 가 무효함을 입증**.
- **현 시점 trigger 재정의**:
  - 다른 VTE 백엔드 (alacritty_terminal / 자체) 를 지원해야 할 use case 발생.
  - 외부 프로젝트가 *tasty 의 wgpu 셀 파이프라인* 만 재사용하고 싶다는 요구 발생.
  - `wgpu` 공개 API 가 1.0 stable 도달 + tasty 자체 셀 파이프라인이 안정화 (현재는 둘 다 미달).
- 위 3 조건 중 *2 개 이상* 충족 시 `TerminalSurface` trait 설계로 분리 진입.

---

## Phase 5: tasty-model 분리 — **장기 과제 유지 (디렉토리 분할만 완료)**

옛 분석은 *크레이트 분리* 와 *파일 분할 (대안)* 두 갈래로 제시. **파일 분할은 완료, crate 분리는 미완**.

- **현재 위치**: `src/model/` 디렉토리 (13 파일: `closed_item.rs`, `surface_layout.rs`, `pane_tree.rs`, `tab.rs`, `workspace.rs` 등).
- **회고**:
  - *옛 model.rs 1,775 LOC 단일 파일* → 디렉토리 분할로 모듈별 응집 확보. 옛 "크레이트 분리 없이 파일 분할" 대안 권고 정확.
  - 크레이트 분리는 미완. 옛 *제네릭 전파 8 단계* (`SurfaceNode<T>` → ... → `Workspace<T>`) 문제 그대로 — `Terminal` 타입 직접 의존이 분리 비용을 만들고 있음.
- **현 시점 trigger 재정의**:
  - *headless 모드* (Phase E.B) 가 안정화되면 `Terminal` 의존을 `TerminalBackend` trait 으로 추상화하는 비용이 *headless 비용에 흡수* 됨 → 그 시점에 분리.
  - 그 외엔 *외부 재사용 use case* 가 필요 (현재 0).

---

## Phase 6: tasty-notification 분리 — **비권장 유지, 재검토 필요**

옛 분석: 239 LOC → 비권장.

- **현재 위치**: 4 곳 분산.
  - `src/store/notification.rs` — `NotificationStore`.
  - `src/adapters/ui/notification.rs` — OS 알림 (notify-rust).
  - `src/adapters/ipc/handler/notification.rs` — IPC 핸들러.
  - `src/view/settings/ui/tabs/notifications.rs` — 설정 UI.
- **회고**: *분산만* 진행되고 *crate 분리* 는 미진행. 옛 비권장 판정의 사유 (*tasty 고유 설정/알림 구조*) 는 여전히 유효하나, plugin 시스템이 알림 IPC 를 호출하는 use case 가 발생 시 도메인 crate 화 가치가 ↑ 한다.
- **현 시점 trigger 재정의**:
  - plugin 이 *host 의 알림 도메인 타입* 을 직접 import 해야 할 use case 발생 시 → `tasty-notification` 으로 분리 후 plugin-sdk 가 재수출.
  - 현재까지 plugin 은 IPC wire format 만 사용 → 도메인 타입 노출 불필요 → 분리 trigger 미도달.

---

## 신규 crate 분리 시 참고 절차 (boilerplate)

옛 Phase 1/2 의 절차에서 추출. *신규 crate 분리* 시 출발점으로 사용 가능.

1. `mkdir -p crates/{crate-name}/src` + `Cargo.toml` 작성 (`[package] name / edition / version`, `[dependencies]` 외부만, `[lints] workspace = true`).
2. 루트 `Cargo.toml` `[dependencies]` 절에 `{crate-name} = { path = "crates/{crate-name}" }` 추가.
3. 본 바이너리 `src/{name}.rs` 또는 `src/{name}/` 디렉토리 이동 — `mv src/{path} crates/{crate-name}/src/lib.rs` 후 `pub mod` 재구성.
4. 본 바이너리에서 `use crate::{path}` → `use {crate_name}::{path}` 일괄 교체.
5. `cargo check --workspace` → 의존 누락 보완 → `cargo test --workspace`.
6. 분리 후 docs 갱신: `architecture/index.md` 워크스페이스 표 행 추가 + 본 문서 Phase 완료 표기.

---

## 인접 문서

| 문서 | 설명 |
|------|------|
| [`index.md`](index.md) | 28 crate 현황 매트릭스 + 옛 8 후보 도달 상태 |
| [`workspace-design.md`](workspace-design.md) | Cargo workspace 구조 / Cargo.toml 발췌 / 의존성 그래프 |
| [`../refactoring.md`](../refactoring.md) | 본 바이너리 내부 리팩토링 우선순위 (crate 분리 후보 절 cross-link) |
