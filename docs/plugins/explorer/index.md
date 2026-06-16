# File Explorer (`com.tasty.explorer`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface)
- **배포/통합**: bundled · surface_kind(plugin-rendered) — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-explorer/`
- **권한**: `surface.read` · `surface.write` · `fs.read` · `ui.settings_page`
- **화면**: [screens/explorer.md](screens/explorer.md)

## 목적

파일 트리를 보고 탐색하는 **`explorer` surface 종류**를 제공하는 번들 플러그인. surface 로 열려 작업 영역 타일 안에서 디렉토리를 탐색한다.

## 내부 동작

- **surface_kind `explorer`** — `rendering` 기본값(플러그인 직접 렌더). host 트리엔 `RemoteSurface` marker 로 들어가고 플러그인 UI DSL 로 트리를 그린다 (→ [work-area Surface 종류](../../features/work-area/index.md#surface-종류)).
- **commands** — `explorer.refresh`(새로고침) · `explorer.go_up`(상위 디렉토리).
- **settings_page** — `explorer` 페이지(폰트 override 등) → [설정](../../features/settings/index.md) 플러그인 페이지로 노출.
- 시작 디렉토리는 surface 생성 시 carry 된 cwd(Surface cwd invariant).

## 인터페이스

- **사용자**: explorer surface 를 열어 탐색. refresh/go_up 명령, 설정 페이지에서 폰트 조정.
- **AI Agent**: surface 생성은 [work-area](../../features/work-area/index.md) (`tasty new tab --type explorer` / `split --... ` ). 플러그인 전용 CLI 는 없음.

## 비-목표

- surface 생성/배치 도메인 — [work-area](../../features/work-area/index.md).
- 파일 *열기*(확장자 → 뷰어 매핑) — markdown/image/html 플러그인의 핸들러.

## Acceptance Criteria

- [ ] Given explorer 플러그인 활성 When `tasty new tab --type explorer` Then explorer surface 가 열린다.
- [ ] Given explorer surface When go_up/refresh Then 상위 이동/새로고침된다.
- [ ] Given 설정 창 Then explorer 페이지(폰트 override)가 보인다.

## 화면

- [screens/explorer.md](screens/explorer.md) — 파일 트리 surface.
</content>
