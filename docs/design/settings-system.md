# 설정 시스템

Tasty의 설정 시스템 구조. 설정 파일, GUI 설정 윈도우, 카테고리 구성을 정의한다.

## 설정 파일

- 경로: `~/.tasty/config.toml` (전 플랫폼 통일)
- 형식: TOML + serde 직렬화/역직렬화
- 파일이 없거나 파싱 실패 시 기본값으로 폴백
- 드래프트 패턴: 편집 중 원본 보존, Save로 디스크 저장 + 즉시 적용, Cancel로 폐기

## GUI 설정 윈도우

독립 OS 윈도우(ModalView). Ctrl+, 단축키로 토글. 모달 활성 시 다른 윈도우 입력 차단.

### 탭 구성

| 탭 | 설명 |
|---|---|
| General | 셸 경로, 시작 명령, 스크롤백, CWD 상속, 셸 모드, 레이아웃 복원 |
| Appearance | 폰트, 테마, 배경 투명도, 사이드바 너비, surface별 색상 |
| Clipboard | 복사/붙여넣기 활성화, 히스토리 설정 |
| Notifications | 알림 활성화, 시스템 알림, 사운드, 병합 간격 |
| Keybindings | 단축키 설정 (서브탭으로 분류) |
| Language | UI 언어 선택 (en/ko/ja) |
| Performance | PTY 폴링, 디스크 스왑, 지연 PTY 초기화 |
| File Handler | Detectors / Handlers / Extension Mapping / Recent picks (`~/.tasty/file-handlers.toml` 사용자 영역 편집) |
| Misc | tastyrc 편집기 등 기타 |

## Keybindings 탭 — 서브탭 구조

단축키 설정은 서브탭으로 분류된다. 서브탭 순서는 유비쿼���스 언어 계층 구조를 따른다.

### 서브탭 순서 규칙

```
General → Workspace → Pane → Tab → Surface → Clipboard → Zoom → Preset
         \___________ 계층 순서 ____________/
```

- **General, Clipboard, Zoom**: 계층에 속하지 않는 전역/기능별 단축키
- **Workspace → Pane → Tab → Surface**: 유비쿼터스 언어 계층 구조 순서
- **Preset**: 프리셋 적용 (항상 마지막)

### 서브탭 내부 항목 순서 규칙

각 서브탭 내부의 단축키 항목은 다음 순서로 정렬한다:

1. **생성/분할** (new, split, open)
2. **탐색** (next, prev, focus)
3. **수정** (rename, convert)
4. **닫기** (close)
5. **수식키 설정** (modifier — separator로 구분)

### 서브탭별 항목 배치

| 서브탭 | 항목 |
|--------|------|
| General | toggle_settings, toggle_notifications, toggle_clipboard_viewer, restore_closed, new_window, quit, quit_immediate, quit_minimize |
| Workspace | new_workspace, rename_workspace, rename_workspace_subtitle, close_workspace + workspace_switch_modifier |
| Pane | split_pane_vertical, split_pane_horizontal, focus_pane_next, focus_pane_prev, close_pane |
| Tab | new_tab, open_markdown, open_explorer, next_tab, prev_tab, rename_tab, close_active + tab_switch_modifier |
| Surface | split_surface_vertical, split_surface_horizontal, focus_surface_next, focus_surface_prev, convert_surface, convert_to_markdown, convert_to_explorer, close_surface |
| Clipboard | copy, copy_path, cut, select_all, paste |
| Zoom | zoom_in, zoom_out, zoom_reset |
| Preset | 프리셋 목록 + 미리보기 테이블 + 적용 버튼 |

### 항목 배치 판단 기준

어떤 단축키가 어떤 서브탭에 속하는지 판단하는 기준:

- **해당 동작의 대상이 무엇인가?** 대상 엔티티의 이름을 가진 서브탭에 배치한다.
  - `new_tab` → 탭을 생성 → **Tab**
  - `split_pane_vertical` → 페인을 분할 → **Pane**
  - `close_surface` → 서피스를 닫음 → **Surface**
- **수식키 설정(modifier)**은 해당 엔티티의 서브탭에 배치한다.
  - `tab_switch_modifier` → 탭 전환 수식키 → **Tab**
  - `workspace_switch_modifier` → 워크스페이스 전환 수식키 → **Workspace**
- **close_active**는 cascade 동작(탭→페인→워크스페이스)이지만, 가장 먼저 닫히는 대상이 탭이므로 **Tab**에 배치한다.
- **open_markdown / open_explorer**는 새 탭으로 열리는 동작이므로 **Tab**에 배치한다.
