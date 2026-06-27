# 클립보드 (Clipboard)

- **Status**: Implemented
- **주체**: 로컬 사용자 (복사/붙여넣기는 사용자 행동)
- **ADR**: [ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) (뷰어의 plugin 직접-read 모델)
- **코드**: 시스템 클립보드 `arboard`; 복사/붙여넣기/선택 = `src/view/main/`
- **화면**: 현재 클립보드 뷰어는 [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md)

## 목적

터미널 텍스트의 **복사/붙여넣기/선택**, OSC 52 클립보드 설정. 복사/붙여넣기는 사용자 행동이라 토스트로 피드백하지만, 에이전트(IPC)·OSC 52 경로는 사용자 시각 상태를 건드리지 않는다([toast](../../design/systems/toast.md) 트리거 정책).

> **히스토리 기능 제거됨.** 과거 host 가 OS 클립보드를 폴링해 누적하던 클립보드 히스토리(메모리 `ClipboardHistory` + DB 테이블 + `tasty clipboard` CLI + `tool.clipboard.*` IPC + `clipboard.copied` 이벤트)는 전부 제거됐다. 현재는 *히스토리 누적 없이* 지금 클립보드 내용만 [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md) 이 read-only 로 보여준다.

## 내부 동작

### 복사 / 붙여넣기 — KeybindingSettings 경유

`copy`/`paste` 바인딩 목록 중 하나와 매칭되면 동작(다중 바인딩). 기본값은 OS 별로 다르다(Win `ctrl+c`/`ctrl+v`, Linux `ctrl+shift+c/v`, macOS `alt+c/v`). 바인딩 편집은 [keybindings](../keybindings/index.md). 위치 기반 매핑은 [key-mapping](../../design/policies/key-mapping.md).

- **소프트 랩 인지 복사**: 셸이 너비에 맞춰 자동 줄바꿈한 라인은 복사 시 한 줄로 합쳐지고, 진짜 hard newline 은 보존.
- **붙여넣기**: bracketed paste(DECSET 2004) 지원. 텍스트 없고 이미지가 있으면 PNG 로 저장 후 경로를 붙여넣기(AI 에이전트가 이미지 참조 가능).
- **Paste 후 Ctrl+C 보호(500ms)**: paste 직후 500ms 내 Ctrl+C 는 무시(SIGINT·복사 안 함) — Ctrl+V 옆 키 오타로 입력을 날리는 사고 방지, 무시 시 토스트.

### 텍스트 선택

마우스 드래그(Normal) / 더블클릭(Word) / 트리플클릭(Line) / vi 복사 모드의 `Ctrl+v`(Block). 선택은 화면↔스크롤백을 넘나들고 전각(CJK) 2셀 폭을 정확히 처리. vi 스타일 키보드 복사 모드(`enter_copy_mode` 액션)는 hjkl 이동·visual 선택·`/`·`?` 검색·`y` 복사를 제공.

마우스 선택은 기본적으로 **마우스 트래킹이 꺼진 화면에서만** 동작한다. 앱이 마우스 트래킹(DECSET 1000/1002/1003)을 켜면(vim `:set mouse=a`, htop, Claude Code 등) 마우스가 앱에 전면 위임되어 plain 좌클릭 드래그는 앱으로 보고된다 — 근거: [ADR-0019](../../adr/0019-mouse-button-reporting-app-delegation.md).

**트래킹 ON 에서도 `Shift`+좌클릭 드래그로 로컬 텍스트 선택이 가능하다** (xterm/iTerm2 표준 modifier 우회). Shift 여부는 press 시점에 1회만 판정해 release 까지 유지하므로, 드래그 도중 Shift 를 떼도 선택이 깨지지 않는다. `Shift`+더블/트리플클릭은 word/line 선택. 선택 후 복사 단축키로 클립보드에 복사된다 — 즉 트래킹 앱 위에서도 키보드 vi 복사 모드 외에 마우스 선택 경로가 열려 있다. plain 좌클릭은 그대로 앱에 위임되어 회귀가 없다 (우클릭 `Shift` 우회는 [ADR-0022](../../adr/0022-shift-rightclick-context-menu-bypass.md) 의 동일 패턴).

### OSC 52

**쓰기(set)**: 터미널 프로그램이 OSC 52 로 시스템 클립보드에 텍스트를 설정할 수 있다(termwiz `SetSelection` → arboard 반영). 사용자가 누른 동작이 아니라 **토스트 없음**.

**읽기(query)**: `OSC 52 ; c ; ? ST` 클립보드 읽기 질의는 설정 토글 `general.allow_clipboard_read`(기본 **off**)로 게이트된다. off 면 **무응답**(1바이트도 내보내지 않음) — 터미널 안의 임의 프로그램(원격/SSH 프로세스 포함)이 로컬 클립보드(비밀번호·토큰)를 조용히 탈취하는 것을 차단한다(xterm/iTerm 계열 정책). on 이면 시스템 클립보드를 base64 로 인코딩해 `OSC 52 ; c ; <base64> ST` 로 회신. 경로: 터미널 크레이트가 `TerminalEventKind::ClipboardQuery` 이벤트만 발화(설정·클립보드 무지) → host(`Core::drain_terminal_events`)가 게이트·읽기·인코딩 후 해당 surface 의 PTY 로 `send_bytes`. 설정 UI 는 Terminal 탭. 토스트 없음.

### 현재 클립보드 뷰어

지금 시스템 클립보드에 담긴 내용은 [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md) 이 popup 으로 보여준다. host 백엔드 없이 **plugin 프로세스가 `arboard` 로 직접 read** 한다([ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) — plugin 은 비-샌드박스 OS 프로세스라 host 가 OS 클립보드 접근을 막을 수 없으므로, 단발 read 는 host 를 경유하지 않는다). 히스토리 누적·재복사는 없다.

## 인터페이스

- **사용자**: 복사/붙여넣기/선택(위), 현재 클립보드 내용은 plugin 뷰어 팝업.
- **AI Agent / CLI**: 단발 클립보드 read/write 는 각 에이전트 프로세스의 직접 접근 영역(ADR-0009). host 는 클립보드 IPC 네임스페이스를 노출하지 않는다.

## 비-목표

- 클립보드 **히스토리** 누적·재복사 — 제거됨(위 목적 참고).
- 현재 클립보드 **뷰어 UI** — 빌트인 [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md) 이 popup 으로 제공.
- IME(한글/CJK) 입력 파이프라인 — 별도 영역.

## 관련

- [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md) · [keybindings](../keybindings/index.md) · [settings](../settings/index.md)
