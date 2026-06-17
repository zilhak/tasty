# 클립보드 (Clipboard)

- **Status**: Implemented
- **주체**: 로컬 사용자 (복사/붙여넣기는 사용자 행동)
- **ADR**: 없음
- **코드**: 시스템 클립보드 `arboard`; 복사/붙여넣기/선택 = `src/view/main/`; 히스토리 = `CoreState.clipboard_history`
- **화면**: 히스토리 뷰어는 [clipboard-history plugin](../../plugins/clipboard-history/index.md)

## 목적

터미널 텍스트의 **복사/붙여넣기/선택**, OSC 52 클립보드 설정, 그리고 **시스템 클립보드 히스토리** 기록. 복사/붙여넣기는 사용자 행동이라 토스트로 피드백하지만, 에이전트(IPC)·OSC 52 경로는 사용자 시각 상태를 건드리지 않는다([toast](../../design/systems/toast.md) 트리거 정책).

## 내부 동작

### 복사 / 붙여넣기 — KeybindingSettings 경유

`copy`/`paste` 바인딩 목록 중 하나와 매칭되면 동작(다중 바인딩). 기본값은 OS 별로 다르다(Win `ctrl+c`/`ctrl+v`, Linux `ctrl+shift+c/v`, macOS `alt+c/v`). 바인딩 편집은 [keybindings](../keybindings/index.md). 위치 기반 매핑은 [key-mapping](../../design/policies/key-mapping.md).

- **소프트 랩 인지 복사**: 셸이 너비에 맞춰 자동 줄바꿈한 라인은 복사 시 한 줄로 합쳐지고, 진짜 hard newline 은 보존.
- **붙여넣기**: bracketed paste(DECSET 2004) 지원. 텍스트 없고 이미지가 있으면 PNG 로 저장 후 경로를 붙여넣기(AI 에이전트가 이미지 참조 가능).
- **Paste 후 Ctrl+C 보호(500ms)**: paste 직후 500ms 내 Ctrl+C 는 무시(SIGINT·복사 안 함) — Ctrl+V 옆 키 오타로 입력을 날리는 사고 방지, 무시 시 토스트.

### 텍스트 선택

마우스 드래그(Normal) / 더블클릭(Word) / 트리플클릭(Line) / vi 복사 모드의 `Ctrl+v`(Block). 선택은 화면↔스크롤백을 넘나들고 전각(CJK) 2셀 폭을 정확히 처리. vi 스타일 키보드 복사 모드(`enter_copy_mode` 액션)는 hjkl 이동·visual 선택·`/`·`?` 검색·`y` 복사를 제공.

### OSC 52

터미널 프로그램이 OSC 52 로 시스템 클립보드에 텍스트를 설정할 수 있다(termwiz `SetSelection` → arboard 반영). 사용자가 누른 동작이 아니라 **토스트 없음**.

### 시스템 클립보드 히스토리

`CoreState.clipboard_history`(메모리 전용, host 소유)에 시스템 클립보드 변경을 기록. 별도 스레드가 `clipboard.poll_interval_ms`(기본 500ms)로 폴링 → arboard 로 현재 값을 읽어 기록. 연속 중복·빈 문자열 제거, 출처 태그(System/Internal). 설정: `history_enabled`(기본 on), `history_max`(기본 100), `poll_interval_ms`(재시작 필요). 재시작 시 휘발.

> 비밀번호 관리자 등 민감 정보도 기록될 수 있다 — OS 민감 플래그 구분 수단이 제한적이라 1차는 필터 없음.

## 인터페이스

- **사용자**: 복사/붙여넣기/선택(위), 히스토리 뷰어는 plugin 팝업.
- **AI Agent / CLI**: 히스토리 읽기·붙여넣기는 `tool.clipboard.{list,get,paste}` IPC + `tasty clipboard` CLI. (clear/remove 는 plugin 뷰어 경로.)

## 비-목표

- 히스토리 **뷰어 UI** — 빌트인 [clipboard-history plugin](../../plugins/clipboard-history/index.md) 이 popup 으로 제공.
- IME(한글/CJK) 입력 파이프라인 — 별도 영역.

## 관련

- [clipboard-history plugin](../../plugins/clipboard-history/index.md) · [keybindings](../keybindings/index.md) · [settings](../settings/index.md)
