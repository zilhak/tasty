# 사이드바 (Sidebar)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 봄)
- **ADR**: 없음
- **코드**: `src/adapters/ui/sidebar/`
- **화면**: [screens/sidebar.md](screens/sidebar.md)

## 목적

[MainView](../main-view/index.md) 좌측 패널. 워크스페이스를 전환·관리하고, 도구/플러그인/설정으로 들어가는 진입점을 모아둔 영역.

## 내부 동작

### 두 상태 — full / collapsed

접기 버튼으로 full ↔ collapsed 토글. collapsed 는 아이콘만 남긴 좁은 형태.

### 영역 구성

- **헤더**: 워드마크 `tasty.` + 수박 로고 + 접기 버튼.
- **워크스페이스 영역** (남는 높이 전부): "Workspaces" heading + 워크스페이스 카드 목록 + New workspace 버튼.
- **하단**: 도구 / 플러그인 / 설정 버튼.

### 워크스페이스 조작

- **전환**: 카드 클릭 → 해당 Workspace 활성.
- **재정렬**: 카드 drag-and-drop.
- **생성**: New workspace 버튼(좌클릭) / 우클릭 → 프리셋으로 생성.
- **attach 인디케이터**: 다른 client(원격 사용자)가 그 Workspace 를 점유 중이면 빨간 인디케이터 ([점유 모델](../../concepts/actors.md)).

## 인터페이스

- **사용자**: 클릭(전환/버튼), 드래그(재정렬), 접기 토글.
- **하단 버튼은 다른 기능으로 위임** (연결 개념 — 사이드바는 진입점만, 내용은 각 문서):
  - 도구 버튼 → `features/tools-menu/` *(재작성 예정)*
  - 플러그인 버튼 → `features/plugin-system/` *(재작성 예정)*
  - 설정 버튼 → `features/settings/` *(재작성 예정)*

## 비-목표

- 워크스페이스/Pane/Tab/Surface 자체의 동작 정의 — 사이드바는 *전환·목록* 만. 도메인은 [구조 계층](../../concepts/hierarchy.md) 및 해당 feature.
- 도구/플러그인/설정 *창의 내용* — 버튼은 진입점일 뿐, 내용은 각 feature.

## Acceptance Criteria

- [ ] 접기 버튼 클릭 시 full ↔ collapsed 가 토글된다.
- [ ] 워크스페이스 카드 클릭 시 해당 Workspace 로 전환된다.
- [ ] New workspace 버튼으로 새 Workspace 가 생성된다 (우클릭 시 프리셋).
- [ ] 다른 client 가 점유 중인 Workspace 카드에 점유 인디케이터가 표시된다.
- [ ] 도구/플러그인/설정 버튼이 각각 도구 메뉴 / 플러그인 창 / 설정 창을 연다.

> GUI 컴포넌트라 시각 검증은 스크린샷, 전환/생성 동작은 IPC/CLI(workspace 조작) + 스크린샷 병행.

## 구현

- `src/adapters/ui/sidebar/full.rs` (full), `collapsed.rs` (collapsed), `view.rs` (공통 레이아웃 — 헤더/하단/목록), `tools.rs` (도구 버튼 → 도구 메뉴).
- 헤더 로고: `assets/icons/icon_256.png`. 하단 버튼 아이콘: `icons::{TOOLS, PLUG, SETTINGS}`.

## 화면

- [screens/sidebar.md](screens/sidebar.md) — full/collapsed 레이아웃과 하단 버튼의 연결.
