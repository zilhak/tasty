# Surface 타입 전환 (Convert Surface)

- **Status**: Implemented
- **Surface**: 사용자 (Alt+' 팝업) + 에이전트 (kind 별 IPC 경유 — 범용 convert 메서드는 없음)
- **Related ADR**: ADR 후보 (adr-candidates.md #0013 surface-cwd-invariant)
- **Related design**: [`../architecture/invariants/surface-cwd.md`](../architecture/invariants/surface-cwd.md) (전환 시 cwd carry), [`../dev-guide/popup-implementation.md`](../dev-guide/popup-implementation.md)

## 목적

이미 열려 있는 surface 의 *종류* (Terminal / Markdown / Explorer / Image / …) 를 그 자리에서 교체한다. 탭을 새로 만들지 않고 현재 위치·레이아웃을 유지한 채 콘텐츠 유형만 바꾼다.

## 사용자 행동 (UX)

- 트리거:
  - `convert_surface` 단축키 (기본 `Alt+'`, 4 개 프리셋 모두 동일) → Surface 스코프 팝업.
  - Empty surface 중앙의 타입 전환 버튼 → 동일 팝업.
  - `convert_to_markdown` 직접 전환 단축키 (기본값 없음, 설정에서 할당).
- 결과:
  - 팝업 항목은 `SurfaceKindRegistry` 에 등록된 kind 에서 **동적으로 enumerate** 된다 — 빌트인 + plugin 제공 kind. `empty` 같은 시스템 kind 는 제외 (`src/adapters/ui/popup/convert.rs` `HIDDEN_KINDS`).
  - 현재 타입과 동일한 항목은 체크 표시 + 비활성.
  - 키보드 탐색 (Up/Down + Enter), kind 첫 글자 즉시 선택 (중복 시 뒷 항목 무시).
  - Markdown 전환 시 파일 경로 입력 다이얼로그, Terminal 전환 시 새 PTY 생성 (탭 이름 CWD 기반 자동 복원).
  - **개별 surface 교체 원칙**: 대상 surface 의 구현체만 교체. 탭 레이아웃·다른 surface 등 주변 구조 무영향.
- 예외: 팝업이 열려 있는 동안 키보드 입력은 터미널로 전달되지 않는다 (PopupManager 포커스 관리). Esc / 외부 클릭 / X 로 닫기.

## 에이전트 행동 (CLI / IPC)

- **범용 `surface.convert` IPC/CLI 는 없다.** 전환 팝업은 사용자 단축키 전용 UI 다 — release 는 물론 debug IPC 로도 전환 팝업을 여는 경로가 없다 (`debug.popup.open` 은 plugin contribute popup 전용).
- kind 별 IPC 가 내부적으로 동일한 전환 메커니즘 (`DomainIntent::ConvertSurface`) 을 사용한다 — 예: `image.open` 은 `surface_id` (필수) + `path` (필수) 를 받아 대상 surface 를 image kind 로 전환한다 (`src/adapters/ipc/handler/image.rs`).
- 새 타입 surface 가 필요하면 `tab.create` 의 `type` 파라미터 (terminal / markdown / explorer + plugin contribute) 로 **새로 생성** 하는 것이 에이전트의 기본 경로다.
- 비-목표: 전환 시 포커스 이동 없음 (포커스 독립 원칙).

## 비-목표 (Out of Scope)

- 전환 전 surface 상태의 보존/병합 — 기존 구현체는 메모리에서 해제된다 (터미널 → 다른 kind → 터미널로 되돌려도 이전 셸 세션은 돌아오지 않음).
- 팝업의 에이전트 트리거 (release) — 사용자 입력 재현 금지 정책.

## Acceptance Criteria

- [ ] Given `SurfaceKindRegistry` 에 N 개 kind 등록 When 전환 팝업 호출 Then 팝업에 N 개 항목 표시 (`empty` 등 `HIDDEN_KINDS` 제외).
- [ ] Given Image plugin uninstall When 다시 팝업 호출 Then Image 항목이 사라진다 (동적 enumerate — 하드코딩 목록 없음).
- [ ] Given Markdown 으로 전환 선택 When 파일 경로 미입력 상태 Then 경로 입력 다이얼로그가 표시된다.
- [ ] Given 탭 내부 분할의 한 surface 를 전환 When 전환 완료 Then 다른 surface 는 영향 없다 (개별 surface 교체 원칙).
- [ ] Given 현재 kind 와 같은 항목 When 팝업 표시 Then 체크 표시 + 선택 비활성.
- [ ] Given 터미널에서 `cd /foo/bar` 후 Explorer 로 전환 When 전환 완료 Then Explorer 루트는 `/foo/bar` (cwd carry invariant — 호스트 시작 cwd 로 fallback 하지 않음).

## 관련 문서

- [`../features.md`](../features.md) "워크스페이스 & 탭 > Surface 타입 전환" 섹션
- [`../architecture/invariants/surface-cwd.md`](../architecture/invariants/surface-cwd.md)
- `.claude-workspace/todo/adr-candidates.md` #0013
