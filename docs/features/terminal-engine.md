# 터미널 엔진

- **Status**: Implemented

### PTY 기반 셸 실행
- ConPTY(Windows) / Unix PTY를 통한 네이티브 셸 실행
- `TERM=xterm-256color` 환경 설정
- PTY 리사이즈 전파: 윈도우 크기 변경 시 자식 프로세스에 새 크기 통보. rows 축소 시 커서 아래 빈 행을 먼저 제거하고 부족하면 위쪽 행을 scrollback으로 캡처하여 커서-콘텐츠 관계를 보존. rows 확대 시 scrollback에서 복원. 모든 워크스페이스/탭의 터미널에 리사이즈 전파
- 자식 프로세스 핸들 관리: 생존 여부 확인 가능
- 입력 스레드 분리 파싱: 터미널마다 **파서 스레드**가 PTY raw 바이트를 읽는 즉시 그 스레드에서 VTE 파싱·그리드 갱신(`ingest`)을 수행한다. winit(메인) 이벤트 루프는 더 이상 파싱하지 않으므로, 백그라운드 터미널 다수가 출력해도 포그라운드 키 입력/IPC가 파싱 백로그에 막히지 않는다. 그리드 상태(`TerminalState`)는 `Arc<Mutex<_>>`로 공유하며 파서 스레드는 8KB 청크마다 락을 잡고 즉시 해제해, 메인 스레드의 렌더/IPC/이벤트 수집은 최대 1 청크 파싱 시간만 대기한다. 백프레셔는 파서 스레드의 blocking read 가 담당하고, `Terminal` 핸들이 drop 되면(surface 닫힘) 파서 스레드는 `Weak` 업그레이드 실패로 즉시 종료한다. 설계 근거·대안: [ADR-0002](../adr/0002-vte-parsing-off-input-thread.md).
- 작업 디렉토리 상속: 새 surface/pane/workspace 생성 시 소스 surface의 현재 작업 디렉토리를 자동 상속. CWD 결정은 플랫폼별로 다르다 — **macOS/Linux**는 셸 PID로 OS를 직접 조회(`get_cwd_of_pid`: macOS `proc_pidinfo`, Linux `/proc/<pid>/cwd`)하므로 셸 설정과 무관하게 동작하며, OSC 7 캐시가 있으면 그 값을 우선 사용한다. **Windows**는 타 프로세스 cwd 조회 API가 없어 셸이 내보내는 OSC 7 시퀀스 캐시에만 의존한다 — tasty 가 bash 를 `--rcfile <합성 rc>` 로 띄워 **셸 모드(default/tasty)와 무관하게 OSC 7 emit·UTF-8·MSYS PATH 빌트인을 강제 주입**한다. VTE 파서가 `/C:/path` URI 형식을 Windows 경로로 정규화. 설정에서 on/off 가능 (`general.inherit_cwd`, 기본 on). CLI/IPC에서는 `--cwd` 옵션으로 명시적 경로 지정도 가능

### VTE 파싱 및 터미널 에뮬레이션
- termwiz `Parser`를 통한 VT 이스케이프 시퀀스 파싱
- termwiz `Surface`를 통한 셀 그리드 상태 관리
- 지원하는 시퀀스:
  - **텍스트 출력**: Print, PrintString
  - **제어 코드**: LF, CR, BS, HT, Bell
  - **SGR (텍스트 속성)**: Reset, Intensity(Bold/Dim), Underline, Italic, Blink, Inverse, Invisible, StrikeThrough, Foreground/Background 색상
  - **커서 이동**: Up/Down/Left/Right, Position(CUP), CharacterAbsolute(CHA), LinePositionAbsolute(VPA), NextLine(CNL), PrecedingLine(CPL), Save/Restore
  - **화면 편집**: EraseInDisplay(ED 0/2/3), EraseInLine(EL 0/1/2), ScrollUp(SU), ScrollDown(SD), ClearScreen, ClearToEndOfLine, ClearToEndOfScreen, EraseToStartOfDisplay, EraseToStartOfLine, DeleteCharacter(DCH), InsertCharacter(ICH), DeleteLine(DL), InsertLine(IL), EraseCharacter(ECH). DCH/ICH는 전각 문자(CJK)의 2셀 너비를 올바르게 처리
  - **ESC 시퀀스**: DECSC/DECRC(커서 저장/복원), IND(인덱스, ESC D), RI(역방향 인덱스, ESC M), RIS(전체 리셋). IND/RI는 스크롤 리전 경계에서 ScrollRegionUp/Down을 수행
  - **DECSET/DECRST (CSI ? Pm h/l)**: 터미널 모드 전환
    - DECCKM (모드 1): 애플리케이션 커서 키 — 방향키가 `\x1bO{A..D}` 시퀀스를 전송
    - DECTCEM (모드 25): 커서 가시성 제어
    - 대체 화면 버퍼 (모드 47/1047/1049): vim, htop, less, nano 등 TUI 앱 지원. 모드 1049는 커서 저장/복원 및 화면 클리어 포함
    - 마우스 트래킹 (모드 1000/1002/1003): 클릭/셀 모션/전체 모션 추적
    - SGR 마우스 (모드 1006): 확장 마우스 좌표 인코딩
    - 포커스 트래킹 (모드 1004): FocusIn/FocusOut 이벤트
    - 브래킷 붙여넣기 (모드 2004): 붙여넣기 텍스트를 브래킷으로 감쌈
    - 커서 저장/복원 (모드 1048)
    - 동기화 출력 (모드 2026): 모드 상태를 추적. 변경사항은 항상 즉시 적용 (process→render 순차 파이프라인이므로 렌더러가 항상 최종 상태만 표시)
  - **디바이스 응답**: Device Status Report (DSR → `\x1b[0n`), Primary Device Attributes (DA → `\x1b[?1;2c`), Cursor Position Report (CPR → `\x1b[row;colR`)
  - **스크롤 리전 (DECSTBM)**: `CSI Pt;Pb r`로 스크롤 영역 설정. InsertLine/DeleteLine/LineFeed/Index/ReverseIndex가 스크롤 리전 내에서 동작

### 키보드 입력
- **중앙 키보드 디스패처**: 키보드 이벤트는 focused surface 타입에 따라 정확히 하나의 대상에만 전달됨
  - Terminal: PTY에 바이트 전송 (기존 경로)
  - Explorer/Markdown: `PendingKeyEvent` 큐에 저장 → 다음 egui 렌더 프레임에서 해당 패널이 소비
  - overlay(설정/다이얼로그/팝업) 열림 시에만 egui에 키보드 이벤트 전달 — 비활성 패널의 `ui.input()` 전역 오염 방지
- winit `KeyEvent.text`를 활용한 수정자 키 반영 (Ctrl+C 등 제어 문자 자동 처리)
- 특수 키 매핑: Enter, Backspace, Tab, Escape, 방향키, Home/End, PageUp/PageDown, Insert/Delete, F1~F12
- DECCKM 모드에 따른 방향키 시퀀스 자동 전환: 일반 모드 `\x1b[{A..D}` / 애플리케이션 모드 `\x1bO{A..D}`

### 스크롤백 버퍼
- 화면 위로 스크롤된 줄을 `VecDeque`에 보관하여 이전 출력을 다시 볼 수 있음
- 기본 10,000줄, 설정에서 0~100,000줄까지 조절 가능 (`scrollback_lines`)
- 마우스 휠로 스크롤백 탐색 (일반 모드), PageUp/PageDown으로 페이지 단위 이동
- 대체 화면(vim, less, htop 등)에서는 스크롤백 비활성 — 모든 입력이 PTY로 전달됨
- 키보드 입력(타이핑) 시 자동으로 최하단(라이브 뷰)으로 복귀
- 스크롤백 중에는 새 PTY 출력이 도착해도 스크롤 위치를 유지 — 새 라인이 추가되면 scroll_offset을 자동 보정하여 동일한 위치를 표시
- 스크롤 시 GPU 렌더러가 스크롤백 라인과 현재 화면 라인을 혼합하여 표시. 전각 문자(CJK, 한글 등)의 2셀 너비를 올바르게 반영하여 배치
- `ScrollRegionUp`(전체 화면 스크롤)과 `\n`(커서가 하단에 있을 때) 발생 시 최상단 줄 캡처
- 텍스트 wrap에 의한 implicit 스크롤(termwiz Surface가 `ScrollRegionUp` Change 없이 내부 처리하는 케이스)도 변경 전후 화면 스냅샷 비교로 감지하여 사라진 행을 scrollback에 기록 — 선택 영역의 `absolute_row`가 콘텐츠를 정확히 따라가도록 보장

### GPU 가속 렌더링
- wgpu 기반 크로스 플랫폼 GPU 렌더링
- `Arc<Window>` 기반 안전한 surface 생명주기 관리 (unsafe transmute 제거)
- 구조체 드롭 순서 보장: GPU 리소스가 윈도우보다 먼저 해제
- 인스턴스 렌더링 기반 2-pass 파이프라인:
  - Pass 1: 셀 배경색 쿼드
  - Pass 2: 알파 블렌딩 글리프 쿼드
- 다중 서피스를 단일 submit cycle 로 배치 (Phase F.G.a): `CellRenderer` 는 `begin_frame` / `append_terminal_viewport` / `flush_buffers` 로 모든 서피스 인스턴스를 누적한 뒤 kind 당 `queue.write_buffer` 1 회 + 하나의 encoder 로 제출. per-surface 오프셋은 인스턴스 attribute (`viewport_offset`) 에 박혀 있어 셰이더가 read
- WGSL 셰이더: NDC 변환, 텍스처 샘플링

### 폰트 래스터라이징
- cosmic-text FontSystem/SwashCache를 이용한 글리프 래스터라이징
- **번들 D2Coding ligature 폰트** (NAVER, OFL 1.1): Regular/Bold ttf를 바이너리에 임베드해 사용자의 OS에 D2Coding이 설치되지 않아도 동작. `font_family`가 빈 문자열이거나 `"monospace"`일 때 자동 적용되며, 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. `Shaping::Advanced`로 합자(`==`, `!=`, `=>`, `->`, `<=`, `>=` 등) 자동 적용. 한자/가나는 시스템 CJK 폰트로 자동 fallback
- 2048×2048 R8 텍스처 페이지에 선반(shelf) 기반 글리프 패킹
- 다중 페이지 글리프 아틀라스 + LRU eviction (Phase F.G.b): D2Array 4 layer (`MAX_PAGES = 4`, 페이지당 ~4 MiB) 로 확장. 활성 페이지가 가득 차면 다음 페이지로 이동, 모든 페이지 full 이면 활성 페이지를 제외한 가장 오래 미사용 (`last_access_frame` 최소) 페이지를 evict 후 해당 layer 만 zero-clear. 옛 *전체 아틀라스 리셋* 정책은 폐기 — CJK 세션의 주기적 stutter 해소
- 베이스라인 기반 글리프 오프셋 계산
- Bold/Italic 변형 지원 (Bold는 D2Coding ligature Bold face가 직접 매칭, synthetic bold 회피)
- Mask/Color/SubpixelMask 콘텐츠 타입별 그레이스케일 변환 (`chunks_exact` 사용)
- 블록 요소(U+2580–U+259F) 및 박스 드로잉(U+2500–U+257F) 커스텀 렌더링: 셀 경계를 정확히 채우는 픽셀 퍼펙트 비트맵을 프로그래밍 방식으로 생성. 상/하/좌/우 분할 블록, 쉐이드(25%/50%/75%), 사분면, light/heavy/double 선, 코너, T-접합, 크로스, 대각선 지원. Bold/Italic이 아닌 일반 스타일에서만 적용되며, 나머지는 swash 래스터라이저로 폴백

### 색상 지원
- xterm-256color 팔레트: ANSI 16색, 216색 큐브, 24단계 그레이스케일
- TrueColor (24-bit RGB) 지원
- SGR을 통한 전경색/배경색 개별 설정
- SGR 2 (dim, `Intensity::Half`): 전경색을 배경과 50:50 블렌딩하여 흐리게 렌더링 (reverse swap 후 적용). 디스크 스크롤백에서도 보존된다.

### 윈도우 관리
- winit 기반 크로스 플랫폼 윈도우 생성
- 리사이즈 시 뷰포트 유니폼 자동 갱신 및 터미널 그리드 재조정
- DPI 변경 감지: `ScaleFactorChanged` 이벤트 처리로 모니터 간 이동 시 스케일 팩터 자동 갱신
- 모노스페이스 폰트 기반 셀 그리드 레이아웃 (기본 14pt)
- **Windows 릴리스 빌드 서브시스템**: 릴리스 빌드는 `#![windows_subsystem = "windows"]`로 GUI 서브시스템으로 빌드되어 `tasty.exe` 실행 시 빈 콘솔 창이 뜨지 않음. 진입 직후 `AttachConsole(ATTACH_PARENT_PROCESS)`로 부모(ConPTY 포함) 콘솔에 attach하여 내부 pane에서의 CLI 호출 출력은 정상 표시. 디버그 빌드는 기존처럼 콘솔 창을 유지해 `tracing` 로그를 바로 확인 가능
- **Windows 자식 프로세스 콘솔 창 억제**: 호스트가 백그라운드로 spawn 하는 콘솔 서브시스템 자식 프로세스(플러그인 바이너리, `cmd`/`sh` 훅, agent 셸, Lua `run_cli` 등)에 `CREATE_NO_WINDOW` 플래그를 적용해 빈 콘솔 창이 뜨지 않음. `tasty_utils::process::hide_console` 헬퍼로 일원화(비-Windows no-op). pane 안에서 실제로 도는 사용자 셸은 `portable-pty`(ConPTY) 경로라 무관

### 이벤트 드리븐 렌더 루프
- `EventLoopProxy<AppEvent>` 기반 PTY 웨이크업: 터미널의 파서 스레드가 raw 바이트를 ingest(파싱·그리드 갱신)한 *직후* `AppEvent::TerminalOutput` 이벤트를 메인 이벤트 루프로 전송. 메인 루프는 파싱이 아니라 변경된 그리드의 렌더·이벤트 수집만 수행한다
- 무조건적 `request_redraw()` 제거: 이전에는 매 프레임 끝에 `request_redraw()`를 호출하여 VSync 기반 busy-loop을 실행했으나, 이제는 실제 변경이 있을 때만 redraw 요청
- 웨이크업 소스:
  - PTY 출력 → `AppEvent::TerminalOutput` → `user_event()` → `request_redraw()`
  - 키보드/마우스 입력 → `window_event()` → dirty 플래그 설정 → `request_redraw()`
  - 윈도우 리사이즈/포커스 → `window_event()` → dirty 플래그 설정 → `request_redraw()`
  - IPC 명령 → `process_ipc()` → dirty 플래그 설정 → `request_redraw()`
- `Waker` 타입 (`Arc<dyn Fn() + Send + Sync>`): Terminal 생성 시 전달되어 파서 스레드가 이벤트 루프를 깨울 수 있게 함
- Waker 전파 경로: `App` → `AppState` → `Workspace` → `Pane` → `Tab` → `Terminal`
- CPU 유휴 시 0% 사용: 터미널 출력이 없고 사용자 입력이 없으면 이벤트 루프가 대기 상태로 진입
