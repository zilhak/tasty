# 릴리스 절차

릴리스는 **Git 태그 push** 로 트리거된다(`.github/workflows/release.yml`). 버전 형식은 `MAJOR.MINOR.PATCH`. 버전 정책은 [`../../CLAUDE.md`](../../CLAUDE.md) "버전 정책" — 이 문서는 절차다.

## 버전 자동 bump 규칙 (요약)

CLAUDE.md 정책의 운영 형태:

- **본체** (`Cargo.toml` 루트): 사용자가 *빌드를 요청* 했고, 마지막 빌드 이후 새 커밋이 있으며, 사용자가 막지 않았으면 **patch +1**. AI 자체 검증 빌드(`cargo build`/`test`)는 올리지 않는다.
- **Plugin** (`crates/tasty-plugin-*/Cargo.toml`): 그 plugin 의 빌드 산출물이 달라지면(판정 기준은 staged 파일이 아니라 의존 폐포의 내용 — 루트 CLAUDE.md 의 plugin 버전 정책) **patch +1 을 같은 커밋에** 포함. 여러 plugin 변경 시 각각 독립 적용. 본체 규칙과 독립.
- **Plugin 매니페스트 lockstep** (`crates/tasty-plugin-*/tasty-plugin.toml`): 위 patch +1 과 함께 매니페스트 `version` 을 **동일 값**으로 맞춰 같은 커밋에 포함(`.sig` 는 `.gitignore` 된 빌드 산출물이라 커밋하지 않는다). Cargo.toml 만 올리면 `plugin.list`·업그레이드 판정이 노출·비교하는 매니페스트 version 이 어긋난다(version drift). `crates/tasty-doc-guards/tests/plugin_manifest_version_parity.rs` 가 정합을 강제한다(채널은 [ci-gates](ci-gates.md)). 자동 잡은 push 된 커밋만 보므로 커밋 전에는 직접 돌린다.
- **minor / major**: 사용자가 직접 지정. AI 가 임의로 올리지 않는다.

본체 patch bump 절차: `Cargo.toml` patch +1 → `cargo build`(Cargo.lock 갱신) → `README.md`·`README.ko.md` 의 Version 배지(`badge/version-X.Y.Z-blue`)를 같은 값으로 갱신 → `Cargo.toml` + `Cargo.lock` + 두 README 를 **함께** 커밋(`chore: bump version to X.Y.Z`) → 아래 릴리스 절차로 이어감. 배지를 빠뜨리면 `crates/tasty-doc-guards/tests/readme_badge_parity.rs` 가 실패시킨다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌리므로([ci-gates](ci-gates.md)) 빠뜨린 bump 는 CI 에서 잡힌다. 다만 자동 잡은 push 된 커밋만 보니 **커밋 전에 직접 돌리면 그 자리에서 잡힌다** — 릴리스뿐 아니라 이 자동 patch +1 커밋에도 적용된다.

## 릴리스 단계

### 1. 버전 + CHANGELOG

1. `Cargo.toml`(루트) `version` 갱신, `cargo build` 로 `Cargo.lock` 갱신.
2. `CHANGELOG.md` 의 `## [Unreleased]` 를 새 버전 헤더로 promote 하고, 빈 `[Unreleased]` 를 다시 추가한다 (`tests/changelog_unreleased.rs` 가 빈 절 존재를 강제).
   ```markdown
   ## [Unreleased]

   ## [X.Y.Z] - YYYY-MM-DD
   ### Fixed
   - `fix(...)`: ...
   ```
3. `README.md`·`README.ko.md` 의 Version 배지(`img.shields.io/badge/version-X.Y.Z-blue`, `CHANGELOG.md` 로 링크)를 `Cargo.toml` 과 같은 값으로 갱신한다. shields.io static badge 라 URL 에 값이 박혀 있어 어디서도 파생되지 않는다 — 이 단계가 빠지면 배지와 `Cargo.toml`의 버전이 달라진다. 배지 변경은 §3 의 bump 커밋에 함께 넣는다. **`crates/tasty-doc-guards/tests/readme_badge_parity.rs` 가 이 정합을 강제하고, `doc-guards.yml` 이 main push · PR 마다 그것을 돌린다**([ci-gates](ci-gates.md)). **다만 자동 잡은 push 된 커밋만 본다 — 배지를 빠뜨린 bump 커밋은 push 전까지 통과한 것처럼 보이므로 아래 로컬 확인을 거르지 마라.** 로컬 확인:
   ```bash
   cargo test -p tasty-doc-guards --test readme_badge_parity
   ```
4. schema 변경이 있었다면 `crates/tasty-plugin-protocol/CHANGELOG.md` 도 동일 처리. break/deprecation 분류는 [api-conventions](api-conventions.md) "안정성 정책".

### 2. Plugin 매니페스트 서명

`crates/tasty-plugin-*/tasty-plugin.toml` 가 이번 사이클에 하나라도 변경됐으면(또는 첫 release 빌드면) 서명을 갱신한다.

```bash
./scripts/sign-bundle.sh --key ~/.tasty-keys/dev.pem --all-builtins
```

생성/갱신되는 `*.toml.sig`(번들 plugin 매니페스트 전부 — `--all-builtins` 가 `crates/tasty-plugin-*/tasty-plugin.toml` 을 자동 검색한다)는 **`.gitignore` 로 제외된 빌드 산출물**이라 커밋되지 않는다 — 로컬 release 빌드·dev 검증용이며, CI 정식 release는 각 러너가 선택한 로컬 키로 재서명한다. 키가 없으면 생성하고 기존 개인키는 재사용한다. dev/debug 빌드는 서명을 검증하지 않는다. 따라서 매니페스트 version bump 시 커밋되는 건 `tasty-plugin.toml` 자체뿐이고 `.sig` 재생성은 커밋 절차 밖이다. 알고리즘·키 보관은 [plugin-packaging](plugin-packaging.md).

### 3. 커밋 — body 가 곧 릴리스 노트

`Cargo.toml` + `Cargo.lock` 을 함께 커밋한다. **커밋 body에 변경 내용을 영어 평문으로 적는다** — 워크플로가 `git log -1 --format=%b` 로 추출해 GitHub Release 노트로 쓴다.

```
chore: bump version to X.Y.Z

Add the new feature and describe its user-visible behavior.

Fix the reported issue and describe the corrected behavior.
```

이전 태그 이후 커밋: `git log v<이전>..HEAD --oneline`.

### 4. 태그 + push

```bash
git tag vX.Y.Z          # 'v' 접두사 + Cargo.toml 버전과 정확히 일치 (불일치 시 워크플로 fail)
git push origin main --tags
```

### 5. 워크플로 (release.yml)

1. **create-release** — 버전 검증 → draft release 생성(body = 릴리스 노트).
2. **build-macos / build-windows / build-linux-x64 / build-linux-arm64** — 각 빌드 스크립트가 로컬 키를 준비해(Linux·macOS 는 `ensure-sign-key.sh`, Windows 는 `build-windows.ps1` 이 직접) plugin 재서명 → 빌드(`--profile dist`) → 아티팩트 업로드. GitHub Secret 관여 없음(배경은 [plugin-packaging](plugin-packaging.md) "영구 release 키를 두지 않는 이유").
3. **publish-release** — draft 해제(공개).

### 6. 검증

GitHub Releases 에서 노트 + 플랫폼별 아티팩트 확인:

- macOS `*.dmg` / Windows `*.zip`·`*.msi` / Linux x64·arm64 각 `.tar.gz`·`.deb`·`.rpm`·`.AppImage`
- `SHA256SUMS-{macos,windows,linux-x64,linux-arm64}.txt` 4종 (다운로드 무결성 수동 검증용)

> **사용자 업그레이드**: publish 되면 사용자는 GitHub Releases 에서 새 아티팩트를 직접 내려받아 SHA256SUMS 로 검증한 뒤 수동 설치한다.

## 러너

릴리스 빌드는 GitHub Actions **self-hosted runner** 에서 동작한다. 워크플로는 `.github/workflows/release.yml`, 빌드 스크립트는 `scripts/build-{linux,macos-dmg,windows}.{sh,ps1}`.

### 러너 인벤토리

| 항목 | x86_64 | aarch64 |
|------|--------|---------|
| 호스트 | `server` (192.168.0.16) | `gx10` (192.168.0.13) |
| Runner 이름 | `tasty-server-x64` | `tasty-gx10-arm64` |
| 라벨 | `self-hosted, Linux, X64` | `self-hosted, Linux, ARM64` |
| 설치 경로 | `/home/zilhak/actions-runner/` | 〃 |
| systemd 서비스 | `actions.runner.zilhak-tasty.tasty-server-x64.service` | `…tasty-gx10-arm64.service` |
| 자동 시작 | enabled | enabled |

macOS/Windows 러너도 동일 패턴(라벨만 `[self-hosted, macOS]` / `[self-hosted, Windows]`).

같은 mac/win 러너를 `.github/workflows/crossplatform-check.yml` 이 재사용한다 — dist 빌드 없이 컴파일 정합성만 확인하는 가벼운 가드다. **언제 도는가**: `main` 에 push 될 때(문서·사이트·마크다운만 바뀐 push 는 제외) · `main` 대상 PR · 수동 dispatch. 이 저장소는 PR 없이 main 에 직접 push 하는 흐름이라 push 가 실효 트리거이고, PR 트리거는 PR 흐름을 쓰게 될 때를 위해 남아 있다(선택 근거는 워크플로 파일 상단 주석). **무엇을 도는가**: 잡은 넷(`check-macos` · `check-windows` · `check-headless` · `check-release`)이고 잡별 명령은 [ci-gates](ci-gates.md) 의 표가 정본이다. 네이티브 host 타깃이 곧 `x86_64-pc-windows-msvc` / `aarch64-apple-darwin` 이라 `--target` 지정은 불필요. 잡들이 병렬로 돌고 같은 ref 의 앞선 실행은 취소되므로 같은 ref의 중복 실행을 줄인다. (무거운 dist 빌드 검증은 여전히 수동 `build-check.yml`.)

### 1회 도구 설치 (러너 추가 / 새 도구 의존성 시)

각 OS 빌드 스크립트가 시작 시 도구를 검사하고 미설치면 명시적 에러로 안내한다.

```bash
# Linux (x64/arm64 공통)
sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev
cargo install cargo-deb cargo-generate-rpm
mkdir -p ~/.local/bin
curl -fsSL -o ~/.local/bin/linuxdeploy \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage
chmod +x ~/.local/bin/linuxdeploy
which cargo-deb cargo-generate-rpm linuxdeploy   # 검증
```

```powershell
# Windows
cargo install cargo-wix; winget install WiXToolset.WiXToolset
```

```bash
# macOS
xcode-select --install; brew install create-dmg
```

WiX winget 은 `WIX` env 만 등록(`%WIX%\bin` PATH 추가 안 함) — `build-windows.ps1` 이 자동 prepend. 단 러너 서비스가 새 env 를 못 보면 1회 재시작.

### CI PATH 매핑

`release.yml` Linux 잡:

```yaml
- name: Setup PATH
  run: |
    echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"   # cargo, cargo-deb, cargo-generate-rpm
    echo "$HOME/.local/bin" >> "$GITHUB_PATH"   # linuxdeploy
```

cargo install 도구는 PATH 변경 불필요. 별도 다운로드 도구는 `~/.local/bin` + 동일 설정.

### 운영 명령

```bash
ssh server 'systemctl status actions.runner.zilhak-tasty.tasty-server-x64.service'
sudo systemctl {restart|stop|start} <service-name>
sudo journalctl -u <service-name> -f
gh api repos/zilhak/tasty/actions/runners        # GitHub 측 등록 상태

# 등록 해제 (머신 정리): stop+disable 후
cd ~/actions-runner && ./config.sh remove --token <REMOVAL_TOKEN>   # token: Settings→Runners→Remove
```

### 트러블슈팅

| 증상 | 해결 |
|------|------|
| `cargo-deb`/`cargo-generate-rpm`/`linuxdeploy` not found | 1회 설치 누락 + `release.yml` Setup PATH 점검 |
| `cmake`/`freetype`/`fontconfig` 빠짐 | `build-linux.sh` 안내대로 `apt install` |
| 러너 offline | `systemctl status` → `restart` |
| release upload 실패 | `GH_TOKEN` 권한/tag 이름 확인 (`--clobber` 재업로드) |
| Windows `candle could not be found` | WiX 3.x 미설치/`WIX` env 누락 → `winget install` 후 서비스 재시작 |
| AppImage `fuse: failed to exec fusermount` | 타깃 환경 FUSE 미지원 → `--appimage-extract-and-run` |

## API 안정성 가드

외부 API의 추가·제거는 명시한 호환성·deprecation 규칙과 예외를 따른다 (메서드 baseline 등 `cargo test --workspace` 강제). 분류는 [api-conventions](api-conventions.md).

## 관련

- [commit-convention.md](commit-convention.md) — bump 커밋 형식
- [build.md](build.md) — `--profile dist` 배포 빌드·패키징 스크립트

## 배포 범위와 번들 서명

Tasty 본체는 새 버전을 확인·다운로드·설치하지 않는다.
배포 패키지와 릴리스 페이지를 통해 갱신한다.

서명 키는 `SIGN_KEY_PATH`, 기존 `~/.tasty-keys/release.pem`, 로컬 dev 키 순서로 선택한다.
dev 키는 없을 때 생성하며 기존 개인키는 재사용한다. 공개키는 그 키에서 다시 도출해
빌드에 포함한다. 따라서 '매 빌드 새 키'나 '유출 영향이 빌드 한 번뿐'이라고 설명하지 않는다.
서명 전에 공개키를 준비하고 그 뒤 앱을 빌드해야 번들과 앱의 신뢰 키가 맞는다.
GitHub Secret을 필수로 요구하지 않으며 `.sig`와 로컬 키 산출물은 커밋하지 않는다.

## 플러그인 버전 비교

`check-plugin-version-bump.sh`는 매니페스트를 가진 번들 플러그인의 실제 내용 변경을 검사한다.
대상은 해당 디렉터리와 normal/build 의존성이다. dev 의존성은 제외한다.
Cargo가 보고하는 저장소 안 path 의존성은 workspace 밖에 있어도 포함한다.
`vendor` 같은 디렉터리 이름을 따로 목록으로 관리하지 않는다.

- 인라인 `cfg(test)`와 테스트 선언으로만 도달하는 파일은 제품 비교에서 제외한다.
- test를 요구하는 `cfg_attr`은 조건부 속성 줄만 제외한다. 항목 본문까지 지우지 않는다.
- `not(test)`나 test 밖에서도 참인 `any(test, ...)`는 테스트 전용으로 보지 않는다.
- Rust 내용은 rustfmt로 정규화하고, 실패하면 원문을 비교한다.
- Cargo.toml과 매니페스트의 비교 대상 버전 선언은 증거에서 제외한다.
- 주석·일부 선언 재배치는 산출물이 같아도 버전 증가를 요구할 수 있다.

보조 도구가 없거나 오래되면 테스트 전용 범위를 제외하지 못해 더 넓게 비교하고 안내를 남긴다.
다시 빌드한 뒤 재검사한다. 신선도는 파일 시각이 아닌 소스 내용 지문으로 확인한다.
보조 도구를 수정했다면 파일 SLOC·예외 총합·플러그인 버전 검사를 소비자로 함께 확인한다.

여러 작업을 나누어 push하면 이전 작업이 이미 발행한 버전보다 새 버전을 사용해야 한다.
pre-commit의 비교 기준과 실제 push 직전 원격 끝점은 다를 수 있다.
pre-push는 Git이 전달한 원격·로컬 SHA를 비교한다. 원격 객체가 없으면 fetch 후 다시 검사한다.
새 ref 생성과 삭제처럼 비교할 이전 버전이 없는 경우는 건너뛴 이유와 개수를 따로 보고한다.

Cargo.toml과 매니페스트 버전은 함께 맞추고 Cargo.lock도 갱신해 같은 커밋에 넣는다.
현재 설치본 반영 방법은 [플러그인 개발](plugin-development.md)에 있다.
같은 버전에서 내용이 달라 재동기화될 수 있다는 사실이 배포 버전 관리 의무를 없애지는 않는다.

남는 제한은 루트의 patch 선언이나 Cargo.lock만으로 바뀐 외부 의존 그래프,
그리고 Windows 셸의 경로 표기다. 현재 검사가 모든 의존 변화와 모든 셸을 보장한다고 쓰지 않는다.
CI 경로 필터도 검사 대상보다 넓어야 하며 assets 아래 Markdown을 일반 문서처럼 제외하지 않는다.

## 배포물의 고지 파일

고지 세트는 `LICENSE`, `THIRD_PARTY_LICENSES.md`, `LICENSES/`의 모든 파일이다.
릴리스 때 생성하지 않고 저장소 파일을 복사한다. Rust 의존 목록 생성기만으로는 별도로
벤더링한 폰트·JS·CSS 자산을 찾을 수 없기 때문이다.
파일의 출처와 산출물별 위치는 [제3자 고지](../../THIRD_PARTY_LICENSES.md)가 관리한다.

macOS는 앱의 Resources에 codesign 전에 넣는다. DMG에서 앱을 설치한 뒤에도 남아야 한다.
ZIP은 실행 파일 옆, MSI는 설치 디렉터리에 두며 LICENSES 하위 경로를 유지한다.
MSI 동의 화면의 RTF는 전체 고지 세트를 대신하지 않는다.
셸과 PowerShell 스테이징은 디렉터리를 읽고, 정적 WiX 목록은 빌드 전에 실제 파일과 비교한다.

확인은 패킹 전 트리와 최종 산출물을 구분한다. DMG는 읽기 전용 마운트,
ZIP은 압축 해제, MSI는 관리 설치 등으로 파일을 꺼내 저장소 사본과 바이트를 비교한다.
스크립트에 확인 코드가 있다는 것과 해당 플랫폼에서 실행해 통과했다는 것은 따로 보고한다.
다른 플랫폼에서도 스테이징은 구현하되 실제로 열어 보지 않았다면 미확인이라고 쓴다.

이 고지 세트가 모든 정적 링크 Rust 의존성의 고지를 완성하는 것은 아니다.
라이선스 허용 정책 검사와 배포물 고지 생성은 서로 다른 일이다.
새 자산에는 고지 파일·인벤토리·필요한 WiX 항목을 함께 갱신한다.
