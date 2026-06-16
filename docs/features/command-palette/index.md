# 명령 팔레트 (Command palette)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 봄)
- **ADR**: 없음
- **코드**: `src/state/command_palette.rs`, `src/adapters/ui/popup/command_palette.rs`
- **화면**: [screens/command-palette.md](screens/command-palette.md)

## 목적

VS Code 스타일 명령 팔레트. 모든 단축키 명령을 쿼리로 검색해 실행한다 — 단축키를 외우지 않아도 키보드만으로 모든 기능에 접근. [도구 메뉴](../tools-menu/index.md) 항목이자 전용 단축키로 연다.

## 내부 동작

### 후보 목록

`KeybindingSettings::GENERAL_BINDING_FIELDS` (단축키 설정 탭에 나타나는 모든 명령). `toggle_command_palette` 자신은 제외 (이미 팔레트 안이므로).

### 매칭

쿼리 입력 → case-insensitive. 정확 substring(단어 시작 보너스) → 부분 시퀀스(gap 페널티) 순으로 점수화.

### 탐색 / 실행

`↑/↓` 이동, `Enter` 실행, `Esc` 닫기, 클릭으로도 실행. 행 우측에 첫 번째 바인딩을 회색으로 표시.

### 실행 경로

Enter 시 `command_palette.pending_run` 에 `action_id` 적재 → MainView 가 다음 프레임에 drain 해 `dispatch_action_by_id` 호출. **단축키와 정확히 같은 action body** 를 타므로 효과 동일.

## 인터페이스

- **사용자**: `toggle_command_palette` 단축키 + 도구 메뉴 `Command palette` 항목.
- **IPC/CLI**: 없음 — 의도된 것. 팔레트는 *사용자의 키보드 런처* 다. agent 는 자기 동작을 IPC/CLI 로 직접 하므로 팔레트가 불필요.

## 비-목표

- IPC/CLI 노출.
- 단축키 명령 자체의 정의 — 팔레트는 *실행 진입점* 일 뿐, 각 명령의 동작은 그 기능.

## Acceptance Criteria

- [ ] 단축키 또는 도구 메뉴로 팔레트가 열린다.
- [ ] 쿼리 입력 시 모든 단축키 명령(자기 자신 제외)이 점수순으로 필터된다.
- [ ] `↑/↓`/Enter/Esc 와 클릭으로 탐색·실행·닫기가 된다.
- [ ] 항목 실행 결과가 해당 단축키 직접 실행과 동일하다.

> GUI 키보드 기능이라 시각은 스크린샷, 매칭 로직은 단위 검증 가능.

## 구현

- 상태: `src/state/command_palette.rs` (후보 생성, `match_score`, `pending_run`).
- popup: `src/adapters/ui/popup/command_palette.rs`.
- dispatch: `src/view/main/redraw.rs` 가 `pending_run` drain → `dispatch_action_by_id`.

## 화면

- [screens/command-palette.md](screens/command-palette.md) — 검색 입력 + 후보 리스트 레이아웃.
