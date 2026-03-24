# GUI 테스트 시 스크린샷 방법

| 상태 | 방법 |
|------|------|
| 정상 모드 (IPC 사용 가능) | `ui.screenshot` IPC 호출 — Tasty 자체 렌더링만 캡처 |
| 셸 설정 모드 (IPC 없음) | PowerShell `CopyFromScreen` — OS 전체 화면 캡처 |

**방법 1: IPC `ui.screenshot` (정상 모드, 권장)**
```bash
# CLI로 한 줄
tasty screenshot --path ./capture.png
```

**방법 2: PowerShell OS 캡처 (IPC 없을 때 폴백)**

1. **프로세스 종료**: bash의 `taskkill`은 `/F` 플래그가 경로와 충돌한다. PowerShell을 사용할 것.
   ```bash
   powershell -Command "Get-Process tasty -ErrorAction SilentlyContinue | Stop-Process -Force"
   ```

2. **빌드 → 실행**: tasty.exe가 실행 중이면 cargo build가 exe를 덮어쓸 수 없다 (access denied). 반드시 프로세스를 먼저 종료한 후 빌드할 것.

3. **스크린샷 캡처**: PowerShell 스크립트 파일을 만들어 실행한다. bash에서 `$` 변수가 먹히므로 인라인 PowerShell은 동작하지 않는다.
   ```powershell
   # take_screenshot.ps1
   Add-Type -AssemblyName System.Windows.Forms, System.Drawing
   $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
   $bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
   $g = [System.Drawing.Graphics]::FromImage($bmp)
   $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
   $bmp.Save("E:\workspace\tasty\screenshot.png")
   $g.Dispose(); $bmp.Dispose()
   ```
   ```bash
   powershell -NoProfile -ExecutionPolicy Bypass -File take_screenshot.ps1
   ```

4. **윈도우 포커스/최대화**: `Win32::ShowWindow` + `SetForegroundWindow`로 Tasty 창을 최대화한 후 찍어야 전체가 보인다.
