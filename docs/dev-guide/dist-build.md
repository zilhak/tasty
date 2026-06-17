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

산출물: `dist/Tasty.app/...`(`CFBundleVersion` = Cargo version) · `dist/Tasty-{version}-macos.dmg`. `build-macos-dmg.sh` 마지막에 자동 sanity check(`tasty --version` / Mach-O / `CFBundleVersion` 일치 / DMG 존재) — 실패 시 빌드 fail. `dist` 는 `release` 상속(`strip=true`)이라 `nm` 이 거의 빈 건 정상.

**서명/공증은 범위 밖** — dist 산출물은 미서명(`codesign --display`/`spctl -a` 로 확인, Gatekeeper rejected 가 정상). 사용자는 Finder 우클릭→열기로 우회. universal binary(arm64+x86_64)도 별도 작업(`--target x86_64-apple-darwin` + `lipo`).

## Windows

> Darwin 작성 환경에서 직접 실행 불가 — Windows 머신에서 빌드 후 결과 반영.

```powershell
cargo install cargo-wix; winget install WiXToolset.WiXToolset   # 1회
.\scripts\build-windows.ps1            # dist + ZIP + MSI
.\scripts\build-windows.ps1 -SkipMsi   # ZIP 만
```

산출물: `tasty-{v}-windows-x64.{zip,msi}` + `SHA256SUMS-windows.txt`. `build-windows.ps1` 이 MSI 단계에서 `$env:WIX\bin` 을 자동 PATH prepend. 자동 sanity check(ZIP 풀어 `tasty.exe --version`, MSI 존재). 검증 포인트: MSI UpgradeCode 유지(`wix/main.wxs`), 설치→시작메뉴→제거.

## Linux

```bash
just dist-setup-linux              # 또는 수동 (아래)
./scripts/build-linux.sh           # uname -m 으로 x64/arm64 자동 감지
```

수동 사전 도구: `sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev` + `cargo install cargo-deb cargo-generate-rpm` + `linuxdeploy`(GitHub continuous, `~/.local/bin`). 도구 역할은 [build](build.md) Linux 섹션.

산출물: `tar.gz` · `.deb` · `.rpm` · `.AppImage` + `SHA256SUMS-linux-{x64|arm64}.txt`. 자동 sanity check(tar.gz `tasty --version`, `dpkg-deb -I`, `rpm -qpi`, AppImage ELF 확인 — 실행은 안 함, GUI hang 회피).

## 산출물 요약

| 플랫폼 | 명령 | 산출물 |
|--------|------|--------|
| macOS (arm64) | `./scripts/build-macos-dmg.sh` | `Tasty-{v}-macos.dmg` (~18MB) |
| Windows (x64) | `.\scripts\build-windows.ps1` | `{zip,msi}` |
| Linux | `./scripts/build-linux.sh` | `{tar.gz,deb,rpm,AppImage}` (~83MB AppImage) |

## 관련

- [build](build.md) — 프로필 정의 · [release](release.md) — 릴리스 워크플로 · [release-runners](release-runners.md) — self-hosted runner
