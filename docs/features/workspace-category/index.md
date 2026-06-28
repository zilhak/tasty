# 워크스페이스 카테고리 (Workspace Category)

- **Status**: Partial
- **주체**: AI Agent (CRUD·소속 변경, IPC/CLI 양면) · 로컬 사용자 (사이드바 그룹·전환, 토글 on 시)
- **ADR**: [ADR-0029](../../adr/0029-workspace-category-global-index.md)
- **코드**: `crates/tasty-model/src/workspace_category.rs` · `src/core/state.rs` (`categories` / CRUD 메서드) · `src/engine/layout_persistence/{schema,capture,restore}.rs` · `src/adapters/ipc/handler/workspace_category.rs` · `crates/tasty-cli/src/commands/workspace_category.rs`
- **화면**: 없음 — 사이드바 렌더링(섹션 그룹·카테고리 헤더)은 미구현(현재 사이드바는 평면 렌더 유지).

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
- **사용자 트리거**: 설정 → 일반 → "워크스페이스 카테고리" 토글. (사이드바 그룹 UI·헤더 조작은 미구현.)

## 비-목표 (Out of scope)

- **선택(active) 카테고리 변경·접힘 토글** 은 사용자 UI 상태 — IPC 에 노출하지 않는다(원칙 1·3). 카테고리 IPC 의 어떤 부수효과도 사용자 active/포커스를 바꾸지 않는다.
- 카테고리 자동 삭제 없음(빈 카테고리 허용).

## Acceptance Criteria

- [x] Given 토글 off When 구버전 layout.json 로드 Then 모든 워크스페이스가 `normal` 로 귀속(무손실).
- [x] Given `workspace_category.create {name:"normal"}` Then 예약어 거부.
- [x] Given 카테고리 A 에 워크스페이스 존재 When `workspace_category.delete A` Then 워크스페이스는 normal 로 이동, active 전역 인덱스 불변.
- [x] Given `workspace_category.move {from:0}` 또는 `{to:0}` Then 거부(normal 0번 고정).
- [ ] (미구현) 사이드바가 카테고리 섹션으로 그룹 렌더.
