# 터미널 (Terminal)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent(입력 주입은 [terminal-output](../terminal-output/index.md)/`surface.send*`) · 원격(mirror)
- **ADR**: [ADR-0002](../../adr/0002-vte-parsing-off-input-thread.md)(파서 스레드) · [ADR-0008](../../adr/0008-inline-graphics-protocols-deferred.md)(인라인 그래픽 보류)
- **코드**: `crates/tasty-terminal/` (PTY·VTE·grid·scrollback), 렌더 `src/gfx/`
- **화면**: GPU 렌더링 셀 그리드 (egui 아님)

## 목적

`terminal` surface kind 의 본체 — PTY 셸 세션을 VTE 파싱해 셀 그리드로 에뮬레이트하고 GPU 로 그린다. host 내장 surface([work-area](../work-area/index.md)의 Surface 종류).

## 내부 동작

### PTY 셸

ConPTY(Windows) / Unix PTY 로 네이티브 셸 실행(`TERM=xterm-256color`). 윈도우 리사이즈 시 자식에 새 크기 전파 — rows 축소 시 커서 아래 빈 행 먼저 제거 후 부족분은 위쪽 행을 scrollback 으로 캡처(커서-콘텐츠 관계 보존), 확대 시 scrollback 에서 복원.

**작업 디렉토리 상속**: 새 surface 생성 시 소스의 현재 cwd 를 상속(`general.inherit_cwd`, 기본 on). macOS/Linux 는 셸 PID 로 OS 직접 조회(`proc_pidinfo` / `/proc/<pid>/cwd`, OSC 7 캐시 우선), Windows 는 타 프로세스 cwd API 부재로 셸이 내보내는 OSC 7 캐시에만 의존(합성 rcfile 로 OSC 7 emit 강제). carry 규칙은 [surface-cwd invariant](../../architecture/invariants/surface-cwd.md).

### VTE 에뮬레이션

termwiz `Parser`/`Surface` 로 VT 시퀀스를 파싱·grid 갱신. 지원: 텍스트·제어코드(LF/CR/BS/HT/Bell) · SGR(intensity/underline/italic/blink/inverse/strikethrough + fg/bg, **SGR 2 dim** = bg 50:50 블렌딩) · 커서 이동(CUP/CHA/VPA/CNL/CPL/save·restore) · 화면 편집(ED/EL/SU/SD/DCH/ICH/DL/IL/ECH — DCH/ICH 는 전각 2셀 처리) · ESC(DECSC/DECRC/IND/RI/RIS) · 스크롤 리전(DECSTBM) · 디바이스 응답(DSR/DA/CPR). **DECSET/DECRST 모드**: DECCKM(1, 앱 커서 키) · DECTCEM(25, 커서 가시성) · 대체 화면(47/1047/1049) · 마우스 트래킹(1000/1002/1003) · SGR 마우스(1006) · 포커스 트래킹(1004) · bracketed paste(2004) · 동기화 출력(2026). **표준 모드(SM/RM, `?` 없는 `CSI .. h/l`)**: IRM(4, 삽입/덮어쓰기 — on 이면 출력 글리프가 기존 셀을 우측으로 밀어 ICH 처럼 삽입) · ShowCursor(25, DECTCEM 과 동일 동작). 그 외 표준 모드(KAM/SRM/LNM 등)는 무시.

> **파서 스레드 분리**: 터미널마다 파서 스레드가 PTY raw 바이트를 읽는 즉시 그 스레드에서 VTE 파싱·grid 갱신(`ingest`). 메인(winit) 루프는 파싱하지 않아 백그라운드 터미널 출력이 포그라운드 입력/IPC 를 막지 않는다. grid 는 `Arc<Mutex<_>>` 공유, 8KB 청크마다 락 잡고 즉시 해제. 근거·대안 [ADR-0002](../../adr/0002-vte-parsing-off-input-thread.md).

### 스크롤백

화면 위로 밀린 줄을 `VecDeque` 에 보관(`scrollback_lines`, 기본 10,000, 0~100,000 설정). 마우스 휠/PageUp·Down 탐색, 타이핑 시 자동 라이브 뷰 복귀. 대체 화면(vim/less/htop)에선 스크롤백 비활성(모든 입력 PTY 로). 스크롤백 중 새 출력 도착 시 `scroll_offset` 자동 보정으로 위치 유지. 텍스트 wrap 에 의한 implicit 스크롤도 화면 스냅샷 비교로 감지해 사라진 행을 기록(선택 영역 `absolute_row` 가 콘텐츠를 정확히 추적). 세션 간 보존은 disk scrollback. **ED3(`CSI 3J`)** 는 스크롤백 히스토리(메모리+디스크)를 비우고 뷰포트를 라이브로 되돌린다 — 화면 내용은 보존(`clear` 가 보내는 `\x1b[3J\x1b[2J` 에서 ED2 가 화면을, ED3 가 스크롤백을 담당).

### 키보드 입력

중앙 키보드 디스패처가 focused surface 타입에 따라 정확히 한 대상에만 전달 — Terminal 은 PTY 로 바이트. 특수 키(Enter/Backspace/Tab/Escape/방향키/Home·End/PageUp·Down/Insert·Delete/F1~F12) 매핑, DECCKM 모드에 따라 방향키 시퀀스 전환(`\x1b[{A..D}` ↔ `\x1bO{A..D}`). 복사/붙여넣기/선택/IME 는 [clipboard](../clipboard/index.md).

### 색상 / 폰트

xterm-256color(ANSI 16 + 216 큐브 + 24 그레이) + TrueColor. 색은 Theme 의 ansi 팔레트([theme](../../design/systems/theme.md)). 폰트는 번들 D2Coding ligature(OFL 1.1, 임베드 — OS 미설치에도 동작), 합자·CJK fallback, 블록/박스 드로잉 글리프는 픽셀 퍼펙트 커스텀 렌더. 렌더 파이프라인(누적→flush→단일 패스, atlas LRU)은 [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md).

### 이벤트 드리븐 렌더

파서 스레드가 ingest 직후 `AppEvent::TerminalOutput` 으로 메인 루프를 깨운다(`Waker`). 메인은 파싱이 아니라 변경된 grid 의 렌더·이벤트 수집만. 무조건 redraw 제거 — 실제 변경 시에만. **가시성 게이트**: 안 보이는 surface(비활성 워크스페이스/탭)는 출력을 항상 drain·파싱하되 `request_redraw` 는 생략(데이터 무손실, 재렌더만 절약). 출력·입력 없으면 CPU 0%.

## 인터페이스

- **사용자**: 키보드/마우스 직접 입력.
- **AI Agent**: 입력 주입·출력 읽기는 [terminal-output](../terminal-output/index.md) + `surface.send*` ([reference/api](../../reference/api.md#surface-상호작용)). 터미널 surface 생성/닫기는 [work-area](../work-area/index.md).
- **원격**: PTY 없는 detached mirror 로 grid 재구성 ([remote-attach](../remote-attach/index.md)).

## 비-목표

- **인라인 그래픽**(Sixel/Kitty/iTerm 이미지) — 보류([ADR-0008](../../adr/0008-inline-graphics-protocols-deferred.md)). 이미지는 [image surface](../../plugins/image/index.md).
- 렌더/폰트 atlas 내부 구현 — [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md).

## 관련

- [terminal-search](../terminal-search/index.md) · [terminal-link](../terminal-link/index.md) · [clipboard](../clipboard/index.md)
- [ADR-0002](../../adr/0002-vte-parsing-off-input-thread.md) · [dev-guide/gpu-rendering](../../dev-guide/gpu-rendering.md)
