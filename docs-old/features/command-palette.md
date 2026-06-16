# 명령 팔레트

- **Status**: Implemented

### 개요
VS Code 스타일의 모든 단축키 명령을 쿼리로 검색하여 실행할 수 있는 popup. 키보드만으로 단축키를 외우지 않아도 모든 기능에 접근 가능.

### 트리거
- 기본 단축키: `Ctrl+Shift+P` (macOS는 `Alt+Shift+P` 추가)
- Tools 메뉴 `Command palette…` 항목

### 동작
- 텍스트 입력으로 `KeybindingSettings::GENERAL_BINDING_FIELDS` 의 i18n 라벨에 대해 case-insensitive 매칭
- 매칭 알고리즘: 정확 substring (단어 시작 보너스) → 부분 시퀀스 (gap 페널티) 순으로 점수화
- `↑/↓` 이동, `Enter` 실행, `Esc` 닫기, 클릭으로도 실행
- 우측에 첫 번째 바인딩(예: `ctrl+w`)을 회색으로 표시
- 행 좌측 leading 아이콘: 디자인 명시 6개 명령(new_workspace / new_tab / open_markdown / toggle_settings / split_pane_vertical / toggle_clipboard_viewer)은 전용 아이콘, 나머지 동적 명령은 `COMMAND` fallback 글리프
- Enter 시 `state.command_palette.pending_run` 에 `field_id` 를 적재 → MainView가 다음 프레임 render 직후 drain하여 `dispatch_action_by_id` 호출
- dispatch는 동일한 action body를 사용하므로 단축키와 정확히 같은 효과

### 지원 명령
모든 `GENERAL_BINDING_FIELDS` 항목 (단축키 설정 탭에 나타나는 모든 동작). `toggle_command_palette` 자신만 제외.

### 사용자 vs 에이전트 행동
명령 팔레트 자체는 **사용자 행동**이다 (사용자가 키보드로 명령을 선택). 활성 surface/pane 등 포커스 상태를 사용하는 동작도 허용됨. CLI/IPC로 노출하지 않는다.

### 구현
- 상태: `src/command_palette.rs` — `CommandPaletteState { query, selected, pending_run }`, `search()`, `match_score()`
- popup: `src/ui/command_palette_popup.rs` (`command_palette` ID, 540x360 / list max_height 320 디자인 canonical, sticky_focus, close_on_outside_click)
- dispatch: `src/shortcuts.rs::MainView::dispatch_action_by_id(action_id: &str) -> bool`
- 단축키: `KeybindingSettings::toggle_command_palette` (`ctrl+shift+p`)
- drain: `src/view/main/redraw.rs` 의 render 직후
- i18n: `command_palette.*` (en/ko/ja)
