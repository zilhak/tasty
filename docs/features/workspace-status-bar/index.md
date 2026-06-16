# 상태바 (Workspace status bar)

- **Status**: Implemented
- **주체**: 로컬 사용자 (GUI 전용)
- **ADR**: 없음
- **코드**: `src/adapters/ui/status_bar.rs`
- **화면**: [screens/workspace-status-bar.md](screens/workspace-status-bar.md)

## 목적

[작업 영역](../work-area/index.md) 하단의 24px 바. 현재 focus surface 의 컨텍스트(브랜치/surfaceId/셸·그리드)와 우측 빠른 액션(명령 팔레트·테마 토글)을 보여준다. 항상 그려지며 작업 컬럼 아래 strip 을 차지한다(타이틀바 `top_inset` 과 대칭인 `bottom_inset`).

## 내부 동작 (headless-valid)

순수 view(`draw_status_bar_view`)가 `StatusBarData` + `Theme` 만 받아 바를 그리고 클릭을 `StatusBarAction` 으로 보고 → wrapper(`draw_status_bar`)가 state/engine 에서 데이터를 추출하고 액션을 적용한다.

### 표시 데이터 (focus surface read)

좌측 클러스터는 **현재 focus surface 를 read** 해 표시한다 — 표시 *대상 결정* 이 focus 에 의존하지만 *동작이 아니라 표시* 라 [포커스 독립성](../../identity.md) 에 위배되지 않는다(허용된 조회 read).

- **브랜치 점 + 이름** — focus surface 의 cwd 기준 git 브랜치(`.git/HEAD` 를 std::fs 로 상위 탐색, git 바이너리 비의존·크로스플랫폼). repo 아니거나 detached 면 미표시.
- **surfaceId** — focus surface 의 숫자 ID ("Copy Terminal ID" 와 동일 값).
- **셸 · cols×rows** — terminal 한정(포그라운드 프로세스명 + 그리드 크기).

### 우측 액션 (clickable)

- **팔레트 칩** (`<단축키> palette`) → 명령 팔레트 토글. 단축키 문자열은 `KeybindingSettings`(`toggle_command_palette`) 연동.
- **테마 토글** (점 + 테마명) → latte ↔ mocha 전환(그 외 테마에서 누르면 latte). 점 색은 light=yellow / dark=mauve.

## 인터페이스

- **사용자 트리거**: 팔레트 칩 클릭(→ [command-palette](../command-palette/index.md)), 테마 토글 클릭(→ 테마 설정). 좌측 클러스터는 표시 전용(비클릭).
- **AI Agent**: 없음 — 표시 위젯. 표시되는 값(브랜치/그리드 등)은 각 도메인 조회(`tasty list surfaces` 등)로 별도 접근.

## 비-목표 (Out of scope)

- **명령 팔레트 내용** — [command-palette](../command-palette/index.md).
- **테마 정의/적용 규칙** — 테마 시스템(설정).
- **표시 값의 도메인 동작**(터미널 그리드·cwd 추적 등) — 각 기능.

## Acceptance Criteria

- [ ] Given focus surface 가 git repo 안 터미널 Then 브랜치 점+이름 / surfaceId / 셸·cols×rows 가 표시된다.
- [ ] Given repo 아님 Then 브랜치 클러스터가 숨는다.
- [ ] Given 팔레트 칩 클릭 Then 명령 팔레트가 토글된다.
- [ ] Given 테마 토글 클릭 Then latte ↔ mocha 가 전환되고 점 색이 바뀐다.
- [ ] Given 팔레트 단축키 설정 변경 Then 칩의 단축키 표시가 따라간다.

> GUI 위젯이라 시각은 스크린샷. 표시 값은 `tasty list surfaces` 등 도메인 조회와 대조, 액션 결과(팔레트/테마)는 해당 기능으로 확인.

## 구현

- view: `src/adapters/ui/status_bar.rs` `draw_status_bar_view`(`StatusBarData`→`Vec<StatusBarAction>`), `status_bar_bottom_inset`(= `status_bar_height` 토큰).
- wrapper: `draw_status_bar`(focus surface read → 데이터 추출, 액션 적용: 팔레트 intent / 테마 settings).
- 브랜치: `git_branch`(`.git/HEAD` std::fs 파싱).

## 화면

- [screens/workspace-status-bar.md](screens/workspace-status-bar.md) — 바 레이아웃(좌측 컨텍스트 / 우측 액션).
</content>
