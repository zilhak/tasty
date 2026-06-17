# 레이아웃 프리셋 (Layout presets)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`preset.*`)
- **ADR**: 없음
- **코드**: `tasty-presets` 크레이트, `~/.tasty/presets/{workspace,tab,pane}/<name>.toml`, `preset.*` 핸들러
- **화면**: PresetView (EditorView 계열, modeless)

## 목적

Workspace/Tab/Pane 레이아웃과 각 leaf surface 의 초기화 파라미터(kind·cwd·시작 명령어·kind 별 params)를 미리 저장해 재사용한다. [닫힌 항목 복원](../closed-tab-restore/index.md)(인메모리 LIFO)과 달리 **디스크 영구 저장**, 반복 사용 목적.

## 내부 동작

### 세 종류

WorkspacePreset(전체: 상위 레이아웃 + 모든 pane/tab/surface) · TabPreset(단일 탭) · PanePreset(단일 페인: 탭 목록 + 활성 탭). 셋 다 `LayoutPreset` trait 구현(`tasty-presets`).

### 저장 / 편집

저장: 사이드바 워크스페이스 카드 우클릭 · 탭 타이틀/탭바 빈 공간 우클릭 · 도구 메뉴 "프리셋". 위치 `~/.tasty/presets/{kind}/<name>.toml`(파일명 = 정본, 같은 kind 내 중복 불가 — 충돌 시 `-N` suffix). 편집은 **PresetView**(EditorView 계열, modeless, 종류별 1 인스턴스 — [hierarchy](../../concepts/hierarchy.md))에서 이름·subtitle·레이아웃 트리·각 leaf 의 (kind, cwd, 시작 명령어, params). 시작 명령어 폼은 kind=`terminal` 일 때만.

### 적용 — 포커스 규칙

- 단축키(`apply_workspace_preset`/`apply_tab_preset`/`apply_pane_preset`, 기본 빈 칸): 적용 popup → 선택 → 새 인스턴스 생성 + **포커스 이동**.
- **CLI/IPC `preset.apply` 는 항상 `focus: false`** — 포커스 독립 원칙. 단축키 호출만 포커스 이동.

terminal 시작 명령어는 PTY ready 직후 stdin 에 한 줄 자동 입력.

## 인터페이스

`preset.{list,get,save,delete,rename,capture,apply}`(`SurfaceRead`/`SurfaceWrite`) — `tasty preset {list,get,save,delete,rename,capture,apply}`. 표 → [reference/api](../../reference/api.md#구조--workspace--pane--tab--surface--split--tree).

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-persistence](../layout-persistence/index.md) · [work-area](../work-area/index.md)
