# dist 빌드 명령 카탈로그

[release](release.md) 는 *태그 push → GitHub Actions* 워크플로, 본 문서는 **로컬에서 dist 산출물을 빌드** 하는 명령 카탈로그. 빌드 프로필 정의(LTO/strip)는 [build](build.md) "빌드 프로필".

## 빠른 시작 (Justfile)

```bash
just dist            # 호스트 OS dist 빌드 (자동 sanity check + SHA256SUMS)
just dist-clean      # dist/ 정리
just dist-verify     # SHA256SUMS 재검증
just dist-macos | dist-linux | dist-windows   # 플랫폼 명시
just dist-setup-linux                          # Linux 사전 도구 (1회)
```

`just` 미설치: `cargo install just`. (Windows 일반 PowerShell 은 자동 감지가 안 될 수 있어 `just dist-windows` 명시 권장.)

## 공통 사전 조건

- `Cargo.toml::version` 이 의도한 릴리스 버전인지 확인(검증 빌드엔 버전 안 올림 — [release](release.md) bump 절차).
- 모든 빌드 스크립트는 인자 없으면 `--profile dist` 기본.
- 산출물은 `dist/` 에 누적(동일 버전 재빌드는 silent overwrite).

## macOS

도구(`hdiutil`/`codesign`/`xcrun`)는 Xcode CLT 포함.

```bash
cargo build --profile dist        # 워크스페이스 컴파일
./scripts/build-macos-dmg.sh      # .app 번들 + .dmg
```

산출물: `dist/Tasty.app/...`(`CFBundleVersion` = Cargo version) · `dist/Tasty-{version}-macos.dmg`. `build-macos-dmg.sh` 마지막에 자동 sanity check(`tasty --version` / Mach-O / `CFBundleVersion` 일치 / DMG 존재) — 실패 시 빌드 fail. 고지 세트는 `Contents/Resources/` 에 codesign **전에** 스테이징되고(`scripts/lib/notice-set.sh`), `.app` · DMG 스테이징 트리 · 만든 DMG 를 읽기 전용으로 붙인 트리 세 곳에서 저장소 사본과 바이트 대조한다. 이 확인은 배선이며 macOS 빌더에서 돈 적은 아직 없다(미측정). `dist` 는 `release` 상속(`strip=true`)이라 `nm` 이 거의 빈 건 정상.

**서명은 ad-hoc, 공증은 범위 밖** — `build-macos-dmg.sh` 가 `codesign --sign -` 로 ad-hoc 서명한다(Apple Silicon 의 "손상됨" 하드 블록 완화). 인증서 서명이 아니라 Gatekeeper 는 여전히 rejected 이므로(`spctl -a` 로 확인) 사용자는 Finder 우클릭→열기로 우회. 번들 plugin 은 `Contents/Resources/plugins/` 에 staging 해야 서명이 통과한다 — `Contents/MacOS/` 하위면 codesign 이 그 디렉터리를 nested bundle 로 파싱하려다 실패한다([build.md](build.md#배포-패키징)). 산출물은 **Apple Silicon(arm64) 전용**이다 — dist 는 full LTO 라 타깃을 하나 더 얹으면 빌드 시간이 배로 늘고, Intel Mac 은 macOS 26 이 마지막 지원 릴리스라 배포 대상에서 뺐다. Intel 에서 쓰려면 `--target x86_64-apple-darwin` 으로 직접 빌드한다.

## Windows

> Darwin 작성 환경에서 직접 실행 불가 — Windows 머신에서 빌드 후 결과 반영.

```powershell
cargo install cargo-wix; winget install WiXToolset.WiXToolset   # 1회
.\scripts\build-windows.ps1            # dist + ZIP + MSI
.\scripts\build-windows.ps1 -SkipMsi   # ZIP 만
```

산출물: `tasty-{v}-windows-x64.{zip,msi}` + `SHA256SUMS-windows.txt`. `build-windows.ps1` 이 MSI 단계에서 `$env:WIX\bin` 을 자동 PATH prepend. 자동 sanity check(ZIP 풀어 `tasty.exe --version`, MSI 존재). 고지 세트는 ZIP 최상단과 MSI 설치 디렉토리에 들어가고, 스크립트가 ZIP 을 푼 트리와 MSI 를 관리 설치(`msiexec /a`)로 푼 트리에서 저장소 사본과 바이트 대조한다. `wix/main.wxs` 는 파일마다 이름을 적어야 해서, MSI 빌드 전에 `LICENSES/` 의 파일마다 대응 `Source` 가 있는지 먼저 본다. 이 확인들은 배선이며 Windows 빌더에서 돈 적은 아직 없다(미측정). 검증 포인트: MSI UpgradeCode 유지(`wix/main.wxs`), 설치→시작메뉴→제거.

## Linux

```bash
just dist-setup-linux              # 또는 수동 (아래)
./scripts/build-linux.sh           # uname -m 으로 x64/arm64 자동 감지
```

수동 사전 도구: `sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev` + `cargo install cargo-deb cargo-generate-rpm` + `linuxdeploy`(GitHub continuous, `~/.local/bin`). 도구 역할은 [build](build.md) Linux 섹션.

산출물: `tar.gz` · `.deb` · `.rpm` · `.AppImage` + `SHA256SUMS-linux-{x64|arm64}.txt`. 자동 sanity check(tar.gz `tasty --version`, `dpkg-deb -I`, `rpm -qpi`, AppImage ELF 확인 — 실행은 안 함, GUI hang 회피).

넷 다 고지 세트(`LICENSE` · `THIRD_PARTY_LICENSES.md` · `LICENSES/` 의 모든 파일)를 함께 나른다 — 생성 단계는 없고 저장소의 파일을 그대로 스테이징한다. `LICENSES/` 는 파일 이름이 아니라 디렉토리로 읽는다(`scripts/lib/notice-set.sh` 의 `notice_set_files`, deb/rpm 은 `Cargo.toml` asset 의 glob). 자리는 산출물마다 다르고(`tar.gz` 는 최상단, deb 은 `/usr/share/doc/tasty/`, rpm 과 AppImage 는 `usr/share/licenses/tasty/`) 정본 표는 [`THIRD_PARTY_LICENSES.md`](../../THIRD_PARTY_LICENSES.md) 에 있다. sanity check 가 tar.gz · deb · rpm 리스팅과 AppImage 의 AppDir 에서 세트의 파일 **전부**를 함께 보는데, **rpm 쪽 확인은 `rpm` 명령이 있는 빌더에서만 돈다** — 없으면 그 갈래는 통과가 아니라 미측정이다.

## 산출물 요약

| 플랫폼 | 명령 | 산출물 |
|--------|------|--------|
| macOS (arm64) | `./scripts/build-macos-dmg.sh` | `Tasty-{v}-macos.dmg` (~18MB) |
| Windows (x64) | `.\scripts\build-windows.ps1` | `{zip,msi}` |
| Linux | `./scripts/build-linux.sh` | `{tar.gz,deb,rpm,AppImage}` (~83MB AppImage) |

## 관련

- [build](build.md) — 프로필 정의 · [release](release.md) — 릴리스 워크플로 · [release-runners](release-runners.md) — self-hosted runner
