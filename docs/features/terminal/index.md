# 터미널 (Terminal)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent(입력 주입은 [terminal-output](../terminal-output/index.md)/`surface.send*`) · 원격(mirror)
- **ADR**: [ADR-0013](../../adr/0013-terminal-io-and-process-lifetime.md) — 파서 스레드와 PTY 수명·절전 복구. [ADR-0014](../../adr/0014-terminal-compatibility-scope.md) — 지원 범위와 인라인 그래픽 보류.
- **코드**: `crates/tasty-terminal/` (PTY·VTE·grid·scrollback), 렌더 `src/gfx/`
- **화면**: GPU 렌더링 셀 그리드 (egui 아님)

## 목적

`terminal` surface kind 의 본체 — PTY 셸 세션을 VTE 파싱해 셀 그리드로 에뮬레이트하고 GPU 로 그린다. host 내장 surface([work-area](../work-area/index.md)의 Surface 종류).

## 내부 동작

### PTY 셸

ConPTY(Windows) / Unix PTY 로 네이티브 셸 실행(`TERM=xterm-256color`). 윈도우 리사이즈 시 자식에 새 크기 전파 — rows 축소 시 커서 아래 빈 행 먼저 제거 후 부족분은 위쪽 행을 scrollback 으로 캡처(커서-콘텐츠 관계 보존), 확대 시 scrollback 에서 복원.

**작업 디렉토리 상속**: 새 surface 생성 시 소스의 현재 cwd 를 상속(`general.inherit_cwd`, 기본 on). macOS/Linux 는 셸 PID 로 OS 직접 조회(`proc_pidinfo` / `/proc/<pid>/cwd`, OSC 7 캐시 우선), Windows 는 타 프로세스 cwd API 부재로 셸이 내보내는 OSC 7 캐시에만 의존(합성 rcfile 로 OSC 7 emit 강제). 합성 rcfile 은 OSC 0(cwd 기반 탭 제목, `__tasty_title`)도 함께 보내며, 빌트인 블록에 버전 스탬프(`# tasty-bashrc-v<N>`)를 심어 스탬프가 다른 기존 `~/.tasty/bashrc`·`bashrc.default` 를 셸 spawn 시 자동 재생성한다(`ensure_compiled_bashrc_in` — 사용자 편집 영역 `bashrc.user` 는 보존). carry 규칙은 [surface-cwd invariant](../../design/policies/cwd.md#surface-cwd-invariant).

### 프로세스 종료 / 절전 복귀

자식 종료를 확인하면 ProcessExited를 한 번 발생시키고 훅 통지와 surface 정리를 수행한다.
`try_wait`는 보통 Terminal을 깨워 처리할 때 실행하며 확인 간격은 500ms다. PTY EOF 뒤에는 즉시 확인한다.

EOF와 자식 종료는 같은 사건이 아니다. EOF 직후 자식이 아직 실행 중이면 parser가 10ms부터
간격을 두 배씩 늘려 최대 500ms마다 다시 깨운다. 종료가 확인되거나 take_child로 소유권을 넘기거나
Terminal이 사라지면 멈춘다. 이 처리가 없으면 출력이 끝난 뒤 자식 종료를 확인할 기회가 사라질 수 있다.

자식은 PTY를 소유한 host와 수명을 함께한다. Windows는 KILL_ON_JOB_CLOSE Job Object를 사용한다.
Unix는 비정상 host 종료 때 PTY hangup과 SIGHUP에 의존하므로 HUP을 무시하는 자식이 남을 수 있다.
정상 surface 닫기는 명시적인 종료와 회수를 수행한다. attach client만 끊는 것은 server의 자식을 종료하지 않는다.

Windows 절전 복귀는 WM_POWERBROADCAST로 감지한다.
종료한 자식을 정리하고, 살아 있는 ConPTY에는 현재 크기의 resize를 보내 처리를 재개하도록 유도한다.
복구되지 않을 수 있는 surface는 사용자에게 알린다. 멈춤과 정상 대기를 구분할 수 없으므로
자동 강제 종료·재생성은 하지 않으며 최종 재시작은 사용자가 결정한다.
macOS·Linux에는 이 Windows 전용 처리를 적용하지 않는다.

### VTE 에뮬레이션

termwiz가 VT 시퀀스를 파싱하고 셀 grid를 갱신한다. 주요 지원 범위는 다음과 같다.

| 종류 | 동작 |
|---|---|
| 텍스트와 제어코드 | LF, CR, BS, Bell. HT는 기본 8칸 간격의 탭 정지점을 사용한다. HTS/CTC로 설정하고 TBC로 해제한다. RIS와 리사이즈 때 기본값을 재구성한다. |
| 문자 속성 | SGR intensity, underline, italic, blink, inverse, strikethrough, 전경·배경색. dim은 배경과 50:50으로 섞어 그린다. |
| 커서 | CUP, CHA, VPA, CNL, CPL, CHT, CBT, 저장·복원. DECSCUSR의 블록·바·밑줄은 그리되 blink 변형도 정적으로 표시한다. |
| 화면 편집 | ED, EL, SU, SD, DCH, ICH, DL, IL, ECH, REP. DCH/ICH는 전각 두 셀을 함께 처리한다. |
| ESC | DECSC, DECRC, IND, RI, NEL, HTS, DECALN, RIS. DECALN은 화면을 E로 채운다. |
| 문자 집합 | G0/G1 DEC line drawing과 SO/SI 전환. 활성 상태의 ASCII 0x60–0x7e를 박스 문자로 바꾼다. UK는 ASCII로 처리하고 RIS로 초기화한다. |
| 소프트 리셋 | DECSTR은 마진·저장 커서·문자 속성·앱 커서 키·삽입 모드·커서 가시성을 초기화한다. 화면과 대체 화면은 유지한다. |
| 표준 모드 | IRM(4) 삽입/덮어쓰기와 ShowCursor(25). KAM, SRM, LNM 등은 무시한다. |

지원하는 DEC 모드는 DECCKM(1), DECTCEM(25), 대체 화면(47/1047/1049),
마우스(1000/1002/1003/1006), 포커스 추적(1004), bracketed paste(2004), 동기화 출력(2026)이다.
DECSCNM(5)은 기본 전경·배경을 바꿔 그린다. `general.reverse_screen_enabled`를 끄면
모드 상태와 질의 응답은 유지하면서 화면 반전만 표시하지 않는다. readline의 visible bell도 이 설정으로 억제할 수 있다.
DECOM(6)은 절대 커서 위치를 스크롤 영역 기준으로 해석하고, 설정·해제 시 home으로 이동한다.
상대 커서 이동을 영역 안에 가두는 동작은 아직 구현하지 않았다.

### 터미널 질의와 OSC

| 시퀀스 | 응답·동작 |
|---|---|
| DSR/CPR | 상태·커서 위치 응답 |
| DA1 / DA2 / DA3 | `CSI ?1;2c` / `CSI >0;10;0c` / `DCS !\|54415354 ST` |
| XTVERSION | `DCS >\|tasty(<ver>) ST`; 버전은 tasty-terminal 크레이트 기준 |
| XtGetTcap | 요청별로 `DCS 0+r <hexcap> ST`. 능력 DB가 없어 현재 미지원임을 알리고 요청 hex를 그대로 돌려준다. |
| XTWINOPS | 셀 크기 `18 t`→`CSI 8;rows;cols t`, `19 t`→`CSI 9;rows;cols t`. 제목 push/pop `22/23 t`는 단일 제목과 최대 64개 항목을 사용한다. |
| OSC 8 | 이후 출력 셀에 하이퍼링크 URI를 붙이고 빈 URI로 해제한다. DECSTR/RIS도 속성을 초기화한다. |
| OSC 52 읽기 | host가 `general.allow_clipboard_read`를 확인한 뒤 응답한다. 기본 off이면 응답하지 않는다. [클립보드](../clipboard/index.md) 참조. |
| OSC 10/11/12, OSC 4 질의 | 현재 테마의 전경·배경·커서·ANSI 색을 `rgb:RRRR/GGGG/BBBB`와 ST로 응답한다. host가 생성·테마 변경 시 팔레트를 전달한다. 커서는 전경색이며, 팔레트가 없으면 응답하지 않는다. 색 설정 시퀀스는 무시한다. |

창 이동·크기 변경·최대화·최소화·전체화면·앞뒤 순서 변경은 지원하지 않는다.
창 위치·상태·제목 조회와 픽셀 크기 조회(`14/16 t`)도 응답하지 않는다.
지원 범위와 보류 이유는 [터미널 호환성 결정](../../adr/0014-terminal-compatibility-scope.md)을 참고한다.

### 마우스 입력

트래킹 중 클릭·드래그·휠은 앱으로 보낸다. 1000/1002/1003은 각각 독립적으로 켜고 끄며,
동시에 켜져 있으면 가장 넓은 모드(1003, 1002, 1000 순서)를 적용한다.
RIS는 모두 끄고 DECSTR은 유지한다. SGR(1006)을 협상하지 않으면 X10으로 인코딩한다.
SGR 버튼 코드는 왼쪽 0, 가운데 1, 오른쪽 2이며 motion은 32를 더한다.
modifier는 Shift 4, Alt 8, Ctrl 16이다. 휠·버튼·debug 주입은 같은 인코더를 쓴다.

- 1002와 1003은 눌린 버튼의 드래그를 보고한다. 여러 버튼을 누르면 최근에 누른 버튼이 기준이다.
  그 버튼을 놓으면 아직 눌린 버튼을 사용한다. 앱에 press를 보낸 버튼만 motion을 보낸다.
- 드래그 대상은 press 시점에 고정하고 좌표를 그 surface 안으로 제한한다.
- 1003의 버튼 없는 hover는 `ESC[<35;col;rowM`이다. OS 창과 surface가 포커스된 경우에만 보내며
  셀이 달라졌을 때 전송한다. 중복 제거 키에는 surface ID도 포함한다.
  분할선·창 리사이즈 가장자리·분할선 드래그 중에는 보내지 않는다. 1002는 hover를 보내지 않는다.
- Shift+우클릭은 로컬 메뉴를 열고 Shift+좌클릭 드래그는 로컬 텍스트를 선택한다.
  Shift 여부는 press 때 결정해 release까지 유지하므로 도중에 Shift를 놓아도 선택이 이어진다.
  더블클릭은 단어, 트리플클릭은 줄을 선택하며 release도 앱에 보내지 않는다.
- 트래킹 중 Shift+좌클릭은 새 선택을 시작한다. 트래킹이 꺼졌을 때의 Shift+클릭은 기존 선택을 확장한다.
- macOS 미들클릭 보고는 구현되어 있으나 대상 앱의 paste 동작은 확인되지 않았다.

### 마우스 캡처 안내와 앱별 설정

`general.mouse_capture_hint`는 안내 배너를 켜고 끈다(기본 on).
옛 키 `right_click_capture_hint`도 설정 별칭으로 읽는다.
좌·우 일반 클릭 중 처음 발생한 캡처 입력에서 Shift 선택·메뉴 방법을 안내한다.
트래킹을 켤 때 표시 가능 상태가 되고, 한 번 표시하거나 트래킹을 끄거나 RIS를 받으면 해제한다.

| 설정 | 효과 |
|---|---|
| `general.mouse_capture_blacklist` | 해당 앱의 클릭·버튼·드래그를 로컬에서 처리한다. 휠은 계속 앱에 보내고 안내 배너는 표시하지 않는다. |
| `general.mouse_capture_banner_blacklist` | 앱의 마우스 입력은 유지하고 안내 배너만 숨긴다. |

두 목록의 기본값은 비어 있다. 프로세스 이름에서 `.exe`를 제거하고 소문자로 바꾼 뒤
substring 또는 `*` glob으로 비교한다. 1Hz foreground 조회 결과를 재사용한다.
배너만 숨긴 경우에는 표시 가능 상태를 소비하지 않으므로, 같은 세션에서 다른 앱으로 바뀌면 안내가 나올 수 있다.
원격 mirror는 로컬 foreground 프로세스가 없어 이 목록을 적용하지 않는다.

배너의 더보기 메뉴는 Settings와 같은 목록을 수정한다. 알림 끄기는 배너를 즉시 닫고,
캡처 비활성화는 배너를 남겨 사용자가 안내를 읽고 닫을 수 있게 한다.
더보기 버튼은 hover 중 또는 메뉴가 열린 동안 표시한다. 메뉴는 `PopupDef`의 headless 컨텍스트 메뉴로
트리거 아래에 우측 정렬하고, 공간이 없으면 위쪽에 연다. 프로그램 이름은 별도 텍스트 구간으로 그려
긴 번역문 때문에 이름부터 잘리지 않게 한다. 되돌릴 수 있는 설정이므로 위험 동작 색을 쓰지 않는다.

배너가 열린 때의 foreground generation을 저장하고, 프로그램 이름이 바뀌면 1Hz 조회에서 닫는다.
다른 종류의 배너를 닫지 않도록 ID를 확인한다. 같은 배너 종류라도 generation이 다르면 교체한다.
같은 이름의 프로그램이 연속 실행된 경우와 원격 mirror에서는 이 방법으로 종료를 구별할 수 없다.

### 스크롤 영역과 소거

DECSTBM 영역은 명시 개행과 오른쪽 끝의 자동 줄바꿈 모두에 적용한다.
영역 하단에서 줄바꿈하면 그 영역만 위로 밀고 커서는 하단 열 0에 남는다.
영역 밖에 둔 TUI 입력창은 덮지 않는다.

- 영역 상단이 화면 0행이면 밀려난 행을 scrollback에 순서대로 보관한다.
  내부 영역은 화면 밖으로 나가는 행이 없으므로 scrollback에 넣지 않는다.
- 커서가 영역 아래의 마지막 화면 행에 있으면 화면과 영역을 스크롤하지 않고 같은 행에서 줄바꿈한다.
- 대체 화면도 같은 영역 규칙을 쓰지만 primary scrollback에 기록하지 않는다.
- 마진은 저장할 때 grid 안으로 정규화한다. 하단은 마지막 행까지 제한하고,
  상단이 하단보다 크거나 화면 밖이면 한 줄 영역으로 정리한다.
- DECAWM으로 자동 줄바꿈을 끄는 기능은 현재 미지원이다. 꺼 달라는 요청에도 위 줄바꿈은 일어난다.

ED/EL은 커서 위치를 유지하며 현재 셀을 포함해 지운다.
마지막 셀까지 출력한 뒤 줄바꿈을 기다리는 커서는 `cx == cols`로 표현되지만,
지울 범위는 실제 마지막 셀에서 계산한다. 소거 뒤에도 줄바꿈 대기는 유지한다.

| 명령 | 줄바꿈 대기 커서에서 지울 범위 | 0열의 경계 사례 |
|---|---|---|
| EL0 | 마지막 한 셀 | 커서부터 행 끝 |
| EL1 | 현재 행 전체 | 한 셀 |
| EL2 | 현재 행 전체 | 행 전체 |
| ED0 | 마지막 셀과 아래 행 전체 | 커서부터 화면 끝 |
| ED1 | 현재 행과 위 행 전체 | 커서 행 한 셀과 위 행 전체 |
| ED2 | 화면 전체 | 화면 전체 |

지운 셀은 기본 문자 속성과 소거 당시 배경색만 갖는다. 밑줄·역상·굵기 등은 복사하지 않는다.
소거 뒤 pen은 이전 값으로 복원해 다음 문자가 원래 속성으로 출력되게 한다.
이는 `TERM=xterm-256color`의 BCE 선언에 맞춘 동작이다.

소거 구현은 `vte_handler/edit.rs`의 erase_attrs·erase_color·restore_pen을 사용한다.
검증은 여섯 명령, 커서 행, 줄바꿈 대기 유무를 조합하고 지운 셀과 소거 직후 문자의 속성을 각각 확인한다.
기본 배경과 밑줄을 함께 시험해야 배경만 복사한다는 조건을 확인할 수 있다.
줄바꿈 대기의 EL0/ED0은 과거 tmux 3.4 비교와 다르다. Tasty는 마지막 셀을 지운다.

파싱은 터미널별 reader 스레드에서 수행하고 main 루프는 렌더링과 이벤트만 처리한다.
공유 grid는 8KB 청크 처리 후 락을 놓는다. 자세한 선택 이유는
[PTY 처리 결정](../../adr/0013-terminal-io-and-process-lifetime.md)을 참고한다.

### 스크롤백

화면 위로 밀린 줄을 `VecDeque` 에 보관(`scrollback_lines`, 기본 10,000, 0~100,000 설정). 마우스 휠/PageUp·Down 탐색, 타이핑 시 자동 라이브 뷰 복귀. 대체 화면(vim/less/htop)에선 스크롤백 비활성(모든 입력 PTY 로). 스크롤백 중 새 출력 도착 시 `scroll_offset` 자동 보정으로 위치 유지. 텍스트 wrap 에 의한 implicit 스크롤도 기록한다 — 출력 텍스트를 스크롤이 일어날 바이트 위치에서 잘라 적용하며 밀려나기 직전의 상단 행을 회수하므로, 사라진 행이 빠짐없이 순서대로 남는다(선택 영역 `absolute_row` 가 콘텐츠를 정확히 추적). 부분 스크롤 영역에서의 적재 범위는 위 "스크롤 영역과 소거" 절 참조. 세션 간 보존은 disk scrollback. **ED3(`CSI 3J`)** 는 스크롤백 히스토리(메모리+디스크)를 비우고 뷰포트를 라이브로 되돌린다 — 화면 내용은 보존(`clear` 가 보내는 `\x1b[3J\x1b[2J` 에서 ED2 가 화면을, ED3 가 스크롤백을 담당).

### 키보드 입력

**앱별 Shift+Enter**: 설정 → 터미널 → 입력에서 앱을 추가하고 LF 전송을 켜거나 끄며 규칙을 삭제한다. 저장 모델은 `Settings.terminal_input.rules`의 `app`(실행 파일명)과 `shift_enter_newline`이다. 값이 true인 앱에서 Shift만 누른 Enter는 LF 한 바이트를 보낸다. false 또는 미일치는 아래 플랫폼 기본 인코딩을 쓴다. basename의 대소문자와 `.exe`를 정규화한 정확한 이름으로 매칭하며 부분 문자열이나 `node` 런처를 Claude로 추정하지 않는다. foreground는 기존 약 1초 주기 프로세스 캐시를 재사용하므로 앱 전환 직후에는 다음 갱신까지 지연될 수 있다. 붙여넣기·IME·IPC 텍스트와 호스트에서 소비한 키는 이 규칙을 거치지 않는다.

CLI는 `tasty settings get-input-rules`, `set-input-rule --app claude --shift-enter-newline true`, `remove-input-rule --app claude`이다. 해당 IPC는 `settings.get_input_rules`, `settings.set_input_rule`, `settings.remove_input_rule`이며 전역 설정 저장·전 윈도우 반영 경로를 공유한다. `settings.initialize_input_rule`은 호출자와 앱별 최초 기본값만 등록한다. 기존 사용자 규칙은 우선하며 최초 등록 이력은 삭제 후에도 남아 재시작으로 규칙을 복구하지 않는다. Claude 플러그인은 최초 활성화에서 `claude`의 LF 규칙을 등록한다(이미 설치된 플러그인도 새 버전의 최초 활성화에서 등록).

**Windows 줄바꿈 키**: Shift+Enter와 Ctrl+J는 ConPTY의 win32-input 시퀀스(`CSI Vk;Sc;Uc;Kd;Cs;Rc _`)로 key-down/up 한 쌍을 보낸다. Shift+Enter는 VK_RETURN·CR·SHIFT_PRESSED, Ctrl+J는 VK_J·LF·LEFT_CTRL_PRESSED를 보존한다. CSI-u는 ConPTY에서 소실되고, bare LF는 VK_RETURN으로 변환되어 Windows 네이티브 콘솔 앱이 Ctrl+J와 구분하지 못하기 때문이다. 보조키 좌우는 구분하지 않는다. 일반 Enter와 다른 Ctrl+문자는 기존 제어문자를 보내고, Unix에서는 Shift+Enter의 CSI-u와 Ctrl+J의 LF를 유지한다. 별도 설정은 없다. 프로토콜 필드는 [Microsoft Terminal 구현](https://github.com/microsoft/terminal/blob/main/src/terminal/input/terminalInput.cpp)의 `_makeWin32Output`을 따른다. 인코딩 회귀는 `src/view/main/keyboard.rs`의 `newline_keys_*` 시험이 검사하며 실제 콘솔 수신은 Windows에서 별도로 검증한다.

중앙 키보드 디스패처가 focused surface 타입에 따라 정확히 한 대상에만 전달 — Terminal 은 PTY 로 바이트. 특수 키(Enter/Backspace/Tab/Escape/방향키/Home·End/PageUp·Down/Insert·Delete/F1~F12) 매핑, DECCKM 모드에 따라 방향키 시퀀스 전환(`\x1b[{A..D}` ↔ `\x1bO{A..D}`). 복사/붙여넣기/선택/IME 는 [clipboard](../clipboard/index.md).

방향키와 함께 누른 Shift·물리 Alt(macOS Option)·Ctrl은 xterm 방식 `CSI 1 ; m A/B/C/D`로 전달한다. `m`은 1 + Shift(1) + Alt(2) + Ctrl(4)이며, 보조키가 있으면 DECCKM on/off 모두 CSI를 쓴다. 예: Option/Alt+↑는 `\x1b[1;3A`, Ctrl+Shift+←는 `\x1b[1;6D`. 보조키 없는 방향키는 위 DECCKM 규칙을 유지한다. 근거: [xterm cursor-key 및 modifier 표](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-PC-Style-Function-Keys)와 [modifyCursorKeys](https://invisible-island.net/xterm/manpage/xterm.html#VT100-Widget-Resources:modifyCursorKeys).

이 인코딩은 `option_as_meta`와 독립이다(그 설정은 문자 입력만 제어). 호스트 바인딩의 `alt` 토큰(macOS Command)으로 물리 Option을 치환하지 않으며, Super/Command 자체는 방향키 modifier 값에 넣지 않는다. 호스트 단축키·오버레이·vi 복사 모드가 소비한 키는 PTY로 중복 전달하지 않는다. 인코딩 결정은 `src/view/main/keyboard.rs`의 `decide_key_to_terminal`이 담당하고, 문자 Meta/compose와 IME Commit 경로는 별도로 처리한다.

**Option as Meta (macOS 전용)**: 설정 `general.option_as_meta`(기본 off, macOS 빌드에만 존재). on 이면 Option(Alt)+문자 입력이 특수문자(compose, 예 `å`) 대신 `ESC` + base 문자 Meta 시퀀스(예 `Option+a` → `\x1b a`)로 PTY 에 전달돼 readline/Emacs/vim 의 Meta 바인딩(`Alt+f`/`Alt+b` 등)이 동작한다. 물리 Option 키만 대상이며(Ctrl/Cmd 동시 누름 시 제외), 좌/우 Option 을 구분하지 않는다. base 문자는 Option 합성 이전의 US 레이아웃 문자(`physical_key_to_logical`)라 비-US 레이아웃에서는 US 기준으로 인코딩되는 한계가 있다. off 면 기존 Option=특수문자 동작을 보존한다. 키바인딩이 아니라 입력 인코딩 동작이라 `GeneralSettings` 에 둔다(`KeybindingSettings` 무관). 다른 OS 에는 Option 키가 없어 설정·UI 모두 노출하지 않는다.

### 색상 / 폰트

xterm-256color(ANSI 16 + 216 큐브 + 24 그레이) + TrueColor. 색은 Theme 의 ansi 팔레트([theme](../../design/systems/theme.md)). 폰트는 번들 D2Coding ligature(OFL 1.1, 임베드 — OS 미설치에도 동작), CJK fallback, 블록/박스 드로잉 글리프는 픽셀 퍼펙트 커스텀 렌더. 번들 폰트 파일은 합자 글리프를 포함하지만(폰트 자원), tasty 는 셀-격자 cross-cell 합자(프로그래밍 ligature) 적용도 설정 토글도 **미지원** — 보류 결정은 [ADR-0014](../../adr/0014-terminal-compatibility-scope.md). 렌더 파이프라인(누적→flush→단일 패스, atlas LRU)은 [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md).

### 이벤트 드리븐 렌더

파서 스레드가 ingest 직후 `AppEvent::TerminalOutput` 으로 메인 루프를 깨운다(`Waker`). 메인은 파싱이 아니라 변경된 grid 의 렌더·이벤트 수집만. 무조건 redraw 제거 — 실제 변경 시에만. **가시성 게이트**: 안 보이는 surface(비활성 워크스페이스/탭)는 출력을 항상 drain·파싱하되 `request_redraw` 는 생략(데이터 무손실, 재렌더만 절약). 출력·입력이 없을 때 불필요한 렌더링을 줄인다.

Windows 에서는 focused terminal cursor 를 프로그램 주도 화면 갱신 뒤 짧게 숨긴다. VTE 파서가 carriage return, CSI cursor/edit, cursor save/restore action 을 실제로 본 뒤 120ms 동안 적용한다. Codex 같은 redraw-heavy CLI 가 커서 이동/줄 지우기를 빠르게 내보낼 때 중간 cursor hop 을 화면에 드러내지 않기 위한 렌더 정책이다. 단순 printable echo 는 화면 제어 action 이 아니므로 사용자가 직접 타이핑하는 동안 커서를 숨기지 않으며, 입력 직후라도 프로그램이 화면 제어 action 을 출력하면 억제를 적용한다. 커서를 숨긴 프레임은 후속 redraw 를 예약해 화면 갱신이 조용해진 뒤 커서가 다시 나타나게 한다. 다른 OS 에서는 이 억제 정책을 적용하지 않는다.

## 인터페이스

- **사용자**: 키보드/마우스 직접 입력.
- **AI Agent**: 입력 주입·출력 읽기는 [terminal-output](../terminal-output/index.md) + `surface.send*` ([reference/api](../../reference/api.md#surface-상호작용)). 터미널 surface 생성/닫기는 [work-area](../work-area/index.md).
- **원격**: PTY 없는 detached mirror 로 grid 재구성 ([remote-attach](../remote-attach/index.md)).

## 비-목표

- **인라인 그래픽**(Sixel/Kitty/iTerm 이미지) — 보류([ADR-0014](../../adr/0014-terminal-compatibility-scope.md)). 이미지는 [image surface](../../plugins/image/index.md).
- **XTWINOPS 창 조작·창 탐침·픽셀 크기 리포트** — 미지원([ADR-0014](../../adr/0014-terminal-compatibility-scope.md), 사용자/에이전트 분리).
- **tmux control mode(DCS)·DECRQSS** — 미지원([ADR-0014](../../adr/0014-terminal-compatibility-scope.md), 범위 밖/드묾).
- **일부 사설 입력 모드** — Utf8Mouse(1005), SGRPixels(1016), Win32InputMode(9001), DECCOLM(3), ReverseWraparound(45), Meta/AltSendsEscape(1036/1039), GraphemeClustering(2027)은 지원하지 않는다. 표준 입력 방식으로 대신할 수 있어 구현을 보류했다([ADR-0014](../../adr/0014-terminal-compatibility-scope.md)).
- 렌더/폰트 atlas 내부 구현 — [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md).

## 관련

- [terminal-search](../terminal-search/index.md) · [terminal-link](../terminal-link/index.md) · [clipboard](../clipboard/index.md)
- [ADR-0013](../../adr/0013-terminal-io-and-process-lifetime.md) · [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md)

## 휠 스크롤 거리

`general.wheel_line_scroll`은 휠 한 칸을 논리 포인트로 바꾸는 값이며 기본값은 50이다.
설정은 egui `Options::line_scroll_speed`에 적용한다. host 위젯, modifier hint,
plugin surface·popup·banner와 attach mesh가 모두 이 값을 읽는다.
plugin에 보내는 Scroll도 논리 포인트이므로 재현 로그에서 50을 상수로 가정하지 않는다.
터미널 앱으로 전달하는 휠 보고처럼 단위를 그대로 전달하는 경로는 이 포인트 환산과 구분한다.

검증은 기본값이 아닌 값을 egui 컨텍스트에 넣고 host·와이어 변환·overlay에 동일하게 적용되는지 본다.
새 LineDelta 처리 경로가 생기면 자체 상수를 추가하지 않았는지도 확인한다.
실행 상태 표시는 [busy 정책](../../design/policies/busy-indicator.md)을 따른다.

## 사용자 입력 기록

키보드·IME·붙여넣기는 `CoreState::record_typing`으로 기록한다.
`surface.is_typing`은 최근 5초의 입력 여부와 마지막 입력 후 `idle_seconds`를 반환하며,
기록이 없으면 `idle_seconds`는 -1이다. 에이전트 send/tell은 기록하지 않는다.

붙여넣기는 `MainView::run_paste` 시작에서 기록한다. 단축키·명령 팔레트·이미지·plugin 붙여넣기가
같은 함수를 사용한다. 빈 클립보드나 붙여넣기 미지원 surface에서도 시도는 기록한다.
원격 이미지 업로드 완료 시점에 다시 기록하지 않으므로 긴 업로드 뒤의 typing은 이미 만료될 수 있다.
마우스 보고·휠·클릭 커서 이동과 새 탭을 여는 파일 드롭은 이 기록의 대상이 아니다.

`--wait-idle` 전송과 자동 재개는 이 기록으로 사용자의 입력을 보호한다.
새 붙여넣기 경로는 `run_paste`를 사용하며, 명령 팔레트 회귀 검증은
`tests/gui_tests.rs`의 `test_palette_paste_records_user_typing`으로 수행한다.
