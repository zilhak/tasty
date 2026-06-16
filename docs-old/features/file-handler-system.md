# 파일 핸들러 시스템 (file-handler-system)

- **Status**: Implemented

URI/경로 입력을 받아 **(1) 파일 형식 식별 → (2) 등록된 핸들러 디스패치** 두 단계를 거치는 통합 라우팅 시스템. 두 단계는 독립 모듈(`src/file_format/`, `src/file_handler/`)로 분리되어 있고, `file_handler` 만 `file_format::DetectorId` 를 import 하는 단방향 의존 관계를 가진다.

### 형식 식별 (`FileFormatRegistry`)
- DetectorId 네임스페이스: 일반 `[a-z0-9-]{1,64}` (예: `markdown`), 호스트 예약 `$<word>` (예: `$directory`)
- detector rule 종류 (Cheap = file IO 없음, Deep = 8KB head read + `infer` MIME 추정):
  - `extension`: 확장자 대소문자 무시 매칭 (Cheap)
  - `path_glob`: 파일명 wildcard (`*` 만, 본격 globset 도입은 Phase B+) (Cheap)
  - `is_directory`: 대상이 디렉토리 (Cheap)
  - `magic`: `offset` + `bytes_hex` 매칭 (Deep, regular file 한정 / FIFO/socket/device skip)
  - `mime`: `infer` 기반 MIME 추정 후 대소문자 무관 비교 (Deep)
  - `lua`: 인라인 Lua 5.4 sandbox 평가 (Deep, host/user TOML 만 — plugin 출처는 install 시 drop+warn). `target = { path, is_directory, bytes_head, mime, has_prefix }` 글로벌 주입. 메모리 cap 8MB, 명령어 cap 1M, `io`/`os`/`debug`/`package`/`require`/`load*`/`dofile` 제거, bytecode 청크 금지
  - `structure_check`: 절대 경로의 JSON Schema 파일로 target 의 구조 검증 (Deep). 현재 JSON 입력만 지원 (`.json` 확장자), 5MB 초과 파일은 즉시 false. schema/target 읽기·파싱 실패 시 false + warn 로그
- Deep 평가는 한 `identify` 호출당 `DeepCtx` 가 head/MIME 캐시 → 같은 파일을 여러 detector 가 평가해도 IO 는 1회만
- pre-filter: 디렉토리 대상은 `is_directory` rule 가진 detector 만, 파일 대상은 그 외만 평가 (cross-match 방지)
- 호스트 default 는 `src/file/format/defaults/default-file-format.toml` 에 정의 — html, `$directory` 등. markdown / image detector 는 각각 `com.tasty.markdown` / `com.tasty.image` plugin 이 contribute

### 핸들러 디스패치 (`FileHandlerRegistry`)
- HandlerId 형식: `host/<name>`, `<plugin_id>/<name>`, `user/<name>` (`<name>` 은 `[a-z0-9-]{1,32}`)
- HandlerAction: `OpenSurface { surface_kind, param_key }`, `Ipc { method, owner_plugin_id }`, `System` (OS 기본 열기 위임)
- actor 별 schema 강제:
  - host TOML: OpenSurface / Ipc / System 전부 허용
  - plugin TOML: System 금지 (serde reject) — sandbox 일관성
  - user TOML: 전부 허용 (사용자가 자기 시스템에 명시 위임)
- `handlers_for(detector)` 정렬: priority asc → tie 시 owner 우선 `user > plugin > host` → handler id 사전순
- `all_handlers()`: picker 용 전체 enabled 목록

### Contribution-based registry
- 두 registry 모두 출처별(Host/Plugin(id)/User) contribution 을 보관하고, finalize 시 patch semantics 로 머지 (last-writer-wins, `None` 은 덮어쓰지 않음). rules 는 union + dedupe.
- plugin uninstall 시 그 plugin 의 contribution 만 제거 — host default / user 설정은 그대로 유지.

### Plugin 통합
- 매니페스트 `[[contributes.detector]]` / `[[contributes.handler]]` 로 추가. validate 단계:
  - detector: `$` prefix(예약 sentinel) plugin 추가 금지, 매직바이트 hex 검증, path traversal 차단
  - handler: short-name 패턴 검증, detector 참조 cross-check, surface_kind/ipc_prefix 등록 여부 확인
- 권한:
  - `file_handler.define`: 새 detector + handler 추가 권한
  - `file_handler.extend:<id>`: 기존 detector 에 rule 추가
  - `file_handler.handle:<id>`: 기존 detector 에 handler 만 추가
  - `$unknown` 같은 sentinel 은 모든 토큰에서 reject (예약어)
- **부팅 시 자동 등록**: enabled plugin(=builtin 공식 플러그인 포함)의 detector/handler 는 부팅 진입점 `discover_and_start()` 에서 plugin process spawn 과 **분리하여** 두 registry 에 install 된다 — 런타임 `enable()` 경로와 대칭. 따라서 앱을 켠 직후 별도 enable 조작 없이 공식 플러그인의 파일 동작(예: `.md` 더블클릭 → markdown surface 새 탭)이 기본으로 작동한다. 출처 무관(builtin/외부 동일), spawn 실패와 무관하게 정적 contribute 는 등록된다. `install_plugin_*` 는 같은 owner 의 기존 contribution 을 retain 으로 교체하므로 disable→enable 재등록·다중 윈도우(공유 registry Arc)에서도 중복 누적이 없다(멱등).
- **다중 핸들러**: 같은 detector 에 핸들러가 둘 이상이면 picker 없이 `handlers_for` 정렬(priority→owner→id) 1순위가 자동 디스패치된다 — 정렬이 결정론적이라 1순위 선택도 결정론적.

### User config
- `~/.tasty/file-handlers.toml` — 한 파일에 `[[detector]]` + `[[handler]]` 섹션 혼재 가능, 부팅 시 1회 로드
- patch 가능 필드: priority, display_name_i18n_key, disabled, action — `disabled = true` 만 명시 override (false 는 무시)
- 파일 없음 → 정상 (사용자 설정 없는 상태). parse 실패 시 warn + 전체 무시. entry 단위 schema 오류는 그 entry 만 reject.

### Picker popup
- `file_handler_picker` PopupDef (480x동적). 헤더(대상/형식) + 두 열(후보/최근) + [열기]/[취소]
- 좌측: handler id 사전순, 우측: `~/.tasty/file-handler-recent.json` LRU(cap 10) 순서 — 현재 등록 안 된 id 는 표시만 제외(저장 파일은 유지)
- 더블클릭 또는 [열기] → Selected dispatch + recent 기록 / [취소]/ESC/X → Cancelled
- picker 자체는 dispatch 하지 않고 `state.dialogs.file_handler_picker.result` 로만 결과를 남김 — 호스트 본체 layer 가 frame 끝에 소비해 실행 + atomic save

### RecentPicks
- 저장: `~/.tasty/file-handler-recent.json` (홈 못 찾으면 임시 디렉토리로 fallback)
- 원자적 쓰기: `<path>.tmp` 작성 → rename. fsync 는 안 함 (UX 영향)
- LRU dedupe + cap=10. parse 실패 시 빈 리스트로 시작 + warn

### User config 직렬화 (MD4)
- `FileFormatRegistry::export_user_config()` / `FileHandlerRegistry::export_user_config()` — RuleOrigin::User / HandlerOwner::User 만 추려 TOML 문자열로 emit
- `save_user_config(path)` — tempfile + rename 으로 atomic write. 빈 결과(사용자 항목 0)도 빈 파일로 덮어쓴다
- `file_handlers_save::save_combined_user_config(file_format, file_handler, path)` — 두 registry 의 user export 를 합쳐 한 파일에 atomic write. Settings UI 에서 한쪽만 저장 시 다른 쪽 섹션이 사라지는 문제 방지
- patch semantics 보존: user contribution 의 Some 필드만 emit, 호스트/플러그인이 제공한 base 는 미포함
- `DetectorRuleKind::Unknown` 의 raw payload 도 round-trip — forward-compat 유지
- 주석/공백/key 순서는 보존 안 함 (재발급). 사용자 손편집 친화 보존이 필요해지면 `toml_edit` 도입

### Settings UI — FileHandler 탭
- `Settings > File Handler` 탭, 3 개 sub-tab (Detectors / Handlers / Extension Mapping). 각 sub-tab 첫 진입 시 역할 paragraph + 컬럼 의미 bullet 리스트가 표시되어 사용자가 i18n key 외부 문서 없이 개념을 파악할 수 있다.
- **Detectors**: 등록된 모든 detector 의 id, 출처 (host/plugin/user), rule 종류 요약 (ext/glob/mime/magic/dir/lua/structure), enabled 토글
  - Enabled 체크박스 = host/plugin default 를 user-origin override (`disabled_override`) 로 덮어씀
  - user-origin 항목은 Remove 버튼으로 삭제 가능 (저장 시 적용)
  - "+ Add user detector" inline form — id + 확장자 (콤마/공백 구분) + 단일 path-glob 으로 간단 정의. 고급 rule (magic / mime / structure-check) 은 TOML 손편집
- **Handlers**: priority 오름차순 정렬된 handler 목록 — priority, id, owner, detector, action 요약 (`surface:<kind>` / `ipc:<method>` / `system`), enabled 토글
  - Enabled / Remove 동작은 detector 와 동일 (user-origin 만 Remove 가능)
  - "+ Add user handler" inline form — short-name (`user/<name>` 으로 저장) + detector dropdown + priority + action kind (open-surface / ipc / system) + 각 kind 별 필드 (surface_kind+param_key / method)
- 편집은 `FileHandlerEditDraft` 에 누적되며 Settings 의 Save 버튼이 registry 에 commit + `save_combined_user_config` 로 `~/.tasty/file-handlers.toml` atomic write
- Recent picks 는 picker popup 내 "최근" 열에서만 노출되며, Settings UI 에서는 sub-tab 으로 분리하지 않는다 (forget 은 `~/.tasty/file-handler-recent.json` 직접 편집)

### Extension Mapping (Phase E ME4)
- Plugin 매니페스트 등록 경로: `[[contributes.detector]]` + `[[contributes.handler]]` 로 plugin 이 자기 확장자와 핸들러를 contribute. host TOML 도 같은 구조 — last-writer-wins. 실 예시는 `crates/tasty-plugin-image/tasty-plugin.toml` (image detector + viewer handler) / `crates/tasty-plugin-html/tasty-plugin.toml` (html viewer handler, detector 는 host 유지)
- Settings UI 의 Extension Mapping sub-tab 은 광고 detector ≥ 2 인 ext 만 기본 노출. plugin 만 광고하는 ext (예: image) 는 plugin disabled 시 UI 에서 사라짐 — 단순화 의도, plugin enable 로 즉시 복귀
- 같은 확장자를 광고하는 detector 가 여러 개 있을 때 사용자가 직접 우선순위를 정할 수 있는 표 (`[[extension_priority]]`)
- 호스트 default / 사용자 설정 양쪽에서 정의 가능. plugin manifest 는 이 섹션을 못 씀 — 사용자 영역
- TOML: `[[extension_priority]] extension = "md" order = ["mdx-strict", "markdown"]`
- last-writer-wins (host → user 순서 install) — 사용자가 호스트 default 를 덮어쓸 수 있음
- 빈 `order = []` 는 entry 제거 의도로 해석
- `identify` 의 cheap path 가 파일 확장자가 있을 때 이 표를 fast path 로 사용 — 표에 적힌 detector 가 enabled + 광고 detector 안에 있으면 1순위로 선택. 표에 없거나 부적격이면 `install_order` 순서로 fallback
- Settings UI: `File Handler` 탭의 `Extension Mapping` sub-tab — 기본 노출 대상은 광고 detector 가 2개 이상인 확장자(=실제로 우선순위 의미가 있는 항목)이며, draft 에 추가된 확장자는 candidate 수와 무관하게 함께 노출된다. ↑/↓ 버튼으로 재정렬, 하단 textbox 로 새 확장자 직접 추가 가능, "Reset" 으로 user entry 제거 → host default 가 있으면 그것이, 없으면 `install_order` 순서가 다시 적용된다
- Settings 저장 시 `save_combined_user_config` 로 `~/.tasty/file-handlers.toml` 에 atomic write — `[[handler]]` 섹션 보존

### 한계 (현재)
- mouse.rs 콜사이트 변경은 별도 작업 — ctrl+click 시 여전히 기존 `terminal_link::open_uri` 가 동작
- `structure_check` 는 JSON 입력만 지원 — YAML/TOML 은 별도 deps 도입 후
- 상대 경로의 `spec_path` 는 호스트 CWD 기준으로 해석됨 — plugin 매니페스트 dir 기준 해석은 install 단계에서 수행 필요 (별도 작업)
- `path_glob` 은 단순 `*` wildcard 만, 본격 globset 도입은 후속
- Deep 평가는 sync — worker thread 분리 (`AppEvent::IdentifyDone`) 는 별도 작업
