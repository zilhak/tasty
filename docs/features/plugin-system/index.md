# 플러그인 관리 (Plugins)

- **Status**: Implemented
- **주체**: 로컬 사용자(플러그인 창) · AI Agent(`tasty plugin` CLI). 원격 접속 사용자는 mirror 로 봄.
- **ADR**: 없음
- **코드**: `src/view/plugins.rs`, `src/view/plugins/ui/`, `crates/tasty-cli/src/commands/plugin_cmd.rs`
- **화면**: [아래 절](#화면)

> 이 문서는 *플러그인을 설치·관리* 하는 사용자/에이전트 기능이다. *플러그인을 제작* 하는 법은 [plugin-development](../../dev-guide/plugin-development.md) · [plugin-permissions](../../dev-guide/plugin-permissions.md) · [plugin-development 민감 데이터](../../dev-guide/plugin-development.md#민감-데이터--regular--secret--keyring-선택).

## 목적

플러그인을 설치·활성/비활성·제거하는 기능. [사이드바](../sidebar/index.md) 플러그인 버튼이 여는 **관리 창**(GUI)과 **`tasty plugin` CLI** 두 가지로 제공한다 — tasty-특화 기능이라 [identity §2.2](../../identity.md) 에 따라 IPC/CLI 양면 제공.

## 내부 동작

### 창 — 세 탭

- **Installed (list)**: 설치된 플러그인 목록. 각 항목:
  - **이름 줄과 설명**: 상세 이름은 UI 최대 글자 크기·주 글자색·보통 굵기(굵은 UI 글꼴을 쓰지 않는다)이고, 같은 줄에 버전 Tag(앱 공용 Tag 위젯, mono 작은 글자)와 기본 제공 플러그인이면 기본 Tag `built-in` 이 이어진다. agent 강조색은 built-in 표시에 쓰지 않는다. 설명은 body 크기·보조 글자색·줄 높이 `line-height-ui`(1.4)이며, 상세 열이 넓어도 `measure-lg`(460) 폭에서 줄바꿈한다.
  - **메타 줄**: 상세 이름 줄 아래 `작성자 · id · 홈페이지` 를 ` · ` 로 이은 mono caption·text-muted 한 줄. 작성자가 여럿이면 쉼표로 잇고, 작성자가 없으면 id 가 맨 앞이다. 폭이 모자라면 항목 단위로 다음 줄로 넘어가며, 구분점은 뒤 항목과 함께 넘어가 줄 끝에 점만 남지 않는다. 한 항목이 줄 폭보다 길면 끝을 말줄임한다. 홈페이지가 있으면 마지막 항목이다. scheme 이 `http`·`https`(대소문자 무시)인 주소만 scheme 을 뺀 accent-primary 밑줄 링크로 보이고 누르면 기본 브라우저로 연다. 다른 scheme(`file:`·`javascript:` 등)이나 scheme 없는 값은 다른 항목과 같은 평문으로 보이며 눌러도 열리지 않는다. 본문에 따로 `Homepage:` 줄은 없다. 아바타·이름 줄·메타 줄로 된 정체 블록은 Attention 상세와 같은 위젯(`plugin_detail_identity`)이다.
  - **액션 바**: 상세 아래, 본문 스크롤 밖에 늘 보이는 바. 상세 열 폭 전체를 쓰고 열 아래 끝에 붙으며, 위 1px 구분선이 열 양끝에 닿는다. 왼쪽에 enable/disable Switch 와 현재 상태 라벨(`Enabled`/`Disabled`, 스위치와 한 컨트롤이라 라벨을 눌러도 전환되고 키보드 초점은 한 칸), 오른쪽에 `Configure`(ghost, 톱니 아이콘, 설정 창 Plugins 탭으로 이동)와 `Uninstall`(secondary, 위험 색 글자). 키보드 Tab 순서는 본문 다음 스위치 → Configure → Uninstall 로 화면 순서와 같다.
  - **health error** 인디케이터 (enable 상태인데 오류인 플러그인).
  - **권한 read-only 표시** (창에서 권한을 토글하지 않는다). 상세 `Permissions` 절은 권한마다 버전 표시와 같은 공용 Tag(mono 작은 글자)로 보인다.
  - **절 배치**: 상세의 절 사이에는 구분선이 없고 `space-lg` 만큼 띄운다. 순서는 `PERMISSIONS` → `COMMANDS`(명령이 있을 때) → `SURFACE KINDS`(kind 가 있을 때) → `INSTALL PATH` 다. 머리글은 대문자 mono micro·text-muted 이며 본문과 `space-sm` 떨어진다. Surface kinds 는 Permissions 처럼 kind 마다 공용 Tag 로 보인다.
  - **설치 경로 절**(상세의 마지막 절): 머리글 줄에 대문자 mono `INSTALL PATH` 와 오른쪽 끝 `Open folder`(secondary sm, 폴더 아이콘, OS 파일 관리자로 연다). 그 아래 설치 경로 한 줄과 `Log: <경로>` 한 줄은 mono caption·text-muted 이고, 공백이 없어도 아무 문자에서 줄바꿈하며 선택할 수 있다. 말줄임·툴팁은 없다. 버튼이 경로 줄에 없으므로 창 최소 폭 720 과 기본 폭 880 에서 긴 경로가 버튼을 밀어내지 않는다.
  - **Commands 절**: 명령마다 한 행. 왼쪽에 명령 제목(mono term-sm·text-secondary, 길면 말줄임), 오른쪽에 단축키 키캡(할당이 없으면 없음), 행 아래 1px 구분선. 행 높이는 구분선을 포함해 32 이고 아래 여백을 더하지 않는다. 키캡은 매니페스트 조합을 `+` 로 나눠 앞뒤 공백을 빼고, 한 글자 키는 대문자, 나머지는 첫 글자만 대문자로 쓴다(`ctrl + shift + h` → `Ctrl` `Shift` `H`). Windows·Linux 는 이 낱말 키캡을 매니페스트 순서대로 보인다. macOS 는 키바인딩 매핑(`alt` → Command, `option` → Option, `ctrl` → Control)을 거친 뒤 수식키를 Apple 순서 Ctrl · Option · Shift · Command(⌃ ⌥ ⇧ ⌘)로 놓고 키를 맨 뒤에 둔다. 각 수식키는 설정 › 일반의 표시 스타일(`alt_display_style` · `option_display_style` · `shift_display_style`)을 설정 키바인딩 행·수식키 안내와 같이 따른다: `alt` 는 `Alt`/`Cmd`/⌘, `option` 은 `Option`/⌥, `shift` 는 `Shift`/⇧. 기호는 수식키 안내처럼 아이콘 키캡으로 그린다. Ctrl 은 표시 스타일과 글리프가 없어 늘 낱말 `Ctrl` 이다. 표시 스타일은 창을 열 때와 창 안의 플러그인 동작 뒤 목록을 다시 받을 때 읽는다(설정 창과 Plugins 창은 동시에 열리지 않는 modal 이다).
  - **uninstall**: 액션 바의 `Uninstall` 을 누르면 같은 바의 내용이 그 자리에서 확인으로 바뀐다(본문 끝 블록·popup 없음): alertTriangle(16, accent-attention) · `Uninstall {이름}?`(body·text-primary) 아래 안내 한 줄(caption·text-muted) · `Cancel`(ghost) · `Uninstall`(danger). 안내는 기본 제공 플러그인이면 `Built-in — it won't be installed again on the next launch.`, 아니면 `Its files are removed. Settings stay until you delete them.` 다. 확인은 그 plugin id 에 묶이며, 다른 플러그인을 고르면 확인이 취소되어 원래 플러그인으로 돌아와도 평소 바가 보인다. 확인이 열리면 키보드 포커스가 `Cancel` 로 간다. 확인이 열린 동안에는 어느 탭에서든 Esc 가 확인을 취소한다(다른 탭에 가려진 확인도 닫혀, Installed 로 돌아와도 다시 보이지 않는다). 확인이 없을 때 Esc 는 이 창에서 아무 일도 하지 않는다. 짧은 문구는 평소 바와 같은 높이이고, 줄바꿈되는 긴 문구에서는 바가 글 높이만큼 커진다.
- **Attention (확인 필요)**: 등록 거부(서명/신뢰) 또는 실행 실패(health error) plugin 을 사유·조치와 함께 보여준다. 탭 라벨에 개수를 danger 배지로 표시. 목록 행 끝의 severity 점은 액션 바의 점과 같은 `status-dot-size`(8) 지름이다. 상세 맨 위 정체 블록(아바타 · 이름 줄 · `작성자 · id · 홈페이지` 메타 줄)은 Installed 와 같은 위젯이다. 홈페이지는 `http`·`https` 주소일 때만 링크로 싣고, 다른 값은 메타 줄에 넣지 않는다. 그 아래 매니페스트 설명(Installed 와 같은 설명 위젯, 비면 생략)이 사유 배너 앞에 온다. 서명이 유효하지 않은 사유(`SignatureInvalid`)는 매니페스트가 서명한 내용 그대로라는 보장이 없어 설명과 홈페이지를 숨기고, 설명 자리에 `Description hidden — the signature is invalid.`(caption·text-muted, `measure-lg` 폭에서 줄바꿈)를 둔다. 신뢰하지 않은 키(`UnknownKey`)·권한 변경·실행 오류는 매니페스트 설명과 홈페이지를 그대로 보인다. 정체 블록 · 설명 · 사유 배너 · 사유 detail 은 `space-lg` 간격으로 쌓는다. 액션 바도 Installed 와 같은 바 틀(가로 14 · 세로 `space-md` 여백, 위 1px 구분선, 본문 스크롤 밖 열 바닥)이다. 버튼이 없는 서명 사유의 바도 열 폭 전체를 써서 구분선이 열 양끝에 닿는다. 바 안에는 왼쪽에 status-dot 크기의 점과 `Not registered`/`Needs review`(12, severity 색), 오른쪽에 사유별 버튼 — 권한 변경은 `Re-approve`(primary), 실행 오류는 `Configure`(ghost, 톱니 아이콘) — 이 있고 서명 사유에는 버튼이 없다. 사유별 절 머리글(Permission changes · Signature · Log)은 상세 절처럼 대문자 mono micro·text-muted 다. Signature invalid 항목은 `Signature` 머리글 아래에 고정 설명 `The signature does not match this plugin's files.`과 서명 검증이 실패한 원인(사이드카 파일 없음 · 읽기 오류 · 서명 길이 오류 등) 한 줄을 mono 글자로 보인다.
- **fingerprint 줄**: Attention 서명 절과 Add 신뢰 상자가 같은 줄을 쓴다. colon-hex 값이 16바이트를 넘으면 앞 8바이트와 뒤 8바이트를 ` … `로 이어 한 줄로 보이고, 툴팁과 복사 버튼은 전체 값을 쓴다.
- **Install (add)**: 디렉터리(`tasty-plugin.toml`)에서 설치. 제목 없이 한 화면에서 진행한다.
  - **경로 선택**: `Plugin folder` 머리글 아래 mono 경로 입력, `Find folder…`(폴더 선택 대화상자), `Verify` 버튼과 설명 문단을 둔다. 경로가 비면 `Verify`는 비활성이다. 경로를 고치면 확인한 매니페스트를 버린다. 확인하기 전에는 그 아래에 `Choose a folder and press Verify to read its manifest.` 안내 상자를 두고, 매니페스트를 읽지 못하면(파일 없음·TOML 파싱 실패) 그 자리에 "Can't read tasty-plugin.toml" 오류 상자를, 읽었지만 선언 검사(바이너리 경로·감지기 등)에 실패하면 같은 상자에 "tasty-plugin.toml is not valid" 제목을 둔다. 두 상자 모두 아래 줄에 원문 메시지를 번역하지 않고 보인다.
  - **미리보기**: 경로 선택 바로 아래에 매니페스트 카드(이름·버전, `id · 첫 작성자 +N`(작성자 전체는 툴팁), 설명, 권한·surface 종류 Tag, 원본 경로, 홈페이지 링크)와 그 아래 신뢰 판정 상자. 홈페이지 링크는 text-secondary 글자에 밑줄을 늘 긋고, 마우스를 올리면 text-primary, 키보드 포커스면 focus ring 을 두른다. 누르면 기본 브라우저로 연다. 링크는 `http`·`https` 주소만이고, 다른 값은 같은 자리에 밑줄 없는 text-secondary 평문으로 보이며 열리지 않는다. 권한이나 surface 종류가 없으면 caption 크기 `None`을 적는다.
  - **신뢰 판정 상자 다섯 가지**: 신뢰한 게시자(success, 그대로 추가) · 확인되지 않은 게시자와 권한 변경(warning, 추가하면 키나 새 권한 묶음을 신뢰) · 공개 키 파일 없음과 서명 확인 실패(danger, 추가 불가). 서명 확인 실패를 뺀 미신뢰 상자에는 fingerprint 줄이 붙는다. 서명 확인 실패의 원인은 상자에 쓰지 않고 로그에 남긴다.
  - **액션 바**: 매니페스트를 확인하기 전에는 Cancel 만 둔다. 확인한 뒤에는 왼쪽에 부여할 권한 수(`No permissions` · `Grants 1 permission` · `Grants N permissions`, caption), 오른쪽에 Cancel과 `Add plugin`. Cancel 은 경로와 미리보기를 비운다. 추가하면 키나 새 권한 묶음을 신뢰하게 되는 경우(`UntrustedWithPubkey`)는 같은 Primary 버튼이 `Trust & add`이고 `TrustAndInstall`로 설치한다. 추가할 수 없으면(이미 설치됨 · 공개 키 파일 없음 · 서명 확인 실패) 버튼을 disabled로 두고 왼쪽 문구를 그 이유로 바꾼다. 이미 설치된 플러그인도 신뢰 상자는 판정대로 그린다.

### CLI (`tasty plugin …`)

`list` / `show <id>` / `install <path>` / `remove <id>` / `enable <id>` / `disable <id>` 등. CLI install 은 사용자 의도적 명령이라 매니페스트 권한을 자동 grant.

`enable <id>` / `disable <id>` 는 발견된 설치 패키지가 있는 ID만 받는다. 미설치 ID는
GUI·headless 모두 실패하며, 설정 파일과 메모리의 활성/비활성 상태를 바꾸거나 성공 이벤트를
발행하지 않는다. 토글 실패 응답은 기존 `-32000`과 `enable failed:` / `disable failed:`
접두어를 유지하고 `plugin '<id>' not installed`를 이유로 돌려준다. 설치·부팅은 패키지
목록을 채운 뒤 토글을 수행한다.

끄거나(`disable`) 지우면(`remove`) 그 플러그인이 등록한 surface kind 는 재부팅 없이 **철회**된다
([ADR-0026](../../adr/0026-plugin-registration-and-lifecycle.md)). 그 kind 로
새 surface 를 만들려는 요청은 "그 kind 를 제공하던 plugin 이 꺼졌거나, 다시 켠 뒤 아직 연결되지 않았다" 는
사유로 거절되고(IPC 오류 문장, 사용자 조작이면 경고 toast), `surface.kinds` · 서피스 변환 팝업 목록에서 빠진다. 이미 열린 surface 는
닫거나 바꾸지 않는다 — 다시 켜면 이어진다. 닫은 탭 복원 · 프리셋 적용처럼 복원하는 경로는 그 자리를
kind 대기 placeholder 로 두었다가 다시 켜면 채운다.

### 설정(configure)

플러그인별 설정은 이 창이 아니라 [설정 창](../settings/index.md) 의 Plugins 탭에서 편집한다.

## 인터페이스

- **사용자(GUI)**: 사이드바 플러그인 버튼 → 관리 창. 탭 전환, 토글/설치/제거.
- **AI Agent(CLI)**: `tasty plugin {list,show,install,remove,enable,disable}`.
- **연결**:
  - 플러그인 설정 → [`features/settings/`](../settings/index.md) (Plugins 탭)
  - 플러그인 제작/권한/민감데이터 → [plugin-development](../../dev-guide/plugin-development.md) · [plugin-permissions](../../dev-guide/plugin-permissions.md) · [plugin-development 민감 데이터](../../dev-guide/plugin-development.md#민감-데이터--regular--secret--keyring-선택)

## 비-목표

- 플러그인 *제작*(SDK·매니페스트·권한 모델·서명) — dev-guide.
- 마켓플레이스(registry/install-by-id) — 현재 미도입(보류, [ADR-0025](../../adr/0025-plugin-trust-and-distribution.md)).
- 권한 *변경* UI — 창은 read-only. (권한은 설치 시 grant.)

## Acceptance Criteria

- 사이드바 플러그인 버튼 클릭 시 관리 창이 열린다 (Installed / Attention / Install 탭).
- Installed 에서 enable/disable 토글이 동작하고, 오류 플러그인에 health 인디케이터가 뜬다.
- Given 창 최소 폭 720 또는 기본 폭 880 과 열 폭보다 긴 설치 경로, When 그 플러그인을 고르면, Then `Open folder` 와 설치 경로·로그 경로 전체가 창 안에 보인다.
- Install 탭에서 디렉터리 설치 시 매니페스트·권한 미리보기와 신뢰 검증을 거친다.
- `tasty plugin list/install/remove/enable/disable` CLI 가 동일 동작을 수행한다.
- 미설치 ID의 enable/disable은 GUI·headless 모두 실패하고, 기존 설정 파일은 바이트 단위로 유지되며 없던 설정 파일을 만들지 않는다.
- 설치된 plugin은 disable 후 enable로 다시 실행할 수 있다.
- Given surface kind 를 등록한 plugin 이 켜져 있고 When 그 plugin 을 disable 하거나 remove 하면 Then 그 kind 로 새 surface 를 만드는 요청은 "제공 plugin 이 꺼졌거나 아직 다시 연결되지 않았다" 는 사유로 거절되고 `surface.kinds` 에서 빠지며, 이미 열린 그 kind 의 surface 는 그대로 남는다.
- Given 위처럼 철회된 kind 가 있고 When 그 plugin 을 다시 enable 하면 Then 그 kind 로 다시 만들 수 있다.
- 플러그인 설정은 설정 창 Plugins 탭에 나타난다 (이 창 아님).

> GUI 는 스크린샷, 설치/관리 동작은 `tasty plugin` CLI 시나리오로 검증.

## 구현

- 창: `src/view/plugins.rs` (`PluginsView`), `src/view/plugins/ui/list.rs`(Installed) / `add.rs`(Install).
- CLI: `crates/tasty-cli/src/commands/plugin_cmd.rs` (`PluginCommands`).

## 화면

화면정의서 — **플러그인 관리 창 화면**.

- **트리거 위치**: [사이드바](../sidebar/index.md#화면) 하단 **플러그인 버튼**
- **시각 소스**: `site/vendor/ui_kits/terminal/overlays/plugins_window.jsx` — claude design

### 트리거

사이드바 하단 **플러그인 버튼** 클릭 → 플러그인 관리 모달 창이 열린다.

### 레이아웃

```
┌──────────────────────────────────────────┐
│ [Installed]  [Attention]  [Install]       │  탭
├──────────────────────────────────────────┤
│ Installed:                                │
│  ▸ plugin A          [enable ▢]  [⌫]      │  목록 — 토글 / uninstall
│    권한: …(read-only)                      │
│  ▸ plugin B  ⚠health  [enable ▣]          │
│                                            │
│ Install:                                  │
│  경로: …/tasty-plugin.toml                 │
│  매니페스트 미리보기 + 권한 미리보기          │
│  [신뢰 검증] → [ Add ]                      │
└──────────────────────────────────────────┘
```

### UI 요소 인벤토리

- **탭**: Installed / Attention(확인 필요 — 등록 거부·실행 실패 plugin, 개수 danger 배지) / Install.
- **Installed 항목**:
  - 상세 아래 액션 바의 enable/disable Switch·Configure·Uninstall, health error 인디케이터(오류 플러그인).
  - 권한 표시 — **read-only** (창에서 권한 토글 없음).
  - 설치 경로 절 — `INSTALL PATH` 머리글 줄 오른쪽 `Open folder`, 줄바꿈·선택 가능한 설치 경로와 로그 경로. **uninstall**.
- **Install 폼**:
  - 디렉터리 경로(`tasty-plugin.toml`).
  - 매니페스트 + 권한 **미리보기**.
  - **신뢰/서명 상태** — Trusted 면 바로 Add, 권한 변경 시 재신뢰(TrustAndInstall) 후 Add.
- **설정(configure)** 은 여기 없음 → [설정 창](../settings/screens/settings.md) Plugins 탭.

### 상태별 시각

- **health error**: enable 상태인데 오류인 플러그인에 빨간 인디케이터/박스.
- **신뢰 상태**: Trusted / PermissionsChanged 등에 따라 Install 버튼·안내 문구가 달라진다.

### 시각 소스

`site/vendor/ui_kits/terminal/overlays/plugins_window.jsx` — 창 치수·탭·목록·설치 폼 배치의 단일 출처. 스크린샷: `site/vendor/screens/plugins_window-installed.png`, `plugins_window-install.png`, `plugins_window-modal.png`.
