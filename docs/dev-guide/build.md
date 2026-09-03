# 빌드 가이드

tasty 의 워크스페이스 구조, 빌드 프로필, 빌드 시간 최적화. 강제 정책(어떤 프로필을 언제 쓰는지)은 [`../../CLAUDE.md`](../../CLAUDE.md) "빌드".

## 워크스페이스 구조

cargo workspace — **본 바이너리(`src/`) + `crates/*` 다수**(현재 48개). 크레이트는 레이어로 나뉜다(전체 목록·각 역할은 `crates/` 와 각 `Cargo.toml`):

| 레이어 | 예 | 성격 |
|--------|-----|------|
| **type-\*** primitive | `tasty-type-geometry`(길이), `tasty-type-appearance`(색·theme schema) | 최하위 schema/primitive |
| 도메인 leaf (GUI-free) | `tasty-model`, `tasty-i18n`, `tasty-settings`, `tasty-themes`, `tasty-terminal`, `tasty-memory`, `tasty-hooks`, `tasty-ipc`, `tasty-ssh`, `tasty-remote`, `tasty-portscan` 등 | 공용 도메인·IO |
| plugin 인프라 | `tasty-plugin-protocol`, `tasty-plugin-sdk`, `tasty-plugin-manifest`, `tasty-host-plugin` | 호스트↔plugin 와이어·SDK |
| 번들 plugin | `tasty-plugin-{claude,codex,image,html,markdown,git-viewer,clipboard-viewer,mesh-demo}` | → [`../plugins/`](../plugins/index.md) |
| CLI / 테스트 | `tasty-cli`, `tasty-tui-simulator` | |

본 바이너리는 `pub use tasty_core as ...` 식으로 재수출해 `crate::model::X` / `crate::theme::theme()` 같은 경로가 그대로 동작한다.

### type-\* layer 의존 규약 (필수)

`tasty-type-*` 는 **primitive/schema 레이어**다.

- 그룹 내부 의존은 자유 (예: `tasty-type-appearance → tasty-type-geometry`).
- **도메인/IO crate 의존 금지** — `tasty-model`/`tasty-themes`/본 바이너리 등을 의존하지 않는다.
- **그룹 내 순환 금지.**

새 type-\* crate 도 이 3원칙을 따른다 — 의존 그래프가 한 방향으로 유지되어 순환 위험 0.

## 빌드 프로필 (3종)

| 프로필 | 정의 | LTO | 용도 |
|--------|------|-----|------|
| `dev` (기본) | `opt-level = 0` | off | 일상 개발 `cargo build` |
| `release` | `opt-level = 3`, `strip = true` | **thin** | 최적화 검증 `cargo build --release` |
| `dist` | `inherits = "release"` | **full** (`lto = true`) | 배포 산출물 `cargo build --profile dist` |

- **`release` = thin LTO**: 크레이트 IR 요약을 공유해 cross-crate inlining 을 **병렬** 적용. full 의 95–99% 효과를 1/3 시간에 — 일상 "릴리즈 검증" 은 모두 이걸 쓴다.
- **`dist` = full LTO**: 모든 IR 을 단일 LLVM 모듈로 합쳐 재최적화. 단일 스레드 단계가 길어 약 3.5배 느림. **배포 바이너리(DMG/MSIX/AppImage) 빌드 시에만** 쓴다. (AI 자체 검증 빌드에는 절대 사용 금지.)

```bash
cargo build                 # debug
cargo check                 # 타입검사만 (가장 빠름), -p <crate> 로 단일 크레이트
cargo build --release       # thin LTO 검증
cargo build --profile dist  # 배포용
```

### headless(`--no-default-features`) 빌드

`gui` 는 default feature 라 `cargo build` 는 winit/wgpu/egui 를 켠다. gui 없는 headless/CLI 인스턴스(로컬 GUI 사용자 없이 IPC/CLI + 원격 attach 만 쓰는 인스턴스, `docs/identity.md` "headless 환경을 의식한다")는 `--no-default-features` 로 빌드한다.

```bash
cargo check --workspace --no-default-features   # headless 컴파일 검증
cargo build --workspace --no-default-features   # headless 빌드
```

gui 전용 심볼(`AppState.toasts` 등)을 `#[cfg(feature = "gui")]` 게이팅 없이 쓰면 gui 빌드는 통과하지만 headless 빌드만 깨진다. 이 회귀는 `.github/workflows/crossplatform-check.yml` 의 `check-headless` 잡(`cargo check --workspace --no-default-features --locked`)이 매 PR 자동 검출한다.

본체+플러그인을 한 번에 다루는 래퍼가 있다. `just build` 는 빌드·스테이징만(실행 X), `just run` 은 빌드 후 호스트까지 실행한다. 둘 다 플러그인을 빌드·스테이징하며, 호스트는 부팅 시 builtin 을 강제 덮어쓰기 설치하므로 플러그인 소스 변경이 (`just build` 면 다음 실행 시, `just run` 이면 그 실행에서) 반영된다.

```bash
just build                  # 본체+플러그인 debug 빌드·스테이징 (실행 X)
just build --release        # 동일, release 프로필
just run                    # 본체+플러그인 debug 빌드 + 실행
just run --release          # 동일, release 프로필 (나머지 인자는 호스트로 passthrough)
```

**release/dist 로컬 빌드는 builtin 을 자동 재서명한다.** `--release`(및 `--profile dist`)는
trust 게이트가 켜지므로(debug 는 `#[cfg(debug_assertions)]` 로 우회), `build-plugins` 가
cargo build 전에 서명 키를 보장(`scripts/ensure-sign-key.sh`: `SIGN_KEY_PATH` env →
`release.pem` → `dev.pem`+`gen-dev-key.sh`)하고 임베드 `dev-pubkey.bin` 을 재도출한 뒤,
plugin 빌드 후 `sign-bundle.sh --all-builtins` 로 전체 매니페스트를 재서명한다. 따라서
매니페스트를 언제 고치든(plugin 버전 자동 bump 포함) 로컬 release 산출물은 항상 게이트를
통과한다. `just build`/`just run`(debug)은 게이트가 꺼져 있어 서명 단계를 건너뛴다(기본
dev 워크플로에 openssl 의존 미부과). dist 스크립트(`build-*.{sh,ps1}`)도 같은 규칙을
쓴다 — 상세는 [plugin-packaging](plugin-packaging.md).

## Plugin 빌드 / 스테이징

번들 plugin(`crates/tasty-plugin-*` 중 `tasty-plugin.toml` 보유)은 부팅 시 `install_builtins_if_needed` 가 `~/.tasty/plugins/<id>/` 로 자동 sync 한다. `bundle_root()` fallback 이 `<exe_dir>/builtin-plugins/`(= `target/<profile>/builtin-plugins/`)라, **그 경로에 스테이징만 해두면** 부팅 시 user dir 까지 흐른다. debug 빌드는 `ensure_dev_bundle` 이 매 부팅 mtime 기반으로 workspace→bundle 을 sync 하므로 `cargo build` → `cargo run` 만으로 동작.

```bash
just build-plugins                # 모든 bin plugin → release 스테이징
just build-plugin claude          # 단일 plugin
just build-all                    # plugins + 본 바이너리
just link-plugins                 # cp 대신 symlink (rebuild 즉시 반영)
```

산출물: `target/<profile>/builtin-plugins/<id>/{tasty-plugin.toml, <bin>, tasty-plugin.toml.sig(non-debug), lang/}`. lib-only crate(protocol/sdk/manifest)는 manifest 부재로 자동 skip. release/dist 프로필은 `build-plugins`·`link-plugins` 가 위 자동 서명 단계를 수행해 `.sig` 까지 스테이징한다(`link-plugins` 는 crate-dir `.sig` 를 symlink 해 재서명이 자동 반영). debug 는 서명 없이 매니페스트/bin/lang 만.

> **위 스테이징은 "부팅 시 user dir 로 흐른다" 전제(=호스트 재시작)다.** **이미 실행 중인 tasty 에** 플러그인 변경만(호스트 재빌드·재시작 없이) 반영·검증하려면 — 실행 프로필 확인 → 그 프로필로 플러그인 빌드 → 재서명 → `plugin disable → upgrade-builtins[ --force] → enable` 절차를 쓴다: [plugin-development §9.1](plugin-development.md#91-실행-중인-tasty-에-번들-플러그인만-반복-갱신-호스트-재빌드재시작-불필요).

## 배포 패키징

```bash
./scripts/build-macos-dmg.sh    # .app + .dmg (자동 --profile dist)
./scripts/build-linux.sh        # tar.gz + .deb + .rpm + .AppImage (uname -m 자동 감지)
./scripts/build-windows.ps1     # zip + .msi (cargo-wix + WiX 3.x)
```

- **macOS** `.app` 은 ad-hoc 코드 서명(`codesign --sign -`). 번들 plugin 은 **`Contents/Resources/plugins/<id>/`** 에 staging 한다 — `Contents/MacOS/` 하위에 두면 codesign 이 그 디렉터리를 nested code 로 간주해 번들로 파싱하려다 `bundle format unrecognized` 로 **서명 자체가 실패**한다. `tests/macos_bundle_codesign.rs` 가 이 레이아웃을 강제하고, 런타임 탐색은 `bundle_root()`(`crates/tasty-host-plugin/src/builtin.rs`)가 담당한다.
- **macOS 번들 `Info.plist` 의 정본은 `scripts/build-macos-dmg.sh` 의 heredoc 하나뿐**이다 (`$VERSION` 이 `Cargo.toml` 에서 치환되고, 스크립트 말미의 `PLIST_VER` 검증이 그 정합을 강제한다). 여기엔 `CFBundle*` 계열 외에 TCC usage description 키가 들어간다 — macOS 는 보호 리소스 접근 프롬프트 **본문에 이 문자열을 그대로 표시**하며, 키가 없으면 이유 없는 프롬프트가 뜨거나 일부 서비스는 접근 시도 자체가 즉시 실패한다.

  | 키 | 언제 뜨는가 |
  |----|-------------|
  | `NSDownloadsFolderUsageDescription` | 셸 명령이 `~/Downloads` 에 접근할 때 |
  | `NSDocumentsFolderUsageDescription` | 셸 명령이 `~/Documents` 에 접근할 때 |
  | `NSDesktopFolderUsageDescription` | 셸 명령이 `~/Desktop` 에 접근할 때 |
  | `NSRemovableVolumesUsageDescription` | 마운트된 이동식 볼륨(USB·SD) 접근 시 |
  | `NSNetworkVolumesUsageDescription` | 마운트된 네트워크 볼륨 접근 시 |

  문구는 "터미널이 사용자가 친 명령을 대신 실행한다" 는 tasty 의 실제 접근 이유를 담는다. `t()` i18n 을 타지 않는다 — plist 문자열은 OS 가 표시하며, 다국어화는 `Contents/Resources/*.lproj/InfoPlist.strings` 라는 별개 메커니즘이 필요하다(현재 미도입, 영어 단일). `NSAppleEventsUsageDescription` 은 넣지 않는다 — tasty 자신은 Apple Events 를 보내지 않고(소스 전체에 `osascript`/`NSAppleScript` 호출 0건), 셸 자식 프로세스가 `osascript` 를 실행할 때 Automation 승인이 tasty 로 귀속되는지는 실기 확인이 필요한 별건이다. 키 누락 회귀는 `tests/macos_bundle_codesign.rs` 의 정적 검사가 막는다(스크립트 heredoc 을 직접 파싱하므로 Linux CI 에서도 돈다).
- **macOS 개발 중 권한 프롬프트가 매번 다시 뜬다면** — ad-hoc 서명은 designated requirement 가 cdhash 뿐이라 재빌드마다 macOS 가 다른 앱으로 보고 TCC 승인을 버린다. `./scripts/macos-codesign-identity.sh --create` 로 self-signed 인증서(`Tasty Dev`)를 한 번 발급해두면, 이후 `./scripts/install-macos.sh` 가 키체인에서 그것을 자동으로 집어 서명하므로 재빌드해도 승인이 유지된다. DR 이 cdhash 대신 `identifier "com.zilhak.tasty" and certificate leaf` 가 되기 때문이다. 자동 선택은 로컬 설치 빌드(`NO_DMG=1`)에서만 동작한다 — DMG 배포본은 그 인증서를 신뢰하지 않는 머신으로 가므로 ad-hoc 을 유지한다. 다른 identity 를 쓰려면 `TASTY_CODESIGN_IDENTITY` 로 지정한다.
- **macOS Full Disk Access 는 서명 identity 와 묶여 있다** — FDA 를 부여해도 **ad-hoc 서명 빌드는 재빌드할 때마다 권한이 초기화된다**(designated requirement 가 cdhash 뿐이라 macOS 가 매번 다른 앱으로 본다). 직접 빌드해 쓰면서 FDA 를 유지하려면 위 `Tasty Dev` 인증서 발급이 **선행 조건**이다. "FDA 를 줬는데 또 초기화됐다" 의 원인이 대부분 이것이라, 앱의 FDA 안내 문구도 이 조건을 함께 알린다([macOS 권한](../features/macos-permissions/index.md)).
- **Linux** `.deb`/`.rpm` 은 `cargo-deb` / `cargo-generate-rpm`, `.AppImage` 는 `linuxdeploy`(ELF 의존 라이브러리를 전부 번들 + rpath `$ORIGIN` → distro 무관 동작). 패키지 메타데이터는 `Cargo.toml` 의 `[package.metadata.deb]` / `[package.metadata.generate-rpm]`.
- **Windows** MSI 는 `cargo-wix` + `wix/main.wxs`. **UpgradeCode GUID 는 절대 변경 금지** — 바뀌면 새 제품으로 인식되어 구버전과 공존.
- CI: `.github/workflows/release.yml` (self-hosted runner, Linux x64/arm64 라벨 분기, Windows). `workflow_dispatch` 로 태그 없는 수동 검증 빌드 가능.

현재 머신에 바로 설치하려면 `just install` — 본체+플러그인을 dist 빌드해 현재 OS 에 설치한다 (자동 감지). macOS 는 `scripts/install-macos.sh` 가 `build-macos-dmg.sh` 를 `NO_DMG=1` 로 재사용해 `dist/Tasty.app` 만 조립한 뒤 `/Applications/Tasty.app` 을 덮어쓴다. 번들 플러그인은 앱 첫 실행 시 `~/.tasty/plugins` 로 강제 동기화된다. (Linux/Windows 자동 설치는 미구현 — `just dist-linux`/`dist-windows` 산출물로 수동 설치.)

## 빌드 시간 진단

```bash
cargo build --release --timings   # target/cargo-timings/*.html — 크레이트별 frontend/codegen
cargo machete                     # 미사용 의존성 (컴파일 시간 낭비)
cargo modules / cargo depgraph    # 모듈/크레이트 의존 그래프 (크레이트 분리 검토용)
```

한 크레이트가 전체 빌드의 50%+ 를 잡으면 너무 크다는 신호 — 모듈 분리를 고려한다.

## 크레이트 분리 가이드

본 바이너리의 큰 leaf 모듈을 떼어낼 때 후보 조건: **out-degree 작음**(다른 src/ 모듈 거의 미참조) · **사이클 없음** · **충분히 큼**(1000줄+). 절차: `crates/tasty-<name>/` 생성 → `git mv` → 내부 경로 갱신 → 본 `Cargo.toml` 의존 추가 → `pub use tasty_<name> as <name>` 재수출(backward path 유지) → `cargo check`/`build` 검증. 기존 `crate::model::Foo` 경로가 그대로 동작하는 게 핵심이라 reverse import 갈아끼우기가 불필요하다.

**세 조건 중 크기는 보조 지표다** — 소비자가 둘 이상이고, 합칠 후보 크레이트에 *그 크레이트가 원래 몰라도 되는 의존* 을 들이게 되는 코드는 1000줄에 못 미쳐도 분리한다(판정은 의존 방향이 우선, [ADR-0089](../adr/0089-crate-split-follows-dependency-direction.md)).

**재수출 형태는 둘 중 하나를 고른다.**

| 형태 | 쓰는 곳 | 이유 |
|------|---------|------|
| `pub use tasty_<name>::*;`(glob) | 본체가 그 크레이트를 **통째로 소유**하고 경계를 그을 이유가 없는 shim (`src/model.rs`, `src/adapters/ui/icons.rs`) | 표면 전체가 본체 것이라 좁힐 대상이 없다 |
| `pub use tasty_<name>::{A, B, C};`(명시 목록) | 본체가 **일부만 써야 하는** 크레이트 (`src/adapters/cli.rs`, `src/adapters/ipc.rs`) | glob 이면 계층 위반이 컴파일 에러로 안 잡힌다 — 본체 어디서든 재수출 경로로 크레이트 전체에 닿는다 |

판단 기준은 "본체가 이 크레이트의 아무 심볼이나 써도 되는가" 하나다. 아니라면 명시 목록을 쓰고, 목록 밖 심볼을 쓰려는 시도가 컴파일 에러가 되게 둔다. 소스 스캔 가드(`tests/layering.rs`)는 재수출을 우회한 직접 참조를 막는 **2차 방어**이지, 재수출 형태를 좁히는 것의 대체재가 아니다.

### 의존 방향 규칙 — 본체는 `tasty-cli` 를 참조하지 않는다

`tasty-cli` 는 **바이너리의 진입 계층**(인자 파싱 + 그 파싱 결과로 실행되는 커맨드 구현)이다. GUI 런타임·IPC 핸들러·앱 상태(`src/`)가 그 크레이트 내부를 직접 들여다보면 의존 방향이 뒤집혀, CLI 쪽 타입 변경이 GUI 를 깨고 GUI 재사용 목적의 로직이 CLI 안에 눌러앉는다. 양쪽이 함께 쓰는 코어(ssh · remote browse · stream 등)는 CLI 가 아니라 **별도 크레이트**에 두고 본체는 그쪽을 참조한다.

`src/adapters/cli.rs` 는 boot 진입점 7개(`Cli` · `Commands` · `run_client` · `try_run_plugin_cli` · `print_augmented_help` · `print_command_tree` · `format_parse_error`)만 이름으로 재수출한다. 그 밖의 CLI 심볼은 본체에서 경로 자체가 존재하지 않는다. 재수출 밖의 경로(`tasty_cli::` 직접 참조)는 **`tests/layering.rs` 가 `cargo test --workspace`(CI)로 막는다.** 목록 두 개의 성격이 다르다:

| 상수 | 성격 | 내용 |
|------|------|------|
| `ALLOWED_PATHS` | 영구 허용 | `src/main.rs` · `src/boot.rs` · `src/boot/` · `src/adapters/cli.rs` — 바이너리가 CLI 파서를 소유하는 정당한 의존 |
| `BASELINE_FILES` | 한시 허용 | 이행 중인 위반의 스냅샷. **현재 비어 있다** — 본체는 `tasty-ssh` / `tasty-remote` / `tasty_ipc::client` 를 직접 참조한다. **줄어들기만 한다**(새 항목 추가 금지, 참조를 걷어냈으면 목록에서도 지워야 통과 — 역방향 검사) |

주석 안의 언급도 위반으로 잡는다 — 주석이 옛 경로를 가리키면 그것도 실제 오정보이므로 코드와 함께 갱신한다.
