# 터미널 검색

- **Status**: Implemented

### 개요
터미널의 스크롤백 + 화면 전체를 대상으로 텍스트 검색. GPU 렌더러에서 매치를 하이라이트하며, 현재 매치(active)와 나머지 매치(inactive)를 다른 색으로 구분한다.

### 단축키
- find 단축키 (Tasty 프리셋: `Ctrl+F` / `Alt+F`, Mac: `Cmd+F`(`alt+f`)·`Ctrl+F`, Windows/Linux: `Ctrl+F`) — **포커스 토글**:
  - 터미널 포커스 + find → 검색 바 열림 + 검색 바로 포커스 이동
  - 검색 바가 이미 떠 있고 터미널 포커스 상태에서 find → 검색 바는 그대로, 포커스만 검색 바로 이동
  - 검색 바 포커스 상태에서 find → 검색 바는 떠 있는 채 포커스만 터미널로 복귀
- Escape: **검색 바가 포커스된 상태일 때만** 검색 바 닫기 + 검색 상태/하이라이트 clear + 터미널 포커스 복귀. 터미널 포커스 상태의 Escape 는 그대로 PTY 로 전달된다 (검색 바를 닫지 않는다).
- Enter: 다음 매치 (검색 바 포커스 시)
- Shift+Enter: 이전 매치 (검색 바 포커스 시)
- 화살표 ↑/↓: 매치 탐색 (검색 바 포커스 시)

### 기능
- 검색 옵션 토글 3종 (검색 바 우측에 위치):
  - `Aa` 대소문자 구분 (기본 off)
  - `.*` 정규식 (기본 off, Rust `regex` 문법)
  - `ab` 단어 단위 일치 (기본 off, `\b` 경계. literal/regex 모두 적용)
- 정규식 컴파일 실패 시 상태 영역에 "Invalid regex" 빨간 메시지 표시
- 매치 카운터 표시 (예: 3/42)
- 매치 선택 시 해당 위치로 자동 스크롤
- 검색 바는 sticky_focus 가 아닌 일반 headless PopupDef: 포커스가 터미널이면 검색 바가 떠 있어도 키보드를 가로채지 않고 모든 키(Escape 포함)를 PTY 로 흘려보낸다. 다른 surface 의 입력에도 간섭하지 않는다.
  - 키보드 라우팅 결정자는 `PopupState.focused` 플래그다. find 단축키가 이 플래그(+ egui 텍스트필드 포커스)를 터미널↔검색 바로 토글한다. 검색 바로 갈 때는 egui `request_focus` 1회, 터미널로 갈 때는 `surrender_focus` + `focused = false` 를 함께 처리해 egui 가 키를 계속 소비하지 않도록 한다.
  - 검색 바 포커스 상태(overlay)의 find 단축키는 winit 단축키 경로(overlay 게이트)에 막히므로, 검색 바 draw fn 안에서 `KeybindingSettings.find` 바인딩을 egui 입력에 직접 매칭해 감지한다 (하드코딩 없음).
- 검색 바는 `PopupScope::Surface(focused_surface_id)`로 열려 포커스된 surface 영역 상단(가로 중앙)에 anchor된다. 사이드바·탭 바 위에 떠 있지 않는다.

### 구현
- 검색 엔진: `tasty-terminal/src/search.rs` (Terminal::search)
- UI 상태: `src/search_state.rs` (SearchState)
- 검색 바: `src/ui/search_bar.rs` (PopupDef, headless, 포커스 토글)
- 하이라이트: `src/renderer/mod.rs` (SearchHighlights → 셀별 bg 오버라이드)
