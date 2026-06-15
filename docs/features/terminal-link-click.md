# 터미널 내 링크 hover·클릭 오픈 (Terminal Link Click)

- **Status**: Implemented
- **Surface**: 사용자 전용 (마우스 + 수식키 — CLI/IPC 비노출)
- **Related ADR**: ADR 후보 (adr-candidates.md #0002 user-vs-agent-action-separation)
- **Related design**: [`../concepts/ubiquitous-language.md`](../concepts/ubiquitous-language.md) (사용자/에이전트 분리), [`../design/policies/cwd.md`](../design/policies/cwd.md) (OSC 7 CWD)

## 목적

터미널 출력에 나타난 URL/경로를 마우스로 바로 열 수 있게 한다. 수식키 조합으로만 발동하여 일반 클릭 (포커스/selection) 과 충돌하지 않는다.

## 사용자 행동 (UX)

- 감지 대상:
  - URL: `http://`, `https://`, `ftp://`, `file://`
  - OSC 8 hyperlink
  - 스키마 없는 경로: Unix 절대 (`/foo/bar`), Windows 절대 (`C:\foo`, `C:/foo`), 접두사 있는 상대 (`./foo`, `../foo`), **접두사 없는 상대 (`src/main.rs`, `crates/x/Cargo.toml`)** — **터미널의 OSC 7 기반 CWD 를 기준으로 실제 존재할 때만** 링크로 판정 (오탐 방지).
    - 접두사 없는 상대경로는 **경로 구분자(`/`, Windows 는 `\` 포함)가 1개 이상인 토큰만** 후보로 잡는다. 단어 하나(`Makefile`)는 제외하고, 슬래시가 있어도 경로가 아닌 토큰(`and/or`, `TCP/IP`)은 CWD 기준 `exists()` 검사에서 배제된다 (2단 방어: 슬래시 prefilter → 실존 검사).
- 트리거: 설정된 수식키 (기본 `Ctrl`, 설정 `general` 의 링크 클릭 수식키에서 `Alt`/`없음` 선택 가능 — `LinkModifier`) 를 누른 채:
  - hover → 해당 링크 blue 하이라이트 + PointingHand 커서.
  - 좌클릭 → `webbrowser` crate 로 기본 브라우저/연결 프로그램 실행.
- 예외:
  - 수식키+클릭이 링크 위가 아니면 아무 동작도 하지 않는다.
  - selection 과 충돌하지 않는다 — 수식키+클릭은 selection 을 시작하지도, 기존 selection 을 변경하지도 않는다.
  - CWD 기준으로 존재하지 않는 경로 텍스트는 링크가 아니다 (로컬 surface 한정).

## 원격(mirror) surface 라우팅

attach 로 원격 tasty 데몬에 붙으면 로컬엔 **detached mirror surface** 가 뜨고(진짜 PTY 는 원격), 화면 경로는 **원격 호스트 경로**다. 로컬 핸들러(로컬 파일 전제)로는 열 수 없으므로 다음과 같이 분기한다.

- **mirror 판별**: 클릭한 surface(`hovered_link.surface_id`)의 terminal 이 detached mirror — 자식 PTY 가 없어 `process_id()` 가 `None` — 인지로 판별한다. (mirror 생성처는 `attach_client::start_gui_attach` 의 `Terminal::new_detached` 단 하나라 현재 `process_id().is_none()` ⟺ mirror.)
- **경로 감지**: mirror surface 는 로컬 `exists()` 검증을 건너뛰고(로컬에 없으니), OSC 7 원격 cwd 기준으로 결합한 원격 경로를 그대로 링크 대상으로 emit 한다. 따라서 원격 경로도 하이라이트된다.
- **클릭 동작**: mirror surface 의 경로 ctrl+클릭은 로컬 핸들러 lookup/identify(`DispatchFile`)를 타지 않고 **빈 핸들러 picker 팝업(empty-state, placeholder)** 만 띄운다 — 실제 파일 동작은 없다(1차 placeholder). 로컬 파일을 잘못 열거나 브라우저로 새지 않는다.
- **외부 URL**: `http://` 등 스킴 있는 URL 은 mirror 여부와 무관하게 기존대로 `webbrowser` 로 연다.
- **로컬 surface**: 변경 없음 — 기존대로 실제 핸들러로 열린다.

> **비-목표(후속)**: 실제 원격 파일을 여는 액션(`ssh host vim {path}`, `code --remote`, sftp fetch 등)과 그를 위한 host 컨텍스트(attach 프로필/destination) 주입. 1차는 placeholder + 빈 팝업 노출까지.

## 에이전트 행동 (CLI / IPC)

**없음 (비노출).** 사용자의 키보드/마우스 동작 그 자체이므로 release CLI/IPC 표면에 존재하지 않는다 (핵심 원칙 §1 — 사용자 입력 재현 금지).

- 에이전트가 출력 속 링크/경로 *데이터* 가 필요하면 별도의 read 경로를 쓴다: `surface.parse_since_mark` 의 `path` / `url` / `osc_link` 파서 (읽기 전용 — 열기 동작 없음).
- 비-목표: 링크 "클릭" 을 시뮬레이션하는 IPC, 링크 목록을 UI 상태로 노출하는 IPC.

## 비-목표 (Out of Scope)

- 링크 자동 열기 (출력 감지 시 자동 브라우저 실행 같은 것) — 열기는 항상 사용자 클릭.
- 존재하지 않는 경로의 추측성 링크화 — CWD 기준 실존 검사 통과분만.
- 수식키 없는 일반 클릭에서의 링크 동작 — 일반 클릭은 포커스/selection 전용.

## Acceptance Criteria

- [ ] Given 출력에 `http://` URL When 수식키+마우스 hover Then 링크 하이라이트 (blue) + PointingHand 커서.
- [ ] Given 출력에 `/foo/bar` 경로 When 터미널 CWD 기준 해당 경로 존재 Then 링크로 판정된다.
- [ ] Given 출력에 `/foo/bar` 경로 When CWD 기준 해당 경로 미존재 Then 링크가 *아니다* (hover 해도 무반응).
- [ ] Given 수식키+클릭 When 링크 위가 아님 Then no-op (selection 이 시작되지 않는다).
- [ ] Given 기존 selection 존재 When 수식키+클릭 Then 기존 selection 은 변경되지 않는다.
- [ ] Given release 빌드의 IPC/CLI 표면 When 링크 클릭에 해당하는 메서드를 찾음 Then **존재하지 않는다** (`agent-guide/api-reference.md` 에 부재 — 읽기용 `parse_since_mark` 파서만 존재).

## 관련 문서

- [`../features.md`](../features.md) "워크스페이스 & 탭 > 마우스 인터랙션" 섹션
- [`../agent-guide/output-parsers.md`](../agent-guide/output-parsers.md) — 에이전트용 링크/경로 *읽기* 경로
- `CLAUDE.md` "# 핵심 원칙 §1"
- `.claude-workspace/todo/adr-candidates.md` #0002
