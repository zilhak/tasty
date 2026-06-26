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

저장: 사이드바 워크스페이스 카드 우클릭 · 탭 타이틀/탭바 빈 공간 우클릭 · 도구 메뉴 "프리셋". 위치 `~/.tasty/presets/{kind}/<name>.toml`(파일명 = 정본, 같은 kind 내 중복 불가 — 충돌 시 `-N` suffix).

**PresetView**(EditorView 계열, modeless, 종류별 1 인스턴스 — [hierarchy](../../concepts/hierarchy.md))는 L1 scope 탭(Workspace/Tab/Pane) 아래 2-depth list→detail 본문이다:

- **좌측 리스트**(196px): 현재 scope 의 저장된 preset 목록. row = 이름 + mono subtitle(workspace 는 저장된 subtitle, 없으면 pane/tab 개수 / tab·pane 은 surface·tab 개수). 선택 row 는 `surface-active` 채움 + 2px accent 좌측 bar. 헤더에 `N presets` + New preset(`+`) 버튼(현재 레이아웃 capture 가 아니라 terminal 1개짜리 최소 preset 생성 — 본문 capture 경로는 컨텍스트 메뉴 저장이 담당). 빈 scope → "저장된 프리셋이 없습니다.".
- **우측 detail**: 44px 툴바(좌: preset 이름+subtitle / 우: rename·duplicate·delete 아이콘 + Edit 버튼) 위에 선택 preset 의 **데모 레이아웃 미리보기**(상위 pane split = 카드+gap, 하위 surface split = hairline, leaf = kind 라벨, mini-tab 클릭 전환 — 구조만, 내용 렌더 없음). 툴바 rename·duplicate·delete 는 store 에 직결돼 즉시 동작(rename 은 인라인 입력).

#### WYSIWYG 편집 모드 (Edit 버튼)

툴바 **Edit** 버튼을 누르면 같은 미리보기 영역이 그 자리에서 편집 가능한 WYSIWYG 모드로 전환된다(별도 화면·모달 없음). Edit 는 primary **Done** 으로 바뀌고, 옆에 "자동 저장됨" 안내가 표시된다 — **별도 Save 버튼 없음**. 모든 변경은 `PresetStore::save_*_overwrite` 로 즉시 디스크에 write-through 된다(기존 preset 의 메타데이터는 보존하고 레이아웃 트리만 교체).

- **surface 선택**: 편집 모드에서는 모든 surface 가 1px hairline 윤곽을 얻고, 클릭으로 한 surface 를 선택하면 2px accent inset 윤곽 + 핸들 클러스터(우측 split `dir=row` · 하단 split `dir=col` · 제거 danger)가 붙는다. 마지막 한 장 남은 surface 제거는 무효(트리에 0-surface 탭을 쓰지 않음).
- **leaf 인라인 폼**: 선택한 leaf 의 중앙 라벨이 인라인 폼으로 바뀐다 — kind 드롭다운(Select) + 작업 디렉터리 Input(mono) + 시작 명령어 Input(mono, **kind=`terminal` 일 때만** 노출). kind 를 바꾸면 라벨이 즉시 갱신되고 시작 명령어 필드가 토글된다.
- **이름/subtitle 인라인 편집**: 편집 모드에서 툴바의 preset 이름은 텍스트 입력으로, subtitle 은 (Workspace 한정 실제 필드일 때) 입력으로 바뀌어 포커스 해제 시 store 에 commit 된다.
- **트리 변형**: split(우측/하단) · 제거 · 탭 추가(+) 가 실제 트리를 변형하고 자동 저장된다.

mini-tab strip 은 `tab_bar.rs`, split 라인은 `divider.rs` 위젯을 재사용한다.

### 적용 — 포커스 규칙

- 단축키(`apply_workspace_preset`/`apply_tab_preset`/`apply_pane_preset`, 기본 빈 칸): 적용 popup → 선택 → 새 인스턴스 생성 + **포커스 이동**.
- **CLI/IPC `preset.apply` 는 항상 `focus: false`** — 포커스 독립 원칙. 단축키 호출만 포커스 이동.

terminal 시작 명령어는 PTY ready 직후 stdin 에 한 줄 자동 입력.

## 인터페이스

`preset.{list,get,save,delete,rename,capture,apply}`(`SurfaceRead`/`SurfaceWrite`) — `tasty preset {list,get,save,delete,rename,capture,apply}`. 표 → [reference/api](../../reference/api.md#구조--workspace--pane--tab--surface--split--tree).

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-persistence](../layout-persistence/index.md) · [work-area](../work-area/index.md)
