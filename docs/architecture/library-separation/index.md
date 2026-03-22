# 라이브러리 분리 분석 — 다관점 종합

## 분석 배경

Tasty는 현재 17개 소스 파일, 약 8,870줄의 단일 바이너리 크레이트다. 코드베이스가 성장함에 따라 일부 모듈을 독립 라이브러리 크레이트로 분리할지 판단이 필요하다. 이 문서는 8개 분리 후보를 7개 관점에서 분석한 결과를 종합한다.

## 분리 후보 8개

| # | 후보 | 현재 파일 | 분리 크레이트명 | 줄 수 |
|---|------|-----------|----------------|-------|
| 1 | 터미널 엔진 | `terminal.rs` | `tasty-terminal` | 1,358 |
| 2 | GPU 렌더러 | `font.rs` + `renderer.rs` | `tasty-renderer` | 1,108 |
| 3 | IPC 프로토콜 | `ipc/protocol.rs` | `tasty-ipc-protocol` | 131 |
| 4 | IPC 서버 | `ipc/server.rs` | `tasty-ipc-server` | 196 |
| 5 | 알림 저장소 | `notification.rs` | `tasty-notification` | 239 |
| 6 | 이벤트 훅 | `hooks.rs` | `tasty-hooks` | 290 |
| 7 | 데이터 모델 | `model.rs` | `tasty-model` | 1,775 |
| 8 | 설정 | `settings.rs` | `tasty-settings` | 326 |

## 7개 분석 관점

| 관점 | 문서 | 핵심 질문 |
|------|------|-----------|
| 기술적 분리 가능성 | [technical-feasibility.md](technical-feasibility.md) | 물리적으로 분리할 수 있는가? |
| 생태계 가치 | [ecosystem-value.md](ecosystem-value.md) | 외부에서 재사용할 가치가 있는가? |
| 유지보수 | [maintainability.md](maintainability.md) | 분리가 유지보수를 돕는가 해치는가? |
| 성능 | [performance.md](performance.md) | 런타임/컴파일 성능에 어떤 영향인가? |
| 개발자 경험 | [developer-experience.md](developer-experience.md) | 기여자와 사용자에게 어떤 변화인가? |
| 전략 | [strategic.md](strategic.md) | 프로젝트 비전과 정합하는가? |
| 실행 계획 | [execution-plan.md](execution-plan.md) | 어떤 순서로, 어떻게 실행하는가? |

추가 설계 문서:

| 문서 | 설명 |
|------|------|
| [workspace-design.md](workspace-design.md) | Cargo workspace 구조 설계, 의존성 그래프, feature flags |

---

## 최종 판정 매트릭스

각 관점에서 분리를 **권장(O)**, **중립(△)**, **비권장(X)** 으로 판정한 결과:

| 후보 | 기술 | 생태계 | 유지보수 | 성능 | DX | 전략 | **종합** |
|------|------|--------|----------|------|-----|------|----------|
| `tasty-hooks` | O | O | O | O | O | O | **즉시 분리** |
| `tasty-terminal` | O | O | O | O | O | O | **즉시 분리** |
| `tasty-ipc-protocol` | O | △ | △ | O | △ | △ | **비권장** |
| `tasty-ipc-server` | O | △ | △ | O | △ | △ | **비권장** |
| `tasty-notification` | O | △ | △ | O | △ | △ | **비권장** |
| `tasty-settings` | △ | X | X | O | X | X | **비권장** |
| `tasty-model` | △ | X | △ | △ | △ | O | **장기 과제** |
| `tasty-renderer` | △ | O | △ | △ | △ | O | **장기 과제** |

---

## 핵심 결론

### 즉시 분리 권장 (2개)

1. **`tasty-hooks`** — `hooks.rs` (290줄). tasty 내부 타입 참조 0개. `regex`만 의존. 이미 16개 테스트 보유. 5분 내 분리 가능. AI 에이전트 생태계에 고유 가치.

2. **`tasty-terminal`** — `terminal.rs` (1,358줄). PTY + VTE 파싱 + 이벤트 발생 엔진. `model.rs`에 의존하지 않고 반대로 `model.rs`가 이것에 의존. 헤드리스 터미널, 독립 테스트, 다른 프로젝트 재사용 가능.

### 비권장 (3개)

3. **`tasty-ipc-protocol`** — 131줄짜리 파일을 별도 크레이트로 관리하는 오버헤드가 이점을 초과. 이미 `jsonrpc-core` 등 기존 크레이트가 있어 외부 가치 미미.

4. **`tasty-ipc-server`** — `directories` 크레이트의 포트 파일 경로 하드코딩 등 tasty 고유 로직이 섞여 있어 범용화 비용 대비 이점 부족.

5. **`tasty-notification`** / **`tasty-settings`** — 각각 239줄, 326줄. tasty 고유 설정/알림 구조이므로 외부 재사용 가치 없음. 분리 시 관리 부담만 증가.

### 장기 과제 (2개)

6. **`tasty-model`** — `Terminal` 타입에 직접 의존 (`model.rs:1`, `model.rs:840`). 분리하려면 `TerminalBackend` trait 추상화가 필요하고, 제네릭 파라미터가 8단계 (`SurfaceNode<T>` → `SurfaceGroupLayout<T>` → `SurfaceGroupNode<T>` → `Panel<T>` → `Tab<T>` → `Pane<T>` → `PaneNode<T>` → `Workspace<T>`) 전파됨. 현재 시점에서는 비용이 이점을 초과하나, 코드베이스 15,000줄 이상 성장 시 재검토.

7. **`tasty-renderer`** — `termwiz::surface::Surface`에 직접 의존 (`renderer.rs:3`, `renderer.rs:515`). `TerminalSurface` trait 설계가 필요하고, wgpu 공개 API 안정성 문제. 다른 VTE 백엔드 지원이 필요해지는 시점에 분리.

---

## 기존 library-separation.md와의 차이점

| 항목 | 기존 문서 | 이 문서 |
|------|-----------|---------|
| 관점 | 단일 (기술 중심) | 7개 관점 |
| 후보 수 | 7개 | 8개 (`tasty-settings` 추가) |
| 판정 | 우선순위 목록 | 관점별 매트릭스 + 종합 판정 |
| 제네릭 전파 문제 | 언급만 | 8단계 전파 경로 상세 분석 |
| 렌더러 trait 설계 | 간략 | TerminalSurface trait 복잡도 상세 분석 |
| 생태계 분석 | 없음 | 기존 크레이트 비교, 고유 가치 분석 |
| 성능 분석 | 없음 | dynamic dispatch, 증분 컴파일, binary size |
| 실행 계획 | 간략 단계 | Phase별 상세 로드맵 + 롤백 계획 |
| workspace 설계 | 없음 | Cargo.toml 전문, 의존성 그래프, feature flags |
| model.rs 분할 대안 | 없음 | 크레이트 분리 없이 파일 분할하는 실용적 대안 |
