# 릴리스 러너 설정

Tasty의 릴리스 빌드는 GitHub Actions self-hosted runner에서 동작한다. 이 문서는 러너
인벤토리, 새 러너 추가/기존 러너 점검 시 1회 도구 설치, 운영 명령을 정리한다.

워크플로 정의는 `.github/workflows/release.yml`, 빌드 스크립트는
`scripts/build-{linux,macos-dmg,windows}.{sh,ps1}`을 참조.

## 러너 인벤토리

| 항목 | 16번 (x86_64) | 13번 (aarch64) |
|------|---------------|----------------|
| 호스트 | `server` (192.168.0.16) | `gx10` (192.168.0.13) |
| SSH | `ssh server` | (로컬) |
| Runner 이름 | `tasty-server-x64` | `tasty-gx10-arm64` |
| 라벨 | `self-hosted, Linux, X64` | `self-hosted, Linux, ARM64` |
| 설치 경로 | `/home/zilhak/actions-runner/` | `/home/zilhak/actions-runner/` |
| 실행 유저 | `zilhak` | `zilhak` |
| systemd 서비스 | `actions.runner.zilhak-tasty.tasty-server-x64.service` | `actions.runner.zilhak-tasty.tasty-gx10-arm64.service` |
| systemd unit | `/etc/systemd/system/<service-name>` | `/etc/systemd/system/<service-name>` |
| 자동 시작 | enabled (부팅 시 자동) | enabled |

macOS / Windows 러너도 동일 패턴으로 등록되어 있다 (라벨만 다름:
`[self-hosted, macOS]`, `[self-hosted, Windows]`).

## 1회 도구 설치 (러너 추가 시 또는 신규 도구 의존성 추가 시)

각 OS별 빌드 스크립트가 시작 시 의존 도구를 검사하고, 미설치 시 명시적 에러로 안내한다.
새 러너를 등록한 직후, 또는 빌드 의존성이 늘어났을 때 다음을 zilhak 유저로 한 번 실행한다.

### Linux 러너 (x64 / arm64 공통)

`build-linux.sh`가 요구하는 도구는 다섯 가지다.

```bash
# (1) 시스템 빌드 의존성 (apt 기준 — Fedora/Arch는 패키지명만 다름)
sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev

# (2) Rust 툴체인이 이미 zilhak 홈에 설치되어 있다고 가정 (~/.cargo/bin/cargo)
#     없으면: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# (3) .deb 패키지 생성용
cargo install cargo-deb

# (4) .rpm 패키지 생성용
cargo install cargo-generate-rpm

# (5) .AppImage 생성용 — cargo로 받을 수 없으므로 GitHub continuous 릴리스에서 직접
mkdir -p ~/.local/bin
curl -fsSL -o ~/.local/bin/linuxdeploy \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage
chmod +x ~/.local/bin/linuxdeploy
```

설치 후 검증:

```bash
which cargo-deb cargo-generate-rpm linuxdeploy
cargo-deb --version            # cargo-deb 3.7.0 또는 호환 버전
cargo-generate-rpm --version   # cargo-generate-rpm 0.20.0 또는 호환 버전
linuxdeploy --version          # linuxdeploy version 1-alpha (...)
```

### Windows 러너

`build-windows.ps1`은 `cargo-wix` + WiX Toolset 3.x를 요구한다. 자세한 내용은
[`build.md`](build.md)의 "Windows MSI" 절 참조.

```powershell
cargo install cargo-wix
winget install WiXToolset.WiXToolset    # 관리자 권한
```

WiX winget 패키지는 `WIX` 환경변수만 등록하고 **`%WIX%\bin`을 PATH에 추가하지
않는다**. `build-windows.ps1`이 MSI 단계 시작 시 `$env:WIX\bin`을 현재
프로세스 PATH에 자동으로 prepend하므로 머신 PATH 수정이나 러너 서비스
재시작은 필요 없다. 단 `WIX` 환경변수가 보여야 하므로 winget 설치 후 러너
서비스가 새 환경변수를 인식하지 못하면 한 번 재시작:

```powershell
Restart-Service "actions.runner.zilhak-tasty.tasty-win-runner"
```

### macOS 러너

`build-macos-dmg.sh`는 Xcode Command Line Tools와 `create-dmg` (Homebrew)를 요구한다.

```bash
xcode-select --install
brew install create-dmg
```

## CI 워크플로 PATH 매핑

`release.yml`의 Linux 빌드 잡은 PATH를 명시적으로 추가한다.

```yaml
- name: Setup PATH
  run: |
    echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
    echo "$HOME/.local/bin" >> "$GITHUB_PATH"
```

각 디렉토리에 들어가는 도구:

| 디렉토리 | 도구 |
|---------|------|
| `~/.cargo/bin/` | `cargo`, `cargo-deb`, `cargo-generate-rpm` (cargo install로 설치되는 모든 것) |
| `~/.local/bin/` | `linuxdeploy` (직접 다운로드하는 도구) |

새 도구가 cargo install로 설치된다면 PATH 변경 불필요. 새 도구가 별도 다운로드 형태라면
`~/.local/bin/`에 두고 동일한 PATH 설정을 유지한다.

## 운영 명령

### 상태 확인

```bash
# 16번 (server, SSH)
ssh server 'systemctl status actions.runner.zilhak-tasty.tasty-server-x64.service'

# 13번 (로컬)
sudo systemctl status actions.runner.zilhak-tasty.tasty-gx10-arm64.service
```

### 재시작 / 중지 / 로그

```bash
sudo systemctl restart <service-name>
sudo systemctl stop <service-name>
sudo systemctl start <service-name>
sudo journalctl -u <service-name> -f          # 실시간 로그
sudo journalctl -u <service-name> --since "1h ago"
```

### GitHub 측 등록 상태

- 웹 UI: https://github.com/zilhak/tasty/settings/actions/runners
- CLI: `gh api repos/zilhak/tasty/actions/runners`

### 등록 해제 (머신 정리 시)

```bash
cd ~/actions-runner
sudo systemctl stop actions.runner.zilhak-tasty.<runner-name>.service
sudo systemctl disable actions.runner.zilhak-tasty.<runner-name>.service

# REMOVAL_TOKEN은 GitHub Settings → Runners → 해당 러너 → ... → Remove에서 발급
./config.sh remove --token <REMOVAL_TOKEN>
```

## 트러블슈팅

| 증상 | 원인 / 해결 |
|------|-------------|
| CI에서 `cargo-deb not found` | 1회 설치 누락. `cargo install cargo-deb` 실행 |
| CI에서 `cargo-generate-rpm not found` | 1회 설치 누락. `cargo install cargo-generate-rpm` 실행 |
| CI에서 `linuxdeploy not found` | `~/.local/bin/linuxdeploy` 미설치 또는 PATH 누락. 1회 설치 + `release.yml`의 `Setup PATH` 스텝 점검 |
| `cmake`/`freetype`/`fontconfig` 빠짐 | `build-linux.sh`가 시작 시 명시적 에러로 안내. 메시지대로 `apt install` |
| 러너가 GitHub에서 offline | `systemctl status`로 서비스 상태 확인. 정지되었으면 `restart` |
| 빌드는 성공했는데 release upload 실패 | `GH_TOKEN` 권한 또는 release tag 이름 확인. `--clobber`로 재업로드 가능 |
| Windows에서 `compiler application (candle) could not be found in the PATH` | WiX Toolset 3.x 미설치 또는 `WIX` 환경변수 누락. `winget install WiXToolset.WiXToolset` 후 러너 서비스 재시작. PATH에 `%WIX%\bin`을 직접 추가할 필요는 없다 (`build-windows.ps1`이 자동 처리) |
| AppImage 실행 시 `fuse: failed to exec fusermount` | 타깃 사용자 환경 문제 (FUSE 미지원). 해결: 사용자가 `./Tasty-x.y.z-x86_64.AppImage --appimage-extract-and-run`으로 실행 |
