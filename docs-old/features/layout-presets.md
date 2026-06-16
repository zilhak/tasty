# 레이아웃 프리셋 (Layout Presets)

- **Status**: Implemented

Workspace / Tab / Pane 레이아웃과 각 leaf surface 의 초기화 파라미터(kind, cwd, 시작 명령어, kind 별 params)를 미리 저장해두고 재사용할 수 있다. `ClosedItem`(닫힌 항목 복원, 인메모리 LIFO)과 달리 디스크에 영구 저장되며 반복 사용을 의도한다.

### 종류
- **Workspace Preset**: 워크스페이스 전체 (상위 레이아웃 + 모든 pane/tab/surface)
- **Tab Preset**: 단일 탭 (이름 + 하위 레이아웃 + surface 들)
- **Pane Preset**: 단일 페인 (탭 목록 + 활성 탭 + 각 탭의 하위 레이아웃)

세 종류 모두 `LayoutPreset` trait 를 구현하며 `tasty-presets` 크레이트에 정의된다.

### 저장
- 사이드바 워크스페이스 카드 우클릭 → "워크스페이스 프리셋으로 저장"
- 탭 타이틀 우클릭 → "탭 프리셋으로 저장" 또는 "페인 프리셋으로 저장"
- 탭바 빈 공간 우클릭 → "페인 프리셋으로 저장"
- 좌측 하단 도구 메뉴 → "프리셋" 으로 PresetView 직접 오픈

저장 위치: `~/.tasty/presets/{workspace,tab,pane}/<name>.toml`. 파일명이 정본 — 같은 kind 내 이름 중복 불가. 충돌 시 `unique_name`이 `-N` suffix 를 자동 부여.

### 편집
PresetView(EditorView 계열, modeless, 종류별 1개 인스턴스)에서 좌측 리스트로 항목을 고르고 우측에서 이름, subtitle(workspace), 레이아웃 트리, 각 leaf surface 의 (kind, cwd, 시작 명령어, kind 별 파라미터)를 편집한다. 시작 명령어 입력 폼은 surface kind 가 `terminal` 일 때만 표시된다.

### 적용
- 단축키(`apply_workspace_preset` / `apply_tab_preset` / `apply_pane_preset` — 기본 빈 칸, 사용자 할당): 적용 popup 을 열고 항목 선택 → Enter → 새 워크스페이스/탭/페인 생성 + 포커스 이동
- CLI: `tasty preset apply --kind ... --name ...` (포커스 이동 없음)
- IPC: `preset.apply` (포커스 이동 없음)

terminal 의 시작 명령어는 PTY 가 ready 된 직후 stdin 에 한 줄로 자동 입력된다.

### IPC / CLI 표면
| IPC method | CLI subcommand | 권한 |
|------------|----------------|-----|
| `preset.list` | `tasty preset list --kind <k>` | `SurfaceRead` |
| `preset.get` | `tasty preset get --kind <k> --name <n>` | `SurfaceRead` |
| `preset.save` | `tasty preset save --kind <k> --name <n> --file <p> [--overwrite]` | `SurfaceWrite` |
| `preset.delete` | `tasty preset delete --kind <k> --name <n>` | `SurfaceWrite` |
| `preset.rename` | `tasty preset rename --kind <k> --from <a> --to <b>` | `SurfaceWrite` |
| `preset.capture` | `tasty preset capture --kind <k> --source-id <id> [--name <n>]` | `SurfaceWrite` |
| `preset.apply` | `tasty preset apply --kind <k> --name <n> [--target-pane <id>] [--target-workspace <id>]` | `SurfaceWrite` |

`preset.apply` 는 CLI/IPC 경로에서 항상 `focus: false` — 포커스 독립성 원칙. 단축키 호출만 새 인스턴스로 포커스가 이동한다.
