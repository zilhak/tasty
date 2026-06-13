# CWD 정책

## 개요

각 surface 가 자기 "현재 폴더" (cwd) 를 정의·갱신하는 방식. 이 cwd 는 **surface 전환 / 새 tab 의 cwd 상속 / split 시 부모 cwd carry / 터미널 링크 해석 / 닫힌 항목 복원** 등에서 활용된다.

호스트는 `cwd_from_surface(engine, sid)` (`src/state.rs:898`) 로 surface 의 cwd 를 조회한다. terminal kind 면 `engine.terminals.get(sid).get_cwd()` 로 store 경유, 그 외는 `Surface::source_cwd()` trait method 호출.

생성 시 cwd 가 *손실 없이 carry* 되는 invariant 는 [`docs/architecture/invariants/surface-cwd.md`](../../architecture/invariants/surface-cwd.md) 참조. 본 문서는 **각 surface 가 자기 cwd 를 *어떻게 정의하고 갱신하는가*** 를 다룬다.

## Surface 별 cwd 동작

| Surface | "현재 폴더" 의 정의 | 갱신 트리거 | host 가 아는 방법 |
|---------|---------------------|------------|------------------|
| **Terminal** | shell 의 `$PWD` | shell 이 OSC 7 escape sequence 송신 시 | termwiz parse → `Terminal.cached_cwd` (store 보관). `cwd_from_surface` 가 `engine.terminals.get(sid).get_cwd()` 로 조회 |
| **Markdown** | 열린 파일의 parent 디렉토리 | 파일 자체가 안 바뀌므로 *불변* | `MarkdownPanel::source_cwd()` 가 `self.file.parent()` 반환 |
| **Image** | 열린 파일의 parent 디렉토리 | 불변 | `ImagePanel::source_cwd()` 가 `self.file.parent()` 반환 (file 없으면 None) |
| **Empty** | 생성 시 carry 된 cwd | 불변 | `EmptySurface::source_cwd()` 가 `self.cwd.clone()` 반환 |
| **Explorer (RemoteSurface)** | **주소바 (root_path) 에 표시된 폴더** | 주소바 입력 + enter / 즐겨찾기 클릭 / 폴더 더블클릭 진입 등 root_path 가 바뀌는 모든 path | plugin → host IPC `surface.set_cwd` 발사 → host 가 `RemoteSurface.cwd` 필드 갱신 → `RemoteSurface::source_cwd()` 가 그 값 반환 |
| **기타 RemoteSurface** (git-viewer / codex / claude / ...) | (각 plugin 이 자기 의미로 결정) | (각 plugin 이 자기 의미로 결정) | 동일하게 `surface.set_cwd` IPC 사용 권장. 미구현 plugin 은 cwd None 으로 남음 (호환성 OK) |

### 정의가 *불변* 인 surface (Markdown / Image / Empty)

이들 surface 의 cwd 는 생성 시 결정되고 이후 바뀌지 않는다. 사용자가 *변경할 수단이 없기 때문* — markdown viewer 안에서 디렉토리를 이동할 수 없고, empty surface 는 자체로 어떤 path 도 표시하지 않는다.

따라서 갱신 메커니즘 불요. `source_cwd()` 가 정적 값 반환만으로 충분.

### 정의가 *동적* 인 surface (Terminal / Explorer 등)

사용자가 surface 안에서 *현재 위치를 이동* 할 수 있는 surface — terminal 의 `cd`, explorer 의 주소바 이동. 이런 surface 는 *갱신 메커니즘이 필수*:

- **Terminal**: shell 이 자체 OSC 7 송신 (호스트 협조 불요). 자세한 OSC 7 정책은 ["터미널 OSC 7"](#터미널-osc-7) 섹션 참조.
- **Explorer / 동적 RemoteSurface**: plugin → host IPC `surface.set_cwd` 메커니즘 사용. plugin 이 자기 *현재* 폴더가 바뀐 시점마다 host 에 통보.

## 터미널 OSC 7

쉘이 프롬프트마다 `\e]7;file://hostname/path\e\\` 을 보내면 즉시 `cached_cwd` 에 반영. 비용 0, 이벤트 기반이므로 폴링 불필요. **모든 플랫폼에서 OSC 7 에만 의존**.

### 쉘별 지원 현황

| 쉘 | OSC 7 | 비고 |
|----|-------|------|
| zsh | 기본 지원 | macOS 기본 쉘 |
| fish | 기본 지원 | 자동 |
| bash | 수동 설정 필요 | `PROMPT_COMMAND` 에 추가 |
| PowerShell 7+ | 수동 설정 필요 | `prompt` 함수 수정 |

### bash 에서 OSC 7 활성화

```bash
# ~/.bashrc 에 추가
PROMPT_COMMAND='printf "\033]7;file://%s%s\033\\" "$HOSTNAME" "$PWD"'
```

### 쉘이 OSC 7 을 보내지 않으면?

`cached_cwd` 가 비어 있는 상태로 유지되며, 새 터미널 분할 시 부모 CWD 상속이 동작하지 않는다. 사용 중인 쉘이 OSC 7 을 송신하도록 프롬프트를 설정하면 해결된다.

### 관련 코드

| 파일 | 역할 |
|------|------|
| `crates/tasty-terminal/src/vte_handler.rs` | OSC 7 수신 시 `cached_cwd` 즉시 갱신 |
| `crates/tasty-terminal/src/lib.rs` | `Terminal::get_cwd()`, `set_cached_cwd()` |

## RemoteSurface 의 `surface.set_cwd` IPC

동적으로 cwd 가 변하는 plugin surface 는 본 IPC 로 host 에 통보한다.

### 시그니처

```jsonc
// plugin → host
{
    "method": "surface.set_cwd",
    "params": {
        "surface_id": 42,
        "cwd": "/foo/bar"        // None 도 허용 (cwd 해제)
    }
}
// 응답
{ "ok": true }
```

### 권한

manifest 의 `permissions` 에 `surface.write` 필요. 미선언 plugin 은 호출 시 권한 거부.

### 갱신 시점 (plugin 측 책임)

plugin 은 *사용자가 인지하는 "현재 폴더" 가 바뀐 모든 path* 에 본 IPC 를 발사해야 한다. 단일 setter 로 모든 변경 path 를 모으는 패턴 권장 — 변경 지점 누락 방지.

```rust
impl ExplorerSurface {
    pub fn set_root(&mut self, new_root: PathBuf, host: &HostHandle, sid: u32) {
        if !new_root.is_dir() { return; }   // validation 실패 시 통보 안 함
        self.root = new_root.clone();
        let _ = host.call("surface.set_cwd", json!({
            "surface_id": sid,
            "cwd": new_root.to_string_lossy(),
        }));
    }
}
```

### host 측 저장

`RemoteSurface.cwd: Arc<Mutex<Option<PathBuf>>>` 필드에 보관. `RemoteSurface::source_cwd()` 가 lock 값 clone 반환. 기존 `cwd_from_surface` 분기 수정 없이 자동으로 carry 경로에 합류.

### 옛 plugin 호환성

옛 plugin SDK (본 IPC 모름) 는 `surface.set_cwd` 호출 자체를 안 하므로 cwd 가 None 으로 남는다. 본 IPC 는 *추가만* 하므로 옛 plugin 빌드는 깨지지 않는다 — JSON-RPC 호환성 유지.

## 관련 문서

- [`docs/architecture/invariants/surface-cwd.md`](../../architecture/invariants/surface-cwd.md) — 생성 시 cwd carry invariant
- [`docs/design/flows/split-command.md`](../flows/split-command.md) — split/새 tab 시 cwd 상속 정책
- [`docs/design/flows/explorer-context-menu.md`](../flows/explorer-context-menu.md) — explorer 의 UI 동작
