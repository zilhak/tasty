# 라이브러리 분리 — 현황 + 옛 분석 회고

## 본 디렉토리 구성

| 시제 | 문서 | 성격 |
|------|------|------|
| **현재** | [`index.md`](index.md) (본 문서) | 33 crate 현황 매트릭스 + 옛 8 후보 도달 상태 |
| **현재** | [`execution-plan.md`](execution-plan.md) | 완료 회고 + 미분리 항목 (model / renderer / notification) 권고 |
| **현재** | [`workspace-design.md`](workspace-design.md) | 현 워크스페이스 구조 / 의존 그래프 / Cargo.toml 발췌 |
| **역사** | [`../../evaluations/library-separation/`](../../evaluations/library-separation/) 하위 `technical-feasibility.md` / `ecosystem-value.md` / `maintainability.md` / `performance.md` / `developer-experience.md` / `strategic.md` | 옛 분리 계획 시점 (2025) 의 6 관점 분석 보존본. 신규 crate 추가/제거 판단 framework 로 재사용 가능 |

옛 6 관점 분석 문서는 `docs/evaluations/library-separation/` 로 이전됐다. 본 디렉토리에는 *현재 구조* 만 남는다.

신규 독자는 현황부터 (본 문서 → execution-plan.md → workspace-design.md), 옛 의사결정 맥락 검토 시 6 분석 문서 참조.

---

## 분석 배경 (현 시점, 2026-06)

Tasty 워크스페이스는 본 바이너리 (`src/`, 394 `.rs` / ~69k LOC) + **33 개의 라이브러리 크레이트** (`crates/*`) 로 구성된다.

본 문서는 다음을 종합한다:
1. *현재 33 crate 의 layering 매트릭스* (4 계층 + 테스트/dev 도구).
2. *2025 년 분리 계획 8 후보의 현재 도달 상태* (4 완료 / 2 결정 반전 / 2 장기 과제 유지).
3. *남은 미분리 영역* (model / renderer / notification) 에 대한 현 시점 권고.

옛 6 관점 framework (technical-feasibility / ecosystem-value / maintainability / performance / developer-experience / strategic) 는 `docs/evaluations/library-separation/` 에 보존되어 있으며, 신규 crate 추가/제거 판단 시 재사용 가능.

---

## 옛 8 후보 도달 상태

| 후보 | 2025 판정 | 현재 | 현 위치 / LOC | 비고 |
|------|-----------|------|----------------|------|
| `tasty-hooks` | 즉시 분리 | ✅ 분리 | `crates/tasty-hooks/` (344) | 예측 적중 |
| `tasty-terminal` | 즉시 분리 | ✅ 분리 | `crates/tasty-terminal/` (4,824) | cross-platform pty 흡수 후 3.5× 성장 |
| `tasty-ipc-protocol` | 비권장 | ✅ **분리 (반전, F.B)** | `crates/tasty-ipc/` (통합) | Phase F.B 통합 분리. `IpcHostFacade` trait 으로 호스트 의존 격리 |
| `tasty-ipc-server` | 비권장 | ✅ **분리 (반전, F.B)** | `crates/tasty-ipc/` (통합) | 옛 분리안의 protocol / server 2 분할 대신 *통합 1 crate* 로 분리 |
| `tasty-notification` | 비권장 | ❌ 본 바이너리 잔존 | `src/store/notification.rs` + `src/adapters/{ui,ipc/handler}/notification.rs` + `src/view/settings/ui/tabs/notifications.rs` (분산, F.E NotificationSoundPlayer port 추가) | 권고 유지 (G.E 재검토 시 plugin importer 0 — trigger 미도달) |
| `tasty-settings` | 비권장 | ✅ **분리 (반전)** | `crates/tasty-settings/` (2,244) | type-\* layer + themes 공통 의존으로 plugin/sdk 외부 노출 필요 |
| `tasty-model` | 장기 과제 | ✅ **분리 (G.E, 2026-06-03)** | `crates/tasty-model/` (16 파일 / 3,719 LOC) | F.A headless 도입이 trigger. 본 바이너리 `src/model.rs` 는 `pub use tasty_model::*;` shim |
| `tasty-renderer` | 장기 과제 | ❌ 본 바이너리 잔존 | `src/gfx/renderer/` + `src/gfx/gpu/` (16 파일 / 3,633 LOC) | G.E 재검토 시 본 바이너리 내부 의존 13 모듈 + wgpu 24 미안정 — trigger 미도달 |

**판정 반전 누계 3 건**:

1. `tasty-settings` — `type-*` schema layer 가 plugin SDK 와 themes 양쪽에서 공통 참조되며 본 바이너리 의존을 끊기 위해 분리.
2. `tasty-ipc-protocol` + `tasty-ipc-server` → 통합 `tasty-ipc` (Phase F.B) — 옛 *외부 재사용 가치 미미* 판정이 *호스트 측 facade 격리* 라는 새 분기점에 뒤집힘.
3. `tasty-model` — F.A (headless 도입) 가 옛 *장기 과제* trigger 를 충족해 G.E 에서 분리.

**예측 적중**: `tasty-renderer` 는 *장기 과제* 라 했고 지금도 분리 안 됨 (G.E 재평가에서도 미도달). `tasty-notification` 은 *비권장* 이었고 지금도 분리 안 됨.

옛 비권장이 분리된 사례 (`tasty-settings`, `tasty-ipc-*`) 는 *옛 분석의 오류* 가 아니라 *분기점 추가* — 외부 plugin SDK / 호스트 격리 요구가 *분리 외부 가치* 를 만들어 비권장 판정을 뒤집은 것. 옛 분석 시점에 plugin 시스템 자체가 미존재.

---

## 현재 33 crate 4 계층 매트릭스

권위 본문은 [`../index.md`](../index.md) 의 "워크스페이스 크레이트" 절. 본 표는 *분리 의사결정 분류* 시점.

| 계층 | 크레이트 | 비고 |
|------|----------|------|
| **type-\*** (leaf) | `tasty-type-geometry` (334), `tasty-type-appearance` (1,561), `tasty-utils` (52) | 의존 0 또는 type-\* 끼리만 |
| **도메인-IO** | `tasty-themes` (1,096), `tasty-settings` (2,244), `tasty-font` (1,186), `tasty-terminal` (4,824), `tasty-hooks` (344), `tasty-memory` (5,125), `tasty-telemetry` (1,152), `tasty-output` (1,425), `tasty-approval` (815), `tasty-agent` (3,086), `tasty-presets` (1,069), `tasty-shm` (1,075), `tasty-portscan` (806), `tasty-update` (165), `tasty-lua` (541), `tasty-model` (3,719, G.E 신규), `tasty-ipc` / `tasty-cli` / `tasty-host-plugin` / `tasty-plugin-manifest` (F.B 신규) | type-\* + 다른 도메인-IO 만 의존 가능. **주의**: `tasty-presets` 내부의 `crate::model::` 모듈은 *자기 내부 model* 이지 `tasty-model` crate 가 아님 — `tasty-presets ↛ tasty-model` (cross-crate 의존 없음) |
| **Plugin** | `tasty-plugin-protocol` (2,026), `tasty-plugin-sdk` (3,563) | 도메인-IO 직접 의존 금지 (sandbox 경계) |
| **번들 Plugin** | `tasty-plugin-claude` (3,035), `tasty-plugin-codex` (901), `tasty-plugin-explorer` (529), `tasty-plugin-git-viewer` (730), `tasty-plugin-clipboard-history` (289), `tasty-plugin-image` (67), `tasty-plugin-html` (94) | 모두 `tasty-plugin-sdk` 만 의존 |
| **테스트/dev 도구** | `tasty-tui-simulator` (577) | E2E TUI 시뮬레이터, crossterm + clap 의존, binary 산출 |
| **본 바이너리** | `tasty` (`src/`, 394 `.rs` / ~69k LOC) | 위 33 crate 직접 의존 |

총 33 = 옛 권장 2 (terminal, hooks) + 옛 비권장 반전 3 (settings, ipc 통합, plugin-protocol) + 옛 장기 과제 반전 1 (model, G.E) + 신규 27 (F.B 4 종 포함).

LOC 합계 (workspace 크레이트 270 `.rs`, 실측 2026-06-03): ~63k. F.B (cli/ipc/manifest/host-plugin) + G.E (model) 이전 ~38k 대비 워크스페이스로 약 25k 이동.

---

## 옛 분석 외 영역 (신규 추가)

- **type-\* layer** (geometry / appearance) — `LogicalPx`/`PhysicalPx` typed-length 시스템 ([`../../concepts/typed-length.md`](../../concepts/typed-length.md)). 옛 분석 당시 미존재.
- **Plugin 생태계** — protocol/sdk + 7 개 번들 plugin. 옛 분석 시 plugin 자체 미존재.
- **Agent / Memory / Approval / Presets / Telemetry / Output** — Phase 6.x 부터 추가된 에이전트 도메인.
- **type-\* 계층 규칙** — "type-\* 끼리만 의존 가능. 도메인/IO crate 의존 금지. 그룹 내 순환 금지." 옛 분석에 없던 새 *layering invariant*.

---

## 미분리 항목 현 시점 권고

상세는 [`execution-plan.md`](execution-plan.md) 의 각 Phase 회고 참조.

| 항목 | 현 위치 | 권고 |
|------|----------|------|
| `tasty-renderer` | `src/gfx/renderer/` + `src/gfx/gpu/` (16 파일 / 3,633 LOC) | **G.E (2026-06-03) 시점 trigger 미도달 — 보류**. 본 바이너리 내부 의존 13 unique 모듈 (state/settings/plugin/AppEvent/i18n 등). wgpu 24 미안정 + 외부 사용자 0. 다중 VTE 백엔드 도입 또는 wgpu 1.0 도달 시점 재평가. |
| `tasty-notification` | 4+ 곳 분산 (F.E NotificationSoundPlayer port 도입 후에도 plugin importer = 0) | **G.E (2026-06-03) 시점 trigger 미도달 — 보류**. `grep crates/` plugin importer 0. 본 바이너리 내부 importer 7곳뿐. plugin 이 NotificationSoundPlayer trait 직접 의존 시점 재평가. |

**G.E (2026-06-03) 완료**: `tasty-model` 은 trigger 도달로 분리 — `crates/tasty-model/` (16 파일 / 3,719 LOC). 본 매트릭스의 `tasty-model` 행은 § "옛 8 후보 도달 상태" 와 § "현재 33 crate 4 계층 매트릭스" 에서 *완료* 로 표기됨.

---

## 인접 문서

| 문서 | 설명 |
|------|------|
| [`../index.md`](../index.md) | 워크스페이스 / 모듈 / 의존성 DAG 권위본 |
| [`../refactoring.md`](../refactoring.md) | 본 바이너리 내부 리팩토링 우선순위 (crate 분리 후보 절 포함) |
| [`execution-plan.md`](execution-plan.md) | Phase 별 완료 회고 + 미완 Phase 의 trigger 재정의 |
| [`workspace-design.md`](workspace-design.md) | Cargo workspace 구조 / Cargo.toml 발췌 / 의존성 그래프 |
