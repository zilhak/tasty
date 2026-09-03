<!-- source-hash: 7435dd114005 -->
# Installation

Tasty ships as per-OS, per-architecture, per-format artifacts on GitHub Releases. There is no install script and no package-manager integration — every install starts by downloading from the [releases page](https://github.com/zilhak/tasty/releases).

## Artifact matrix

| OS | Architecture | Artifact | Notes |
|----|---------|--------|------|
| Linux | x86_64 | `tasty-{ver}-linux-x64.tar.gz` | Dynamically linked · needs system libraries such as glibc / freetype / fontconfig / GTK |
| Linux | x86_64 | `tasty_{ver}-1_amd64.deb` | Debian / Ubuntu / Mint |
| Linux | x86_64 | `tasty-{ver}-1.x86_64.rpm` | Fedora / RHEL / openSUSE |
| Linux | x86_64 | `Tasty-{ver}-x86_64.AppImage` | Distro-independent single file |
| Linux | aarch64 | `tasty-{ver}-linux-arm64.tar.gz` / `_arm64.deb` / `.aarch64.rpm` / `-aarch64.AppImage` | ARM64 |
| macOS | arm64 | `Tasty-{ver}-macos-arm64.dmg` | Drag-to-install app bundle (Apple Silicon only) |
| Windows | x86_64 | `tasty-{ver}-windows-x64.zip` | Binary only, zipped |
| Windows | x86_64 | `tasty-{ver}-windows-x64.msi` | Installer with Start Menu / uninstall registration |

Each release also attaches the agent-facing [reference/](reference/index.md) docs (IPC/CLI reference).

## Linux

```bash
# .deb (Debian/Ubuntu/Mint)
sudo apt install ./tasty_{ver}-1_amd64.deb      # or dpkg -i + apt-get install -f

# .rpm (Fedora/RHEL/openSUSE)
sudo dnf install ./tasty-{ver}-1.x86_64.rpm

# .AppImage (distro-independent, no install step)
chmod +x Tasty-{ver}-x86_64.AppImage && ./Tasty-{ver}-x86_64.AppImage
# Without FUSE: ./Tasty-*.AppImage --appimage-extract-and-run

# .tar.gz (manual)
tar -xzf tasty-{ver}-linux-x64.tar.gz && ./tasty-linux-x64/tasty
```

- `.deb` / `.rpm`: puts `tasty` on PATH and registers a desktop-menu icon. Dependencies are resolved from the package metadata (`libfreetype6` / `libfontconfig1` / `libgtk-3` / `libwebkit2gtk-4.1` and so on). GPU acceleration (Vulkan) is only a *Recommends* on `libvulkan1` / `vulkan-loader` — without it the package still installs and runs, falling back to the software renderer.
- `.AppImage`: bundles every dependent library. Desktop-menu registration is manual (`appimaged`, or drop a `.desktop` file into `~/.local/share/applications/`).
- `.tar.gz`: PATH and menu registration are up to you. If you want them handled, prefer `.deb` / `.rpm` / `.AppImage`. If a required `.so` is
  missing, the `tasty` wrapper detects it before launch, explains, and exits (`tasty.bin` is the real binary).
- **Minimum glibc**: distributions older than the build environment (Ubuntu 24.04, glibc 2.39) — Ubuntu 20.04, Debian 11, etc. — may fail with
  `tasty: /lib/x86_64-linux-gnu/libc.so.6: version 'GLIBC_2.39' not found`.
  No separate build for older distributions is provided — build from source if you need one ([dev-guide/build](dev-guide/build.md)).

## macOS

Double-click the `.dmg` → drag `Tasty.app` into `Applications`. If Gatekeeper warns on first launch, allow it under System Settings > Privacy & Security (only applies to builds without a notarised code signature).

To bypass it straight from the terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Tasty.app
```

## Windows

```powershell
# .msi (recommended) — double-click → installer wizard. Start Menu shortcut + Add/Remove Programs entry
# .zip (manual)
Expand-Archive tasty-{ver}-windows-x64.zip; .\tasty\tasty.exe
```

### Getting past the SmartScreen warning

There is no Authenticode signature, so the first launch shows "Windows protected your PC". Choose **More info → Run anyway** and it runs.

### Uninstalling (.msi)

Remove Tasty from "Settings > Apps" or Control Panel > Uninstall a program (`msiexec /x` is registered). Uninstalling:

- Cleans up everything under `Program Files\tasty\` (binaries and plugins), the Start Menu shortcut, and the PATH entry.
- **Also deletes all user data under `~/.tasty/`** — config, sessions, themes, and the plugin copies the runtime made. Complete removal is the policy by design (a leftover plugin copy would break trust on reinstall). Back up `~/.tasty/` first if you want to keep it.
- **Upgrades (reinstalling the same product) preserve `~/.tasty/`** — data is only deleted on a real uninstall (`REMOVE="ALL"`).

> `.zip` has no notion of installation: delete the extracted folder and you are done. User data in `~/.tasty/` has to be removed by hand, though.

For the build side, see "Windows MSI" in [dev-guide/build](dev-guide/build.md).

## GPU requirements

Tasty renders with GPU acceleration (wgpu — Vulkan / DX12 / Metal). Without a hardware GPU adapter (GPU-less servers, VMs, containers) it retries once with the software renderer, and if that is unavailable too it prints a notice and exits. All of the artifacts above are GUI builds and do not have a `--headless` flag — to use only IPC/CLI on a machine with no GPU, build headless from source with `cargo build --no-default-features` ([dev-guide/build](dev-guide/build.md)).

## Verifying

```bash
tasty --version       # version
tasty list info       # system info over IPC, when a GUI instance is running
```

## Install locations

| OS | Binary | User data |
|----|---------|--------------|
| Linux | `/usr/bin/tasty` (.deb/.rpm) or wherever you extracted it | `~/.tasty/` |
| macOS | `/Applications/Tasty.app/Contents/MacOS/tasty` | `~/.tasty/` |
| Windows | `C:\Program Files\tasty\bin\tasty.exe` (.msi, perMachine) | `~/.tasty/` |

`~/.tasty/` holds user data such as `tasty.port` (the IPC port), `config.toml`, and sessions (full map: [design/systems/storage](design/systems/storage.md); per-OS paths: [reference/environments](reference/environments.md)).

## Building packages (maintainers)

[dev-guide/build](dev-guide/build.md) · [dev-guide/dist-build](dev-guide/dist-build.md) · [dev-guide/release-runners](dev-guide/release-runners.md).
