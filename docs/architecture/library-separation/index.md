# 라이브러리 분리 — 현황 + 옛 분석 회고

## 본 디렉토리 구성

| 시제 | 문서 | 성격 |
|------|------|------|
| **현재** | [`index.md`](index.md) (본 문서) | 28 crate 현황 매트릭스 + 옛 8 후보 도달 상태 |
| **현재** | [`execution-plan.md`](execution-plan.md) | 완료 회고 + 미분리 항목 (model / renderer / notification) 권고 |
| **현재** | [`workspace-design.md`](workspace-design.md) | 현 워크스페이스 구조 / 의존 그래프 / Cargo.toml 발췌 |
| **역사** | `technical-feasibility.md` / `ecosystem-value.md` / `maintainability.md` / `performance.md` / `developer-experience.md` / `strategic.md` | 옛 분리 계획 시점 (2025) 의 7 관점 분석 보존본. 신규 crate 추가/제거 판단 framework 로 재사용 가능 |

신규 독자는 현황부터 (본 문서 → execution-plan.md → workspace-design.md), 옛 의사결정 맥락 검토 시 6 분석 문서 참조.

---

## 분석 배경 (현 시점, 2026-06)

Tasty 워크스페이스는 본 바이너리 (`src/`, 479 `.rs` / ~91k LOC) + **28 개의 라이브러리 크레이트** (`crates/*`) 로 구성된다.

본 문서는 다음을 종합한다:
1. *현재 28 crate 의 layering 매트릭스* (4 계층 + 테스트/dev 도구).
2. *2025 년 분리 계획 8 후보의 현재 도달 상태* (4 완료 / 2 결정 반전 / 2 장기 과제 유지).
3. *남은 미분리 영역* (model / renderer / notification) 에 대한 현 시점 권고.

옛 7 관점 framework (technical-feasibility / ecosystem-value / maintainability / performance / developer-experience / strategic / execution-plan) 는 보존본에 그대로 남아 있으며, 신규 crate 추가/제거 판단 시 재사용 가능.

---

## 옛 8 후보 도달 상태

| 후보 | 2025 판정 | 현재 | 현 위치 / LOC | 비고 |
|------|-----------|------|----------------|------|
| `tasty-hooks` | 즉시 분리 | ✅ 분리 | `crates/tasty-hooks/` (344) | 예측 적중 |
| `tasty-terminal` | 즉시 분리 | ✅ 분리 | `crates/tasty-terminal/` (4,824) | cross-platform pty 흡수 후 3.5× 성장 |
| `tasty-ipc-protocol` | 비권장 | ❌ 본 바이너리 잔존 | `src/app/ipc/` + `src/adapters/ipc/` + `src/ports/ipc_server.rs` (분산) | 권고 유지 |
| `tasty-ipc-server` | 비권장 | ❌ 본 바이너리 잔존 | `src/adapters/ipc/server.rs` + `session*.rs` + `handler/` (분산) | 권고 유지 |
| `tasty-notification` | 비권장 | ❌ 본 바이너리 잔존 | `src/store/notification.rs` + `src/adapters/{ui,ipc/handler}/notification.rs` + `src/view/settings/ui/tabs/notifications.rs` (분산) | 권고 유지 (재검토 필요) |
| `tasty-settings` | 비권장 | ✅ **분리 (반전)** | `crates/tasty-settings/` (2,244) | type-\* layer + themes 공통 의존으로 plugin/sdk 외부 노출 필요 |
| `tasty-model` | 장기 과제 | ❌ 본 바이너리 잔존 | `src/model/` (디렉토리 분할 완료) | 파일 분할은 완료, crate 분리는 미완 |
| `tasty-renderer` | 장기 과제 | ❌ 본 바이너리 잔존 | `src/gfx/renderer/` + `src/gfx/gpu/` | 예측 적중 (분리 trigger 미도달) |

**판정 반전 1 건**: `tasty-settings` — `type-*` schema layer 가 plugin SDK 와 themes 양쪽에서 공통 참조되며 본 바이너리 의존을 끊기 위해 분리.

**예측 적중**: `tasty-renderer`/`tasty-model` 둘 다 *장기 과제* 라 했고 지금도 분리 안 됨. *비권장* 이었던 ipc 2 건 + notification 도 분리 안 됨.

옛 비권장이 분리된 사례 (`tasty-settings`) 는 *옛 분석의 오류* 가 아니라 *분기점 추가* — 외부 plugin SDK 요구가 *plugin-protocol 의 분리 외부 가치* 를 만들어 비권장 판정을 뒤집은 것. 옛 분석 시점에 plugin 시스템 자체가 미존재.

---

## 현재 28 crate 4 계층 매트릭스

권위 본문은 [`../index.md`](../index.md) 의 "워크스페이스 크레이트" 절. 본 표는 *분리 의사결정 분류* 시점.

| 계층 | 크레이트 | 비고 |
|------|----------|------|
| **type-\*** (leaf) | `tasty-type-geometry` (334), `tasty-type-appearance` (1,561), `tasty-utils` (52) | 의존 0 또는 type-\* 끼리만 |
| **도메인-IO** | `tasty-themes` (1,096), `tasty-settings` (2,244), `tasty-font` (1,186), `tasty-terminal` (4,824), `tasty-hooks` (344), `tasty-memory` (5,125), `tasty-telemetry` (1,152), `tasty-output` (1,425), `tasty-approval` (815), `tasty-agent` (3,086), `tasty-presets` (1,069), `tasty-shm` (1,075), `tasty-portscan` (806), `tasty-update` (165), `tasty-lua` (541) | type-\* + 다른 도메인-IO 만 의존 가능 |
| **Plugin** | `tasty-plugin-protocol` (2,026), `tasty-plugin-sdk` (3,563) | 도메인-IO 직접 의존 금지 (sandbox 경계) |
| **번들 Plugin** | `tasty-plugin-claude` (3,035), `tasty-plugin-codex` (901), `tasty-plugin-explorer` (529), `tasty-plugin-git-viewer` (730), `tasty-plugin-clipboard-history` (289), `tasty-plugin-image` (67), `tasty-plugin-html` (94) | 모두 `tasty-plugin-sdk` 만 의존 |
| **테스트/dev 도구** | `tasty-tui-simulator` (577) | E2E TUI 시뮬레이터, crossterm + clap 의존, binary 산출 |
| **본 바이너리** | `tasty` (`src/`, 479 `.rs` / ~91k LOC) | 위 28 crate 직접 의존 |

총 28 = 옛 권장 2 (terminal, hooks) + 옛 비권장 반전 2 (settings, plugin-protocol) + 신규 24.

LOC 합계 (workspace, 실측 2026-06-02): 38,711.

---

## 옛 분석 외 영역 (신규 추가)

- **type-\* layer** (geometry / appearance) — `LogicalPx`/`PhysicalPx` typed-length 시스템 ([`../../design/typed-length.md`](../../design/typed-length.md)). 옛 분석 당시 미존재.
- **Plugin 생태계** — protocol/sdk + 7 개 번들 plugin. 옛 분석 시 plugin 자체 미존재.
- **Agent / Memory / Approval / Presets / Telemetry / Output** — Phase 6.x 부터 추가된 에이전트 도메인.
- **type-\* 계층 규칙** — "type-\* 끼리만 의존 가능. 도메인/IO crate 의존 금지. 그룹 내 순환 금지." 옛 분석에 없던 새 *layering invariant*.

---

## 미분리 항목 현 시점 권고

상세는 [`execution-plan.md`](execution-plan.md) 의 각 Phase 회고 참조.

| 항목 | 현 위치 | 권고 |
|------|----------|------|
| `tasty-model` | `src/model/` (디렉토리 분할 완료) | **유지**. 옛 *제네릭 전파 8 단계* 문제는 그대로. 분리 trigger 미도달 (외부 재사용 use case 0) |
| `tasty-renderer` | `src/gfx/renderer/` + `src/gfx/gpu/` | **유지**. `TerminalSurface` trait 설계 부담 그대로. 본 바이너리 91k LOC 임에도 trigger 미도달 — 옛 *15k LOC* 기준이 무관함 입증 → 다른 trigger (다중 VTE 백엔드 / 외부 wgpu 사용자) 가 진짜 결정 요인 |
| `tasty-notification` | 4 곳 분산 | **재검토 필요**. *분리 가치* 가 옛 판정과 달라졌을 가능성 (plugin 이 알림 IPC 를 호출하는 use case 발생 시 도메인 crate 화 가치 ↑) |

---

## 인접 문서

| 문서 | 설명 |
|------|------|
| [`../index.md`](../index.md) | 워크스페이스 / 모듈 / 의존성 DAG 권위본 |
| [`../refactoring.md`](../refactoring.md) | 본 바이너리 내부 리팩토링 우선순위 (crate 분리 후보 절 포함) |
| [`execution-plan.md`](execution-plan.md) | Phase 별 완료 회고 + 미완 Phase 의 trigger 재정의 |
| [`workspace-design.md`](workspace-design.md) | Cargo workspace 구조 / Cargo.toml 발췌 / 의존성 그래프 |
