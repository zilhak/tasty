# 워크스페이스 카테고리 (Workspace Category)

- **Status**: Done
- **주체**: AI Agent (CRUD·소속 변경, IPC/CLI 양면) · 로컬 사용자 (사이드바 그룹·전환·생성/이름변경/삭제, 토글 on 시)
- **ADR**: [ADR-0029](../../adr/0029-workspace-category-global-index.md)
- **코드**: `crates/tasty-model/src/workspace_category.rs` · `src/core/state.rs` (`categories` / CRUD 메서드 / `set_category_collapsed`) · `src/engine/layout_persistence/{schema,capture,restore}.rs` · `src/adapters/ipc/handler/workspace_category.rs` · `crates/tasty-cli/src/commands/workspace_category.rs` · `src/adapters/ui/sidebar/{view,full,collapsed}.rs` (사이드바 그룹) · `src/adapters/ui/popup/{rail_category,confirm_delete_category}.rs` · `src/adapters/ui/{dialog,category_actions}.rs` · `src/view/main/redraw.rs` (컨텍스트 메뉴)
- **화면**: 설정 토글 on 시 사이드바가 카테고리 섹션으로 그룹 렌더. 확장 사이드바는 chevron 헤더(접힘 토글)+소속 행, 축소 레일은 카테고리 경계 `---` 버튼+우측 앵커드 팝업. 우클릭 컨텍스트 메뉴(헤더/배경/행)와 레일 팝업으로 생성/이름변경/삭제/카테고리 이동. 드래그로 다른 카테고리 이동. 갤러리 specimen: Layouts › Sidebar & rail(그룹), Overlays › Workspace categories(다이얼로그·레일 팝업).

## 목적

워크스페이스 수가 늘면 평면 목록이 길어진다. 카테고리는 워크스페이스를 **그룹(사이드바 폴더)** 으로 묶어 정리·전환을 돕는다. 기본은 off — 켜야 사용자 카테고리를 만들 수 있고, 끄면 모든 워크스페이스가 `normal` 로 모여 현행 평면 동작과 동일해진다.

## 내부 동작 (headless-valid)

- **데이터**: 각 `Workspace` 는 소속 카테고리 id(`category`)를 갖는다. `CoreState.categories: Vec<WorkspaceCategory>` 가 카테고리 목록이며 **Vec 순서 = 사이드바 섹션 순서**.
- **`normal` 예약**: id `0`, 이름 `normal`, **`categories[0]` 위치 고정**. rename/delete 불가, 생성 시 이름으로 사용 불가(대소문자 무시). 미지정 워크스페이스의 기본 소속. `ensure_normal_category` 가 생성/복원 직후 이 불변식(0번 고정 + 발급기 floor + dangling 소속 귀속)을 보장한다.
- **이름 규칙**: trim 후 빈 이름 거부, `normal`(대소문자 무시) 예약어 거부, 기존 이름과 대소문자 무시 중복 거부.
- **삭제**: 카테고리를 지우면 그 안의 워크스페이스는 **순서를 보존하며** `normal` 로 귀속한다. 워크스페이스의 전역 인덱스는 불변이므로 사용자 active 는 영향받지 않는다(원칙 1·3).
- **reorder**: `categories` Vec 순서 변경. **from/to == 0 거부**(normal 0번 고정).
- **인덱싱**: 사용자 active 워크스페이스는 전역 인덱스 단일 진실 소스로 유지([ADR-0029](../../adr/0029-workspace-category-global-index.md)). 카테고리-로컬 전환(`switch_workspace_in_active_category`)은 active 카테고리의 로컬 인덱스를 전역 인덱스로 변환해 기존 전환 경로를 재사용한다. `Alt+숫자` 는 토글 on 이면 active 카테고리 내 로컬 전환, off 면 전역 전환(무회귀).
- **영속**: `layout.json` 에 `categories`(이름·접힘 상태) + 각 워크스페이스의 `category` 가 저장된다. 둘 다 `#[serde(default)]` — 구버전 layout.json 은 카테고리 없이 로드되어 `normal` 단일로 무손실 마이그레이션된다.
- **토글 마이그레이션**: `workspace_categories_enabled` on→off 시 normal 외 모든 카테고리를 제거하고 워크스페이스를 normal 로 귀속한다(전역 인덱스·active 불변).

## 인터페이스

- **AI Agent (IPC/CLI)** — 카테고리 CRUD·reorder·소속 변경은 양면 노출(원칙 2):
  - `workspace_category.list` / `tasty workspace-category list`
  - `workspace_category.create {name}` / `tasty workspace-category create --name X`
  - `workspace_category.rename {id,name}` / `tasty workspace-category rename --id N --name X`
  - `workspace_category.delete {id}` / `tasty workspace-category delete --id N`
  - `workspace_category.move {from_index,to_index}` / `tasty workspace-category move --from A --to B`
  - `workspace.create` / `workspace.update` 의 `category`(id 또는 이름) 파라미터 — `tasty new/set workspace --category <name|id>`
  - `workspace.list` 응답에 `category` / `category_name`
- **사용자 트리거**: 설정 → 일반 → "워크스페이스 카테고리" 토글. on 이면 사이드바가 카테고리 섹션으로 그룹 렌더되고, 헤더 클릭(접힘 토글)·우클릭 컨텍스트 메뉴(빈 배경 → 새 카테고리, 헤더 → 워크스페이스 추가(그 카테고리 소속 생성, normal 포함)/이름변경/삭제/새 카테고리 — normal 은 추가·새 카테고리만, 워크스페이스 행 → 카테고리로 이동/새 카테고리)·축소 레일 `---` 팝업(Add workspace/Collapse/Rename/Delete)·드래그 앤 드롭(다른 카테고리로 이동)으로 조작한다. 생성/이름변경은 360px 단일필드 다이얼로그(라이브 검증), 삭제는 destructive confirm 을 거친다.
- **전체 접기/펴기 단축키**: `KeybindingSettings.toggle_categories_collapsed`(기본 빈 binding — 사용자가 Settings › Keybindings 에서 지정). 하나라도 펼쳐져 있으면 전부 접고, 전부 접혀 있으면 전부 편다(normal 포함, `CoreState::toggle_all_categories_collapsed`). 카테고리 토글 off 면 매칭·consume 하지 않아 키가 다른 binding 으로 흐른다. Command Palette 파리티는 `dispatch_action_by_id("toggle_categories_collapsed")`.

## 비-목표 (Out of scope)

- **선택(active) 카테고리 변경·접힘 토글** 은 사용자 UI 상태 — IPC 에 노출하지 않는다(원칙 1·3). 카테고리 IPC 의 어떤 부수효과도 사용자 active/포커스를 바꾸지 않는다.
- 카테고리 자동 삭제 없음(빈 카테고리 허용).

## Acceptance Criteria

- [x] Given 토글 off When 구버전 layout.json 로드 Then 모든 워크스페이스가 `normal` 로 귀속(무손실).
- [x] Given `workspace_category.create {name:"normal"}` Then 예약어 거부.
- [x] Given 카테고리 A 에 워크스페이스 존재 When `workspace_category.delete A` Then 워크스페이스는 normal 로 이동, active 전역 인덱스 불변.
- [x] Given `workspace_category.move {from:0}` 또는 `{to:0}` Then 거부(normal 0번 고정).
- [x] Given 토글 on Then 사이드바가 카테고리 섹션(chevron 헤더 + 소속 행)으로 그룹 렌더, 토글 off 면 평면 렌더(회귀 없음).
- [x] Given 카테고리 헤더 클릭 When 접힘 토글 Then 접힘 상태가 layout.json 에 영속되고 확장↔레일이 공유.
- [x] Given `toggle_categories_collapsed` 바인딩 When 하나라도 펼쳐진 상태에서 누름 Then 전부 접힘, 다시 누르면 전부 펴짐(normal 포함). 카테고리 토글 off 면 no-op(키 흐름).
- [x] Given 워크스페이스를 다른 카테고리 섹션으로 드래그 Then 소속이 그 카테고리로 변경(전역 인덱스 불변).
- [x] Given 카테고리 생성/이름변경 다이얼로그 When 빈/normal/중복 입력 Then 인라인 danger 에러 + 확인 비활성.
- [x] Given 카테고리 삭제 When destructive confirm 확인 Then 카테고리 제거 + 워크스페이스 normal 귀속.
