# 레이아웃 영속화 (Layout persistence)

- **Status**: Implemented
- **주체**: 로컬 사용자 (설정 토글)
- **ADR**: 없음
- **코드**: `src/engine/layout_persistence/`, `~/.tasty/layouts/NN.json` · `~/.tasty/scrollback/<id>.bin`
- **화면**: 없음 (앱 시작 시 자동 복원)

## 목적

`general.restore_layout`(기본 **off**) 활성 시 워크스페이스/페인/탭/서피스 구조를 **슬롯 파일** `~/.tasty/layouts/NN.json` 에 저장하고, 앱 시작 시 복원해 이전 세션 창 배치를 재현한다.

## 내부 동작

### 저장 대상 / 타이밍

워크스페이스(이름·부제·설명) · 페인 트리(split direction/ratio) · 탭(이름·active) · 서피스 레이아웃 트리 · 서피스 타입별 최소 정보(Terminal: cwd·`restore.command`·`scrollback_ref` / Markdown·Image: path / Explorer: root / Html: url) · 활성 워크스페이스·포커스 페인 인덱스. 구조 변경 시 dirty + **500ms 디바운스**, 종료 시 dirty 면 즉시 flush.

복원은 engine(=창) 하나가 만들어질 때 1회 — 앱 시작 시의 첫 창과 이후 새로 여는 창 모두. 파싱 실패/파일 없음 → 기본 "Workspace 1" 폴백. 개별 서피스 복원 실패 시 그 서피스만 스킵.

### 슬롯 파일

레이아웃은 `~/.tasty/layouts/NN.json` 슬롯 파일 하나 = engine 하나의 전체 상태다(워크스페이스 목록 · 활성 워크스페이스 · 카테고리). 슬롯 목록과 순서는 파일명의 숫자에서 전부 파생되며 별도 인덱스 파일을 두지 않는다 — 인덱스는 실제 파일과 desync 되는 두 번째 진실원이 된다. 번호는 2자리 zero-pad(`01.json`)이고 100 이상은 자연 확장된다.

write 는 `NN.json.tmp` 에 쓴 뒤 rename 하는 **원자적** 교체다. 슬롯이 여러 개이므로 잘린 JSON 하나가 아래 scrollback 정리를 통해 다른 슬롯의 `.bin` 까지 잃게 만들 수 있다.

단일 파일 시절의 `~/.tasty/layout.json` 은 부팅 1회 `layouts/01.json` 으로 **이동**(rename)된다. `layouts/` 가 이미 있으면 이동하지 않고 남은 레거시 파일을 로그로 알린다.

### 슬롯 점유와 배정

창 ↔ engine ↔ 슬롯은 1:1 이다. 창을 새로 열면 이미 다른 창이 쓰고 있는 슬롯이 아니라 **다음 free 슬롯**을 잡으므로, 두 창이 같은 레이아웃을 복제해 보여주지 않는다.

배정 규칙 — **실제 존재하는 슬롯 파일** 중 점유되지 않은 가장 낮은 번호를 쓴다.

- 번호 공백은 채우지 않는다: 파일이 `02.json`·`03.json` 뿐이고 점유가 없으면 답은 `1` 이 아니라 **2** 다. 저장된 레이아웃을 건너뛰고 빈 창을 띄우지 않기 위한 것이다.
- 기존 슬롯 파일이 전부 점유면 `max(파일 슬롯 ∪ 점유 슬롯) + 1` 로 **새 슬롯**을 만든다. 파일이 없으므로 그 창은 기본 워크스페이스 하나로 시작한다.
- 파일도 점유도 없으면(첫 설치) `1`.

점유는 **휘발성**이다 — 디스크에 기록하지 않고 별도 레지스트리도 두지 않는다. 살아있는 engine 들이 들고 있는 슬롯 번호를 모은 것이 곧 점유 집합이므로, 재시작하면 전부 free 로 돌아온다(크래시가 슬롯을 영구 점유로 남기지 않는다). 창이 닫혔다가 되살아나는 parked engine 은 자기 슬롯을 그대로 이어쓴다 — 재배정하면 남의 슬롯 파일을 덮어쓴다.

저장된 슬롯이 여러 개여도 **부팅 시 창은 1개**다. 나머지 슬롯은 free 로 남아 있다가 창을 더 열면 순서대로 복원된다.

저장도 각 engine 이 자기 슬롯 파일에만 한다 — 종료 시의 일괄 flush 도 창마다 자기 파일로 나뉜다. headless(`--headless`)는 레이아웃 복원을 적용하지 않으므로 슬롯을 점유하지도, 저장하지도 않는다.

#### 점유 조회

어느 창이 어느 슬롯을 쓰는지는 `window.list` IPC(= `tasty list windows`)의 `layout_slot` 필드로 본다. 슬롯 점유는 "새 창을 열면 어떤 레이아웃이 뜰지" 를 결정하는 상태라, 관측 수단이 없으면 에이전트가 창 생성 결과를 예측·검증할 수 없다.

```json
[
  { "id": 1, "focused": true,  "title": "Tasty",       "layout_slot": 1 },
  { "id": 2, "focused": false, "title": "Workspace 1", "layout_slot": 2 }
]
```

`layout_slot` 은 슬롯을 잡지 않는 engine 에서 `null` 이다(headless). 순수 read 라 포커스·선택 등 사용자 상태를 건드리지 않는다.

**parked engine 은 이 목록에 없다.** 파킹된 engine 도 슬롯을 점유하지만 창이 아니어서 창 id 가 없고, `window.list` 의 `{id, focused, title}` 계약이 깨진다. 따라서 이 목록의 `layout_slot` 집합은 살아있는 창의 점유일 뿐 **점유 집합 전체가 아니다** — 파킹분까지 봐야 할 일이 생기면 별도 조회 메서드로 분리한다.

### Surface 내용 복원 (현재: 터미널 scrollback)

`general.restore_surface_content`(기본 **on**) 시 각 터미널의 scrollback + 현재 화면 라인을 `~/.tasty/scrollback/<persist_id>.bin`(magic `TSSB`)에 보존 → 재시작 후 위로 스크롤하면 [이전 scrollback → 이전 화면 → 새 prompt] 순. `persist_id` 는 surface-meta(`scrollback.persist_id`)에 보관, 같은 surface 면 atomic 덮어쓰기(orphan 없음). 옵션 OFF→ON 전환 시 capture/restore 스킵, ON→OFF 시 `~/.tasty/scrollback/` 전체 삭제. Lifecycle: surface 닫힘 시 `.bin` 삭제, 앱 시작 시 **전 슬롯의** `scrollback_ref` 합집합 외 `.bin` 일괄 정리(크래시 잔재) — 슬롯 하나만 보고 정리하면 다른 슬롯이 참조하는 `.bin` 을 지운다. 읽을 수 없는 슬롯이 하나라도 있으면 그 부팅에서는 정리 자체를 건너뛴다(모르면 지우지 않는다).

### TUI 세션 복원 (`restore.command`)

claude plugin 등이 `tasty claude install` 로 SessionStart/End hook 을 걸면 세션 시작 시 `restore.command`(예: `claude -r <session-id>`)를 surface-meta 에 set. 호스트는 **agent-agnostic** 하게 `restore.command` 값만 읽어 복원에 쓴다 — 그 문자열이 무엇을 싣는지는 전적으로 plugin 소관이다. 예컨대 claude plugin 은 세션 프로필이 부착돼 있으면 `claude -r <id> --settings "<프로필 경로>"` 형태로 써서 **복원된 프로세스에도 프로필이 그대로 붙게** 한다([claude plugin](../../plugins/claude/index.md) "복원을 건너 프로필이 유지되는 방식") — 복원이 발급하는 새 surface id 때문에 surface meta 는 복원을 넘지 못하므로, 프로필을 실어 나르는 유일한 통로가 이 문자열이다. 명령 주입 타이밍은 PTY spawn 그 순간 — `TerminalConfig.initial_input` 으로 writer thread 시작 전 master fd 에 동기 write, child shell 의 첫 stdin read 에 무조건 첫 입력으로 들어감(추가 트리거 없이 spawn 과 동시 실행). 발동 경로 둘: 앱 재시작(레이아웃 복원) · [닫힌 항목 복원](../closed-tab-restore/index.md)(Ctrl+Shift+T).

## 저장하지 않는 것

현재 화면 cells(새 prompt 로 채움) · PTY 상태/환경변수/실행 중 명령 · 팝업 상태.

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-presets](../layout-presets/index.md) · [terminal](../terminal/index.md)(scrollback)
