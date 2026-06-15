# 작업영역 StatusBar

- **Status**: Implemented
- **Surface**: 사용자 (표시 + 클릭 어포던스)
- **Related design**: `Tasty Design System` `ui_kits/terminal/work.jsx` `StatusBar` (changelog `2026-06-15-status-bar.md`, B8-J4 확정)

## 목적

작업 컬럼(탭 스트립 + surface 영역) 하단에 상시 표시되는 24px 정보 바. 현재
focus surface 의 메타(브랜치 / surfaceId / 셸·그리드)를 한눈에 보여주고, 자주
쓰는 어포던스(커맨드 팔레트 / 테마 전환)를 클릭으로 제공한다.

## 사용자 행동 (UX)

- **표시(좌측)**: git 브랜치 점(`accent_success`)+이름 / surfaceId / `<셸> · <cols>×<rows>`.
  - 브랜치는 focus surface 의 cwd 기준 `.git/HEAD` 를 파싱(git 바이너리 비의존,
    크로스플랫폼). repo 가 아니거나 detached HEAD 면 브랜치 셀 미표시.
  - 셸·그리드는 terminal surface 한정(비-터미널 focus 시 미표시). surfaceId 는
    "Copy Terminal ID" 가 복사하는 값과 동일.
- **클릭(우측)**:
  - 팔레트 칩(`<단축키> palette`) → 커맨드 팔레트 토글. 단축키 표기는
    `KeybindingSettings.toggle_command_palette` 에서 가져온다(하드코딩 없음; 빈
    바인딩이면 칩만 표시).
  - 테마 토글(점+테마명) → `latte ↔ mocha` 전환. 점 색은 light=`yellow`,
    dark=`mauve`.
- **레이아웃**: 작업 컬럼 하단을 차지하며, 그만큼 터미널 영역(`compute_terminal_rect`
  의 `bottom_inset`)이 위로 줄어든다. 사이드바 영역에는 걸치지 않는다.

### focus 의존성 (원칙 3)

표시 *대상* 은 현재 focus surface 를 read 해서 결정한다. 이는 "활성 상태 정보를
조회로 제공"하는 허용된 read 용도이며(동작이 아니라 표시), focus 독립성 원칙에
위배되지 않는다. 팔레트 오픈/테마 토글은 사용자 마우스 클릭(사용자 행동 표면)이다.

## 에이전트 행동 (CLI / IPC)

- 없음. StatusBar 는 사용자 표시/어포던스 전용이며 CLI/IPC 표면에 노출되지 않는다.

## 비-목표 (Out of Scope)

- 에이전트가 StatusBar 셀을 조작/조회하는 API.
- git 브랜치 외 git 상태(ahead/behind, dirty 등) 표시.

## Acceptance Criteria

- [x] Given git repo 안의 터미널이 focus When 작업 컬럼이 렌더 Then 하단 24px 바에
  브랜치 점+이름 / surfaceId / 셸·그리드 / 우측 팔레트 칩·테마 토글이 표시되고
  상단 1px separator 가 그려진다.
- [x] Given focus surface 전환 When surface 가 바뀜 Then surfaceId·셸·그리드가 갱신된다.
- [x] Given 팔레트 칩 클릭 Then 커맨드 팔레트가 토글된다.
- [x] Given 테마 토글 클릭 Then `latte ↔ mocha` 가 전환된다.
- [x] 모든 고정 문자열(`palette`, tooltip)은 `t()` 키 경유(en/ko/ja).

## 관련 문서

- 디자인: `ui_kits/terminal/work.jsx` `StatusBar`
- 구현: `src/adapters/ui/status_bar.rs`, `crates/tasty-model/src/lib.rs`
  (`compute_terminal_rect` `bottom_inset`), `crates/tasty-type-appearance`
  (`status_bar_height` 토큰)
- 인접 기능: [workspace-tabs.md](workspace-tabs.md), [command-palette.md](command-palette.md)
