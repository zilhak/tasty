# 레이아웃 영속화 (Layout persistence)

- **Status**: Implemented
- **주체**: 로컬 사용자 (설정 토글)
- **ADR**: 없음
- **코드**: `src/engine/layout_persistence/`, `~/.tasty/layout.json` · `~/.tasty/scrollback/<id>.bin`
- **화면**: 없음 (앱 시작 시 자동 복원)

## 목적

`general.restore_layout`(기본 **off**) 활성 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json` 에 저장하고, 앱 시작 시 복원해 이전 세션 창 배치를 재현한다.

## 내부 동작

### 저장 대상 / 타이밍

워크스페이스(이름·부제·설명) · 페인 트리(split direction/ratio) · 탭(이름·active) · 서피스 레이아웃 트리 · 서피스 타입별 최소 정보(Terminal: cwd·`restore.command`·`scrollback_ref` / Markdown·Image: path / Explorer: root / Html: url) · 활성 워크스페이스·포커스 페인 인덱스. 구조 변경 시 dirty + **500ms 디바운스**, 종료 시 dirty 면 즉시 flush.

복원은 앱 시작 1회. 파싱 실패/파일 없음 → 기본 "Workspace 1" 폴백. 개별 서피스 복원 실패 시 그 서피스만 스킵.

### Surface 내용 복원 (현재: 터미널 scrollback)

`general.restore_surface_content`(기본 **on**) 시 각 터미널의 scrollback + 현재 화면 라인을 `~/.tasty/scrollback/<persist_id>.bin`(magic `TSSB`)에 보존 → 재시작 후 위로 스크롤하면 [이전 scrollback → 이전 화면 → 새 prompt] 순. `persist_id` 는 surface-meta(`scrollback.persist_id`)에 보관, 같은 surface 면 atomic 덮어쓰기(orphan 없음). 옵션 OFF→ON 전환 시 capture/restore 스킵, ON→OFF 시 `~/.tasty/scrollback/` 전체 삭제. Lifecycle: surface 닫힘 시 `.bin` 삭제, 앱 시작 시 `layout.json` 의 `scrollback_ref` 집합 외 `.bin` 일괄 정리(크래시 잔재).

### TUI 세션 복원 (`restore.command`)

claude plugin 등이 `tasty claude install` 로 SessionStart/End hook 을 걸면 세션 시작 시 `restore.command`(예: `claude -r <session-id>`)를 surface-meta 에 set. 호스트는 **agent-agnostic** 하게 `restore.command` 값만 읽어 복원에 쓴다. 명령 주입 타이밍은 PTY spawn 그 순간 — `TerminalConfig.initial_input` 으로 writer thread 시작 전 master fd 에 동기 write, child shell 의 첫 stdin read 에 무조건 첫 입력으로 들어감(추가 트리거 없이 spawn 과 동시 실행). 발동 경로 둘: 앱 재시작(레이아웃 복원) · [닫힌 항목 복원](../closed-tab-restore/index.md)(Ctrl+Shift+T).

## 저장하지 않는 것

현재 화면 cells(새 prompt 로 채움) · PTY 상태/환경변수/실행 중 명령 · 팝업 상태.

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-presets](../layout-presets/index.md) · [terminal](../terminal/index.md)(scrollback)
