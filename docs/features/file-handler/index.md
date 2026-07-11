# 파일 핸들러 (File handler)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`file_handler.dispatch`) · plugin (contribute)
- **ADR**: 없음
- **코드**: `src/file/format/`(식별) + `src/file_handler/`(디스패치); IPC `file_handler.{reload,dispatch}`
- **화면**: [설정 창](../settings/screens/settings.md) Handler 탭의 파일 서브탭 3종 · file_handler_picker popup

## 목적

URI/경로 입력을 **(1) 형식 식별 → (2) 등록 핸들러 디스패치** 두 단계로 라우팅한다(예: `.md` 더블클릭 → markdown surface 새 탭). 두 단계는 독립 모듈로, `file_handler` 만 `file_format::DetectorId` 를 import 하는 단방향 의존.

## 내부 동작

### 형식 식별 (`FileFormatRegistry`)

DetectorId 는 일반 `[a-z0-9-]` / 호스트 예약 `$<word>`(예: `$directory`). rule 종류 — Cheap(IO 없음): `extension`·`path_glob`·`is_directory`; Deep(8KB head + MIME): `magic`·`mime`·`lua`(sandbox 5.4)·`structure_check`(JSON Schema). Deep 평가는 호출당 head/MIME 를 캐시(IO 1회). pre-filter 로 디렉토리/파일 대상에 맞는 detector 만 평가. 호스트 default 는 `default-file-format.toml`(html, `$directory`…), markdown/image detector 는 각 plugin 이 contribute.

> **후속 과제(detector 이중소스)**: 호스트 `save.rs` 의 `md`/`markdown` 확장자 detector 룰이 markdown plugin 매니페스트 `[[contributes.detector]]` 와 이중소스인지 확인이 남아 있다. markdown de-pluginize 범위 밖(파일포맷 detector 영역과 직교)이라 별도 과제로 분리한다.

### 핸들러 디스패치 (`FileHandlerRegistry`)

HandlerId: `host/<name>` · `<plugin_id>/<name>` · `user/<name>`. HandlerAction: `OpenSurface{surface_kind, param_key}` · `Ipc{method}` · `System`(OS 위임). actor 별 schema 강제 — **plugin TOML 은 System 금지**(sandbox 일관성), user TOML 은 전부 허용. `handlers_for` 정렬: priority asc → owner(`user > plugin > host`) → id 사전순. 같은 detector 에 핸들러 여럿이면 picker 없이 1순위 자동(결정론적).

### Contribution 머지 + 부팅 자동 등록

두 registry 모두 출처별(Host/Plugin/User) contribution 을 보관하고 finalize 시 patch 머지(last-writer-wins, rules union+dedupe). **부팅 시** enabled plugin(빌트인 포함)의 detector/handler 가 plugin spawn 과 **분리되어** 등록된다 — 그래서 앱 켠 직후 별도 enable 없이 `.md`/이미지 등이 동작. 멱등(retain 교체)이라 disable→enable·다중 윈도우에서 중복 없음. plugin uninstall 시 그 contribution 만 제거.

### Picker + Recent

핸들러가 모호하거나 사용자가 선택하게 할 때 `file_handler_picker` popup(후보 + 최근 2열, [열기]/[취소]). Recent 는 `~/.tasty/file-handler-recent.json` LRU(cap 10, atomic write). picker 는 dispatch 하지 않고 결과만 남기고 호스트 layer 가 frame 끝에 실행 + 저장.

### 권한

`file_handler.define`(새 detector+handler) · `file_handler.extend:<id>`(기존 detector 에 rule) · `file_handler.handle:<id>`(기존 detector 에 handler). `$` sentinel 은 모든 토큰에서 reject. ([plugin-permissions](../../dev-guide/plugin-permissions.md).)

## 인터페이스

- **사용자**: Settings **Handler** 탭의 파일 서브탭(File Detectors / File Handlers / File Extension Mapping — 토글·user 항목 추가/삭제, 확장자 우선순위). user 설정은 `~/.tasty/file-handlers.toml`(부팅 1회 로드, atomic write). 같은 탭의 Hook Handlers 서브탭은 파일 핸들러가 아니라 [공유 훅 핸들러 레지스트리](../webhook/index.md) 편집이다.
- **AI Agent / CLI**: `file_handler.dispatch`(임의 경로를 흐름에 진입, plugin 호출은 FsRead 권한) · `file_handler.reload`(user 설정 reload) · `tasty file-handler` CLI.

## 비-목표

- 개별 surface kind 의 렌더/동작 — [concepts/plugins](../../concepts/plugins.md), [plugins/](../../plugins/index.md).

## 관련

- **레지스트리 정본 템플릿**: 이 `FileHandlerRegistry`(3출처 patch 병합 + actor 별 action 스키마 + owner tie-break 정렬)를 공유 훅 핸들러 레지스트리가 미러링한다 — [webhook](../webhook/index.md) · [ADR-0047](../../adr/0047-shared-hook-handler-registry-source-gate.md). 차이: file handler 의 `detector`↔hook 의 `source` 게이트, `System` action↔`ShellCommand`(hook 출처 전용).
- [plugin-permissions](../../dev-guide/plugin-permissions.md) · [plugin-development](../../dev-guide/plugin-development.md) · [settings](../settings/index.md)
