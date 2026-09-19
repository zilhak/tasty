# 파일 핸들러 (File handler)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`file_handler.dispatch`) · plugin (contribute)
- **ADR**: [URL 대상은 picker·실행 계층에만](../../adr/0272-url-targets-enter-the-handler-picker-not-identify.md)
- **코드**: `src/file/format/`(식별) + `src/file/handler/`(디스패치); IPC `file_handler.{reload,dispatch}`
- **화면**: [설정 창](../settings/screens/settings.md) Handler 탭의 파일 서브탭 3종 · file_handler_picker popup

## 목적

URI/경로 입력을 **(1) 형식 식별 → (2) 등록 핸들러 디스패치** 두 단계로 라우팅한다(예: `.md` 더블클릭 → markdown surface 새 탭). 두 단계는 독립 모듈로, `file_handler` 만 `file_format::DetectorId` 를 import 하는 단방향 의존.

## 내부 동작

### 형식 식별 (`FileFormatRegistry`)

DetectorId 는 일반 `[a-z0-9-]` / 호스트 예약 `$<word>`(예: `$directory`). rule 종류 — Cheap(IO 없음): `extension`·`path_glob`·`is_directory`; Deep(8KB head + MIME): `magic`·`mime`·`lua`(sandbox 5.4)·`structure_check`(JSON Schema). Deep 평가는 호출당 head/MIME 를 캐시(IO 1회). pre-filter 로 디렉토리/파일 대상에 맞는 detector 만 평가. 호스트 default 는 `default-file-format.toml`(html, `$directory`…), markdown/image detector 는 각 plugin 이 contribute.

> **후속 과제(detector 이중소스)**: 호스트 `save.rs` 의 `md`/`markdown` 확장자 detector 룰이 markdown plugin 매니페스트 `[[contributes.detector]]` 와 이중소스인지 확인이 남아 있다. markdown de-pluginize 범위 밖(파일포맷 detector 영역과 직교)이라 별도 과제로 분리한다.

### 핸들러 디스패치 (`FileHandlerRegistry`)

HandlerId: `host/<name>` · `<plugin_id>/<name>` · `user/<name>`. HandlerAction: `OpenSurface{surface_kind, param_key}` · `Ipc{method}` · `System`(OS 위임). actor 별 schema 강제 — **plugin TOML 은 System 금지**(sandbox 일관성), user TOML 은 전부 허용. `handlers_for` 정렬: priority asc → owner(`user > plugin > host`) → id 사전순. 같은 detector 에 핸들러 여럿이면 picker 없이 1순위 자동(결정론적).

### 대상 — 파일 경로와 URL

dispatch 대상(`DispatchTarget`)은 파일 경로(`File`) 또는 `http`/`https` URL(`Url`)이다. 형식 식별은 파일에만 돌고, URL 은 detector 없이 picker 로 직행한다 — 그래서 URL 에는 1순위 자동 실행이 없다. 식별·평가 계층은 경로 문자열에 URL(`<scheme>://`, scheme 두 글자 이상)이 담겨 들어와도 매칭하지도 읽지도 않는다.

액션별로 URL 을 받는지는 한 판정이 정하고, picker 의 후보 목록 · Recent 목록 · 최종 실행에 똑같이 걸린다(거절된 선택은 recent 에 기록하지 않는다).

URL 대상의 picker 헤더에는 **URL 전용 형태가 따로 없다** — detector 를 안 거치므로 형식 Tag 가 "형식 알 수 없음" 이고, 경로 자리에 URL 원문이 파일 경로와 같은 규칙으로(길면 앞에서 잘려) 들어간다. 디자인 canonical 은 파일 경로만 다루므로 이 자리는 파일 규칙을 그대로 쓴 것이다.

| 액션 | 파일 대상 | URL 대상 |
|------|-----------|----------|
| `OpenSurface` | `{param_key: 경로}` | `param_key = "url"` 인 핸들러만 — `{url: 원문}` (html 핸들러 → webview) |
| `System` | `file://` URI 로 OS opener | 원문 그대로 OS opener |
| `Ipc` | plugin 에 `{"path": 경로}` | 받지 않음 |

### Contribution 머지 + 부팅 자동 등록

두 registry 모두 출처별(Host/Plugin/User) contribution 을 보관하고 finalize 시 patch 머지(last-writer-wins, rules union+dedupe). **부팅 시** enabled plugin(빌트인 포함)의 detector/handler 가 plugin spawn 과 **분리되어** 등록된다 — 그래서 앱 켠 직후 별도 enable 없이 `.md`/이미지 등이 동작. 멱등(retain 교체)이라 disable→enable·다중 윈도우에서 중복 없음. plugin uninstall 시 그 contribution 만 제거.

### Picker + Recent

사용자가 고르게 할 때 `file_handler_picker` popup 이 뜬다. Recent 는 `~/.tasty/file-handler-recent.json` LRU(cap 10, atomic write). picker 는 dispatch 하지 않고 결과만 남기고 호스트 layer 가 frame 끝에 실행 + 저장.

**형상**: 420px headless 모달 — 프레임이 자기 헤더를 그려 **경로가 한 번만** 나온다(공통 타이틀바와 짝지으면 같은 경로가 서로 다른 두 말줄임으로 두 번 잘린다). 헤더는 제목 + 감지된 형식 Tag + mono 경로이고, 긴 경로는 **앞에서** 자른다(`…/federation/screens.tsx` — 파일명이 꼬리이고 그것이 파일을 식별한다). 본문은 후보 그룹과 `Recent` 그룹이 **한 목록** 안에 있고(2열이 아니다 — 420px 에서 한 컬럼은 `icon · name · origin` 을 못 담는다), 선택은 두 그룹을 가로질러 **하나**다. 목록이 264px 를 넘으면 그 영역만 스크롤하고 하단에 페이드가 잘림을 보인다 — 헤더와 footer 는 스크롤하지 않는다. footer 는 [취소]/[열기] 둘뿐이고, 선택이 없으면 [열기]가 비활성이다. 닫힘은 Esc 와 [취소]뿐 — scrim 클릭은 닫지 않는다.

**행의 글리프와 이름은 저장되어 있지 않다 — 도출한다.** `FileHandler` 에는 icon 필드도 표시명 필드도 없다. 글리프는 action 이 여는 surface kind 에서 뽑고(markdown → markdown, editor → edit, pager → terminal, directory → folder, table → columns, binary → layers, log → listView), 모르는 kind 와 여는 surface 가 없는 액션(`Ipc`·`System`)은 `file` 로 떨어진다. 이름은 선언된 표시명(`display_name_i18n_key`)이고, 선언이 없으면 id 의 마지막 `/` 뒤 조각을 **mono** 로 쓴다 — 그 글꼴이 "선언된 이름이 없다" 는 표시다. 둘째 줄이 출처 낱말(built-in / you / plugin)과 전체 id(34자 넘으면 앞에서 자름)를 들어 **id 는 행마다 정확히 한 번** 나오고, Recent 행은 거기에 마지막 사용 시각이 붙는다. plugin 출처는 출처 낱말과 글리프만 mauve 로 물든다 — 이름은 아니다(plugin 의 핸들러도 하는 일로 불린다).

**picker 는 순수 dispatcher 다.** 1회 열고 아무것도 저장하지 않는다 — 형식→핸들러 바인딩을 저장하는 체크박스는 없다. 저장되는 바인딩은 보고 되돌릴 자리가 있어야 하고 그 자리는 설정 › 핸들러다.

**어떤 형식으로 뜨는가**: 매칭 핸들러가 있으면 picker 없이 1순위가 자동 실행되므로(`handlers_for` 정렬), picker 가 뜨는 경로는 **셋**이고 그중 둘만 fallback 이다.

| 경로 | 후보 | fallback 인가 |
|---|---|---|
| 이 detector 에 매칭되는 handler 가 0개 (`Core::apply_identify_result`) | `all_handlers()` | 예 |
| 터미널 링크 메뉴의 "연결 동작" — 식별을 건너뛰고 강제로 연다 | `all_handlers()` | 예 |
| 원격(mirror) surface 의 경로 링크 (`open_remote_placeholder_picker`) | **없음** — 후보도 recent 도 안 싣는다 | 아니오 |

셋째는 화면 경로가 원격 호스트 경로라 로컬 핸들러로 열 수 없어서 목록을 **일부러 비운다**. 그래서 `candidates_are_fallback` 이 `false` 이고 fallback 신호(attention 톤 · 1회성 띠)가 안 뜬다.

앞의 둘에서는 후보가 `handlers_for(d)` 가 아니라 `all_handlers()` 이고(recent 와 중복 제거), 네 신호가 그 약속의 차이를 나른다: 그룹 라벨이 attention 톤의 "전체 핸들러", caption 이 "이 형식에 맞는 핸들러가 없습니다.", 헤더 Tag 가 "형식 알 수 없음", 그리고 헤더 아래 띠가 **1회성**이고 다음에도 이 화면이 나온다고 적는다(user TOML 미변경). 기본 핸들러 Tag 는 이 상태에 붙지 않는다 — 매칭이 없으면 기본도 없다.

**empty-state**: 갈림은 **이 popup 이 실을 행이 0개인가**이지 시스템에 핸들러가 몇 개인가가 아니다 — 후보와 recent 가 **둘 다 비면** 목록 자리가 중앙 블록으로 바뀐다. 그래서 실제로 이 상태에 닿는 것은 위 표의 **셋째 경로**(원격 placeholder)다. 앞의 둘은 `all_handlers()` 를 싣는데 host 기본 핸들러가 `default-file-handlers.toml` 에 박혀 있어 plugin 을 전부 꺼도 0 이 되지 않는다. 블록은 흐린 파일 글리프 + "등록된 핸들러가 없습니다." + 한 줄 안내 + "설정에서 핸들러 등록" 버튼(Settings 를 `FileHandler` 탭으로 오픈)이고, 프레임 폭과 footer 는 그대로고 [열기]만 비활성이다 — 같은 다이얼로그의 한 상태이지 다른 화면이 아니다.

### 권한

`file_handler.define`(새 detector+handler) · `file_handler.extend:<id>`(기존 detector 에 rule) · `file_handler.handle:<id>`(기존 detector 에 handler). `$` sentinel 은 모든 토큰에서 reject. ([plugin-permissions](../../dev-guide/plugin-permissions.md).)

### Origin 소유권과 비동기 완료

`origin_surface_id` 를 지정한 요청은 처음부터 그 surface 소유 engine으로 라우팅된다.
식별 완료와 picker 선택도 origin을 유지하며, OpenSurface 결과는 origin의 pane에
기존 활성 탭과 그 탭의 포커스된 surface를 유지한 채 새 탭으로 추가된다.
터미널·비터미널 kind에 같은 규칙을 적용하며 origin 자체가 비활성 탭에 있어도
현재 선택을 origin으로 옮기지 않는다. 대기 중 다른 창으로 포커스를 옮겨도 대상은
바뀌지 않는다. 소유 engine이 parked 상태이면 그 상태에 적용한다.

처음부터 없는 origin은 기존 `-32602`와 unowned-target 문구로 거절한다. 접수 후
origin이 사라지면 경고 로그를 남기고 실행하지 않는다. 다른 창의 새 탭으로 폴백하지
않는다. `accepted: true`는 큐 접수만 뜻하며 완료 응답을 추가로 보내지 않는다.
picker 취소는 실행·recent 기록 모두 없다. origin 생략은 기존 focused-window /
사용자 NewTab 동작을 유지한다. Ipc handler의 path-only payload는 그대로다.
근거: [ADR-0279](../../adr/0279-file-dispatch-retains-origin-through-completion.md).

## 인터페이스

- **사용자**: Settings **Handler** 탭의 파일 서브탭(File Detectors / File Handlers / File Extension Mapping — 토글·user 항목 추가/삭제, 확장자 우선순위). user 설정은 `~/.tasty/file-handlers.toml`(부팅 1회 로드, atomic write). 같은 탭의 Hook Handlers 서브탭은 파일 핸들러가 아니라 [공유 훅 핸들러 레지스트리](../webhook/index.md) 편집이다.
- **AI Agent / CLI**: `file_handler.dispatch`(임의 경로를 흐름에 진입, plugin 호출은 FsRead 권한 — 경로 자리의 URL 은 `-32602` 로 거절) · `file_handler.reload`(user 설정 reload) · `tasty file-handler` CLI.

## 비-목표

- 개별 surface kind 의 렌더/동작 — [concepts/plugins](../../concepts/plugins.md), [plugins/](../../plugins/index.md).

## 관련

- **레지스트리 정본 템플릿**: 이 `FileHandlerRegistry`(3출처 patch 병합 + actor 별 action 스키마 + owner tie-break 정렬)를 공유 훅 핸들러 레지스트리가 미러링한다 — [webhook](../webhook/index.md) · [ADR-0047](../../adr/0047-shared-hook-handler-registry-source-gate.md). 차이: file handler 의 `detector`↔hook 의 `source` 게이트, `System` action↔`ShellCommand`(hook 출처 전용).
- [plugin-permissions](../../dev-guide/plugin-permissions.md) · [plugin-development](../../dev-guide/plugin-development.md) · [settings](../settings/index.md)
