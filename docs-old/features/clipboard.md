# 클립보드

- **Status**: Implemented

### arboard 기반 크로스 플랫폼 클립보드
- `arboard` 크레이트를 사용한 시스템 클립보드 읽기/쓰기
- 앱 시작 시 `Clipboard` 인스턴스를 생성하여 App 구조체에 보관

### 텍스트 선택 (Text Selection)
- 마우스 드래그로 터미널 텍스트 선택
- 선택 모드:
  - **Normal**: 문자 단위 드래그 선택
  - **Word**: 더블클릭으로 단어 선택
  - **Line**: 트리플클릭으로 줄 전체 선택
  - **Block**: vi 복사 모드의 `Ctrl+v` 로 진입하는 사각형 선택 (마우스에서는 사용 안 함)
- 선택 영역 시각적 하이라이트 (배경색 오버라이드, Catppuccin Surface2 기반)
- 스크롤백 영역과 화면 영역을 넘나드는 선택 지원
- 전각 문자(CJK, 한글) 2셀 너비 올바르게 처리
- 마우스 트래킹 모드(1000/1002/1003) 활성 시 Shift+드래그로 강제 선택

### vi 스타일 키보드 복사 모드
- `Ctrl+Shift+Space` (기본 키, `enter_copy_mode` 액션) 로 진입. 진입 시 PTY 입력이 차단되고 커서 위치가 1셀 하이라이트로 표시됨
- 이동 키: `h`/`j`/`k`/`l` (좌/하/상/우), `w`/`b`/`e` (단어 점프), `0`/`$` (줄 시작/끝), `gg`/`G` (스크롤백 최상단/하단 — vim 과 동일한 double-key 시퀀스, 첫 `g` 후 다른 키를 누르면 시퀀스 취소), `H`/`M`/`L` (viewport top/middle/bottom)
- cursor cell 강조: 진입 직후 및 visual selection 활성 중에도 cursor 가 위치한 셀은 `theme.vi_cursor_bg` (Catppuccin lavender) 로 selection 보다 진한 톤으로 강조되어 식별 가능
- count prefix 지원: `3w`, `5j`, `10l` 등 (6자리 cap)
- visual 선택: `v` (문자), `V` (줄 전체), `Ctrl+v` (사각형 블록)
- `y` 로 클립보드 복사 + 모드 종료, `q` 또는 `Esc` 로 종료 (visual 중이면 visual 만 해제, 한 번 더 누르면 모드 종료)
- 검색: `/` 또는 `?` 로 mini-prompt 활성 → 텍스트 입력 후 `Enter` 로 commit. `n`/`N` 으로 다음/이전 매치. 검색 결과는 기존 `SearchState` 와 동일한 하이라이트
- viewport 자동 정렬: cursor 가 화면 밖이면 스크롤이 자동으로 따라감
- 마우스 좌클릭 시 모드 자동 종료. alt-screen 앱 (vim/tmux) 실행 중이면 진입 차단 + toast 안내
- 터미널 텍스트 영역 위에서 마우스 커서가 I-beam으로 변경
- 마우스 클릭으로 커서 위치 이동 (`click_cursor` 모듈, `general.click_to_move_cursor` 설정으로 on/off):
  - `EditableRegion`: 현재 셸 입력의 편집 가능 영역을 계산 (커서 위치 + 소프트 랩 연속 행)
  - 클릭 위치를 편집 가능 영역으로 클램핑한 뒤, 화살표 키를 전송하여 셸 커서 이동
  - 전각 문자(CJK, 한글) 2셀 너비를 고려하여 정확한 화살표 횟수 계산
  - 소프트 랩(긴 명령어 줄바꿈) 시 여러 줄에 걸친 이동 지원
  - 편집 불가 영역 클릭 방지:
    - 커서 행 아래(빈 영역) 클릭 시 이동하지 않음
    - 이전 명령어 출력 행 클릭 시 이동하지 않음 (소프트 랩 연속 행만 허용)
    - 커서 행에서 커서 오른쪽(빈 공간) 클릭 시 커서 위치로 클램핑
  - 스크롤백 중, alternate screen(vim 등), 마우스 트래킹 모드에서는 비활성

### 복사 (Copy)
- `settings.keybindings.copy` 바인딩 목록 중 하나와 매칭되면 복사 (다중 바인딩 지원)
- 플랫폼별 기본값:
  - **Windows**: `ctrl+c` — 선택 있으면 복사, 없으면 SIGINT 전달
  - **Linux**: `ctrl+shift+c`
  - **macOS**: `alt+c`
- 선택 텍스트를 시스템 클립보드에 복사 후 선택 해제
- 키보드 입력 시 선택 자동 해제
- 사용자가 Keybindings → Clipboard 서브탭에서 바인딩을 추가/삭제/변경 가능
- **소프트 랩 인지 복사**: 셸이 긴 명령을 터미널 너비에 맞게 자동 줄바꿈한 라인은 복사 시 한 줄로 다시 합쳐진다. 진짜 `\n`(hard newline)은 그대로 보존. wrap 여부는 라인의 마지막 컬럼이 비공백 글자로 채워졌는지로 판정하며, 화면과 스크롤백(메모리·디스크) 모두 동일 규칙으로 동작. cursor positioning 기반 TUI(vim, htop, Claude Code 등)는 보통 마지막 컬럼을 채우지 않아 영향이 없음

### 붙여넣기 (Paste)
- `settings.keybindings.paste` 바인딩 목록 중 하나와 매칭되면 붙여넣기 (다중 바인딩 지원)
- 플랫폼별 기본값:
  - **Windows**: `ctrl+v`
  - **Linux**: `ctrl+shift+v`
  - **macOS**: `alt+v`
- 브래킷 붙여넣기 모드(DECSET 2004) 지원: 활성화 시 `\x1b[200~` ... `\x1b[201~`로 감싸서 전송
- 포커스된 터미널의 PTY에 직접 전송
- **이미지 붙여넣기**: 클립보드에 텍스트가 없고 이미지가 있는 경우, 이미지를 PNG 파일로 저장(`/tmp/tasty-clipboard/paste-{timestamp}.png`)하고 파일 경로를 터미널에 붙여넣기. AI 에이전트가 이미지를 참조할 수 있도록 지원
- **Paste 후 Ctrl+C 보호 (cooldown 500ms)**: 터미널에 paste를 보낸 직후 500ms 안에 들어온 Ctrl+C는 SIGINT 전송도, 클립보드 복사도 하지 않고 무시한다. Ctrl+V를 누르려다 옆 키 Ctrl+C를 잘못 눌러 입력을 통째로 날려버리는 사고를 막기 위함. 무시될 때 toast로 알림. 500ms 이후의 Ctrl+C는 기존 동작(선택 영역이 있으면 복사, 없으면 SIGINT)으로 정상 처리

### IME 입력 (한글/CJK)
- winit의 IME 이벤트를 통한 CJK 입력기 지원
- 조합 중인 문자를 GPU 셰이더 파이프라인(CellRenderer)으로 터미널 셀 위에 직접 렌더링 (파란색 배경). 터미널 글리프와 동일한 좌표계를 사용하여 셀 그리드와 정확히 일치
- 조합 확정(Commit) 시 PTY로 전송
- 조합 중 단축키(Ctrl/Cmd/Alt + 키)는 physical key 기반으로 올바르게 동작
- 마우스 클릭 시 조합 중인 텍스트를 먼저 커밋한 후 커서 이동
- 분할 패널에서 포커스된 서피스의 실제 위치에 preedit 오버레이 표시
- OS IME 후보창 위치를 실제 셀 좌표 기반으로 동기화 (`set_ime_cursor_area`)
- macOS: 스페이스 이중 입력 방지 및 쉼표/마침표 유실 복구 (winit quirk 대응)
- macOS: 입력소스 전환(한영 전환) 직후 첫 글자가 조합 없이 확정되는 문제 수정 (winit 포크에서 `interpretKeyEvents` 재시도로 해결)
- Linux: Ime::Commit에 트리거 키가 포함되지 않는 동작을 올바르게 처리
- **IME 시뮬레이션 API**: IPC/CLI를 통해 IME 입력을 프로그래밍 방식으로 시뮬레이션. AI 에이전트가 한글/CJK 입력 파이프라인을 직접 테스트할 수 있음
  - `surface.ime_enable` / `surface.ime_disable`: IME 활성/비활성 전환
  - `surface.ime_preedit`: 조합 중 텍스트 표시 (preedit 렌더링 테스트)
  - `surface.ime_commit`: 조합 확정 → PTY 전송
  - `surface.ime_status`: 현재 IME 상태 조회 (active, preedit_text)
  - CLI: `tasty debug ime-enable`, `tasty debug ime-disable`, `tasty debug ime-preedit "ㅎ"`, `tasty debug ime-commit "한"`, `tasty debug ime-status`

### OSC 52 클립보드 설정
- 터미널 프로그램이 OSC 52 시퀀스로 시스템 클립보드에 텍스트를 설정할 수 있음
- termwiz의 `SetSelection` 파싱을 활용하여 이벤트 발생 → main.rs에서 arboard로 클립보드에 반영

### 시스템 클립보드 히스토리
- `EngineState.clipboard_history`(메모리 전용)에 시스템 클립보드 변경 기록 (호스트가 소유)
- 별도 스레드가 `settings.clipboard.poll_interval_ms`(기본 500ms) 주기로 `AppEvent::ClipboardTick` 발송 → 메인 스레드가 arboard로 현재 값을 읽어 모든 Window의 history에 기록
- 연속 중복 자동 제거 (`last_seen` 비교), 빈 문자열 무시
- 출처 태그: `ClipboardSource::System`(외부 앱) / `Internal`(Tasty 내부 복사). 내부 복사 지점(터미널 선택 복사, OSC 52)에서는 즉시 `record_internal_copy`로 기록
- 설정: `clipboard.history_enabled`(기본 on), `history_max`(기본 100), `poll_interval_ms`(재시작 필요)
- 주의: 비밀번호 관리자 등 민감 정보도 기록된다. OS 레벨 민감 플래그를 구분할 수단이 제한적이라 1차는 필터 없음
- 재시작 시 휘발(디스크 영속화는 별도 TODO)
- 사용자 viewer 는 빌트인 `com.tasty.clipboard-history` plugin 이 popup 으로 제공 (단축키 `toggle_clipboard_viewer`, 기본 `Ctrl+Shift+H`). plugin 은 `tool.clipboard.list` 로 호스트 history 를 읽어 표시하고 다음 액션을 노출한다:
  - **항목 클릭** → `tool.clipboard.paste { index }` 호출 후 popup 자동 종료
  - **항목 옆 × 버튼** → `tool.clipboard.remove { index }` 호출 후 트리만 재렌더 (popup 유지)
  - **헤더 Clear all** → `tool.clipboard.clear {}` 호출 후 트리 재렌더
  - 이미 떠 있을 때 다시 토글하면 placeholder ("Clipboard viewer is already open") 만 표시 — outside-click / Esc 로 닫고 다시 열어야 한다
