# 언어팩 (Language packs)

- **Status**: Implemented
- **주체**: 로컬 사용자
- **ADR**: [ADR-0114](../../adr/0114-language-pack-directory-shape-and-english-fallback.md)
- **코드**: `crates/tasty-i18n/src/lib.rs`(로더 · 스캐너 · `LoadReport`), `src/boot/locale.rs`(부팅 적용), `src/app/boot_machine.rs`(폴백 토스트), `crates/tasty-ui-widgets/src/language_select.rs` + `src/view/settings/ui/tabs/general.rs`(콤보)
- **화면**: [설정 창](../settings/screens/settings.md) General › Language 콤보 · 부팅 직후 경고 토스트

## 목적

내장 3 개 언어(`en`/`ko`/`ja`) 밖의 언어를 사용자가 **파일을 놓는 것만으로** 추가하고, 설정 창에서 다른 언어와 같은 콤보로 고를 수 있게 한다. 팩이 문자열만이 아니라 글리프(폰트)까지 책임진다는 계약을 형상에 박아, 문자열은 뜨는데 글자가 □ 로 깨지는 상태를 "팩의 결함" 으로 드러낸다.

## 내부 동작 (headless-valid)

### 형상

- **언어팩** = `~/.tasty/lang/<code>/pack.toml` (+ 동봉 폰트 파일). 내장이 아닌 새 코드용.
  - `[meta] name` — 표시 이름(선택, 없으면 코드가 라벨).
  - `[font]` — **필수**. `builtin = true` / `file = "<팩 기준 상대경로>"` / `family = "<패밀리명>"` / `candidates = [...]` 중 하나(우선순위 그 순서). 섹션이 없거나 넷 다 없으면 형상 위반.
  - 나머지 문자열 키 — 내장 lang 파일과 같은 트리. 영어 베이스 위에 overlay.
  - **크기 상한 2 MiB** — 넘으면 파싱 전에 거부하고 warn 후 목록에서 뺀다. 가장 큰 내장 파일(`lang/ja.toml`, 89 KiB / 약 1,300 키)의 23 배로, 장황한 언어·주석·키 증가를 모두 곱해도(약 12 배) 여유가 남는다. 상한이 있는 이유는 팩을 읽는 비용이 **사용자가 놓은 파일 크기에 비례**하고 그 비용을 설정 창 첫 오픈 때 렌더 스레드가 물기 때문이다.
  - **빈 값은 "번역 없음"** — 값이 비었거나 공백뿐인 키는 overlay 에서 빠져 아래 층이 그대로 보인다(라벨 없는 버튼이 생기지 않는다). 로드 시 몇 개가 빠졌는지 `tracing::warn!` 한 줄. **두 로드 경로에 같은 규칙**이 걸리고([ADR-0124](../../adr/0124-blank-value-rule-is-load-path-independent.md)), 드러나는 것만 다르다 — 팩은 영어 베이스 위에 얹히므로 영어가, 오버라이드는 그 내장 언어 위에 얹히므로 **그 언어의 원래 문구**가 보인다. 일부러 비운 텍스트가 필요하면 **폭 없는 문자(U+200B)** 를 쓴다 — NBSP(U+00A0)는 안 된다(`str::trim` 이 유니코드 `White_Space` 를 전부 먹는다).
- **오버라이드** = `~/.tasty/lang/<builtin>.toml` 단일 파일. 내장 코드 전용, `[font]` 불필요. 내장이 아닌 코드의 단일 파일은 팩이 아니다(경고 후 무시). 크기 상한과 **빈 값 규칙**은 팩과 같다 — 빠진 키는 내장 `<code>.toml` 의 문구로 되돌아간다.
- 내장 `lang/{en,ko,ja}.toml` 도 `[meta] name` 을 갖는다(`English` / `한국어` / `日本語`).

### 발견 (`available_languages`)

내장 3 개(고정 순서) + `~/.tasty/lang/` 의 유효한 팩(코드순). 항목마다 코드 · 표시 이름 · 출처(`Builtin` / `BuiltinOverridden` / `Pack`) · `[font]` 선언 종류 · 경로. 파싱 실패, `[font]` 부재/무효, 크기 상한 초과, 내장 코드 이름의 디렉토리, 새 코드의 단일 파일은 전부 `tracing::warn!` + 제외. 규칙 표는 [dev-guide/i18n](../../dev-guide/i18n.md) "언어팩".

발견은 목록에 필요한 `[meta] name` 과 `[font]` **둘만** 읽는다 — 문자열 키를 flatten 하지 않는다. 이 스캔은 설정 창 첫 오픈 때 렌더 스레드에서 동기로 돌기 때문이다. 수락/거절 판정은 실제 로드와 동일해 "목록에 오르면 로드된다" 가 성립한다.

### 부팅 적용과 폴백

1. `general.language` 를 정규화(공백 → `en`)해 `tasty_i18n::init` 에 넘긴다.
2. 내장 코드면 내장 + 오버라이드. 아니면 `<code>/pack.toml` 을 읽는다.
3. 팩이 **없거나 거부되면(크기 상한 초과 포함) 영어로 폴백** — 테이블은 en(+ en 오버라이드), `current_language()` 와 자식 env `TASTY_LOCALE` 은 `en`. `LoadReport { requested, effective, outcome }` 가 이유(`PackMissing { expected }` / `PackInvalid { path, error }`)를 담는다.
4. 알림: 로드 시점 `tracing::warn!` 한 줄(요청 코드 + 기대 경로, **경로는 온전한 값**) — headless/CLI 는 이것이 전부. GUI 는 부팅 후 **경고 토스트 1회**(`i18n.warn.pack_missing` / `pack_invalid`, 창 스코프). 토스트는 본문 200자 캡이 있어, 메시지가 캡을 넘기면 **경로만 가운데를 생략**해(`…`) 캡 안에 맞춘다 — 머리(어느 루트)와 꼬리(`<code>/pack.toml`)는 남는다. 잘려서 안내가 사라지는 것보다 경로를 줄이는 쪽이 낫다.
5. **설정값은 어느 경로에서도 고쳐 쓰지 않는다.** 팩을 넣고 재시작하면 그 언어가 뜬다.

폰트 선언은 검증·보관만 한다 — 파일/패밀리로 resolve 해 `TASTY_LOCALE_FONT` 를 채우는 단계는 아직 없다(현재 항상 unset). 폰트 resolve 가 실패해도 문자열은 로드된다는 정책(글리프 □)은 그 단계의 것이다.

### 설정 콤보

목록은 설정 창을 열 때 1회 스캔. 라벨은 `[meta] name` 없으면 코드. 현재 값이 목록에 없으면 `"<code> (not found)"` 행을 끝에 붙여 선택 상태로 보여 주고, 그 행을 다시 골라도 값은 바뀌지 않는다. 다른 행을 고르면 그 코드가 draft 에 들어가고 Save 시 `general.language` 로 영속 — 적용은 재시작 후.

## 인터페이스

- **사용자 트리거**: `~/.tasty/lang/<code>/pack.toml` 작성 → 설정 창 General › Language 에서 선택 → Save → 재시작. 또는 `config.toml` 의 `general.language` 직접 편집.
- **AI Agent (IPC/CLI)**: 전용 메서드 없음 — `tasty debug settings apply --json '{"general":{"language":"<code>"}}'`(debug 빌드)로 설정값을 바꾸고 재시작. CLI(`tasty <subcommand>`)도 같은 부팅 경로를 타므로 팩의 `cli.*` 키가 CLI 출력에 반영된다.
- **원격 / 점유**: 해당 없음(로컬 인스턴스의 표시 언어).

## 비-목표 (Out of scope)

- 폰트 resolve(`file`/`family`/`candidates` → 실제 파일, `TASTY_LOCALE_FONT` 주입, 실패 시 경고).
- 재시작 없는 언어 전환.
- 팩 설치/배포 도구(다운로드 · 서명 · 버전).
- plugin 자체 `lang/` 의 팩화 — plugin 은 SDK `Translator` 가 `TASTY_LOCALE` 로 자기 파일을 고른다(팩 코드가 곧 파일명).

## Acceptance Criteria

- Given `~/.tasty/lang/` 에 `xx/pack.toml`(`[font] builtin = true`) · `yy/pack.toml`(`[font]` 없음) · `ko.toml` · `zz.toml` · `bad/pack.toml`(깨진 TOML) When `available_languages()` Then 목록은 `en, ko(BuiltinOverridden), ja, xx(Pack)` 이고 `yy`/`zz`/`bad` 는 없다(각각 warn).
- Given `general.language = "xx"` When 부팅 Then `xx` 의 문자열이 뜨고 `current_language() == "xx"`, `TASTY_LOCALE=xx`.
- Given `general.language = "zz"` 이고 팩 없음 When 부팅 Then 영어 UI + 경고 토스트 1회(코드 · 기대 경로), headless/CLI 는 warn 1줄, `config.toml` 의 값은 `zz` 그대로.
- Given 콤보의 현재 값이 목록에 없음 When 콤보를 열고 닫음 Then 값이 바뀌지 않는다.
- Given 팩이 어떤 키의 값을 `""` 로 두었음 When 그 팩으로 부팅 Then 그 키는 영어로 보이고 화면에 빈 라벨이 없다.
- Given `ko.toml` 오버라이드가 같은 키를 `""` 로 두었음 When `ko` 로 부팅 Then 그 키는 **내장 한국어 문구**로 보이고 화면에 빈 라벨이 없다(팩과 같은 규칙, 드러나는 층만 다르다).
- Given `~/.tasty/lang/big/pack.toml` 이 2 MiB 를 넘음 When `available_languages()` Then `big` 은 목록에 없고 warn 이 남는다. And `general.language = "big"` 으로 부팅하면 영어로 폴백하고 `config.toml` 의 값은 `big` 그대로다.
- Given 데이터 루트가 길어 경고 메시지가 200자를 넘김 When 폴백 토스트 Then 경로 가운데가 `…` 로 줄고 `(200자 제한)` 접미가 붙지 않는다.

## 화면

- [설정 창](../settings/screens/settings.md) — General › Language 콤보(갤러리 Settings specimen 의 `language_select` 행과 같은 위젯).
