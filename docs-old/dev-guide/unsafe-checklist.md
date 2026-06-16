# Unsafe 작성 체크리스트

워크스페이스 lint는 모든 `unsafe` 블록에 `// SAFETY:` 주석을 강제한다
(`undocumented_unsafe_blocks = "deny"`). 본 문서는 그 주석을 무엇으로 채워야 하는지,
새로운 unsafe를 추가하기 전 무엇을 자문해야 하는지를 정의한다.

## 형식

모든 unsafe 블록은 바로 위 줄에 SAFETY 주석을 둔다. 한 줄 요약 + 필요한 invariant
나열 형식.

```rust
// SAFETY: <한 줄 요약 — "왜 안전한가">
// - <invariant 1>: 누가/어디서 보장하는지
// - <invariant 2>: 호출자 책임 / OS 문서 참조
unsafe { ... }
```

내용은 형식적이면 안 된다. **무엇이 안전을 보장하는지가 핵심**. 검토자가 SAFETY 주석만
보고 "그래, 그래서 안전하구나"라고 동의할 수 있어야 한다.

## 자가검토 5문

1. **이 객체가 OS thread affinity가 있는가?**
   - AppKit·Win32 GUI = main thread only.
   - Xlib `Display*` = `XInitThreads()` 없이는 not thread-safe.
   - POSIX 기본 syscall(getuid, kill 등) = thread-safe.
2. **raw pointer의 lifetime이 어디까지 보장되는가?**
   - 호출 끝까지인가, 전체 함수인가, 'static인가?
   - 누가 free/release 책임을 지는가?
3. **이 FFI 함수가 panic safe인가?**
   - panic 시 invariant가 깨지는가? (예: lock 미해제)
4. **Drop 순서가 의존성을 만족하는가?**
   - Webview controller가 hwnd보다 먼저 dropped되어야 한다 등.
5. **같은 함수에 unsafe 블록이 2개 이상이면 합쳐도 되는가?**
   - `multiple_unsafe_ops_per_block` lint(현재 warn)가 분할 여부를 가린다.

## 의심스러우면 함수 자체를 `unsafe fn`으로

caller에게 안전성 책임을 명시적으로 넘기는 것이 깔끔하다. `unsafe fn` 내부의 unsafe op
도 명시적 unsafe 블록이 강제됨(`unsafe_op_in_unsafe_fn = "deny"`).

```rust
/// # Safety
/// 호출자는 클립보드가 열린 상태에서 본 함수를 호출해야 한다.
unsafe fn read_clipboard_inner() -> Option<...> {
    // SAFETY: 호출자가 OpenClipboard 성공을 보증 (function-level Safety doc 참조).
    let handle = unsafe { GetClipboardData(...) };
    ...
}
```

## SAFETY 주석 예시

### macOS — main thread 강제

```rust
// SAFETY: setAction은 AppKit main thread only. 본 함수는 NSApplicationDelegate 콜백
// 시그니처(unsafe extern "C-unwind")로 ObjC 런타임이 main thread에서만 호출.
unsafe { item.setAction(Some(sel!(tastyNewWindow:))) };
```

### Win32 — open/close 시퀀스

```rust
// SAFETY: OpenClipboard → SetClipboardData* → CloseClipboard를 한 함수 안에서 완결.
// SetClipboardData가 성공하면 HGLOBAL 소유권이 OS로 이전, 모든 분기에서 CloseClipboard 호출.
unsafe {
    OpenClipboard(None)?;
    ...
    CloseClipboard()?;
}
```

### Xlib — Display 단일 thread

```rust
// SAFETY: PlatformWebView는 main thread (winit event loop)에서만 생성/조작된다.
// XInitThreads를 호출하지 않은 환경이라 Xlib Display*에 대한 호출은 main thread로만 한정.
unsafe { (xlib.XMapWindow)(display, x11_window); }
```

### POSIX — thread-safe syscall

```rust
// SAFETY: proc_pidinfo는 darwin libproc 시스템콜로 thread-safe (Apple 문서).
// info는 zeroed로 초기화, ptr+size를 정확히 sizeof만큼 넘김.
let ret = unsafe { libc::proc_pidinfo(...) };
```

## 새 unsafe 추가 시 절차

1. 정말 필요한지 다시 검토. `bytemuck::cast_slice`, `std::mem::take`, `pin-project` 등
   safe 대안이 있는지 확인.
2. 위 5문에 답한다.
3. 답을 SAFETY 주석으로 작성한다.
4. `cargo clippy --workspace --all-targets`가 통과하는지 확인.
5. 새 unsafe가 OS 의존이면 `docs/dev-guide/` 관련 문서에도 invariant를 적는다.

## lint 정책 요약

| lint | 수준 | 비고 |
|------|------|------|
| `clippy::undocumented_unsafe_blocks` | `deny` | 모든 unsafe 블록 SAFETY 필수 |
| `clippy::multiple_unsafe_ops_per_block` | `warn` | 향후 deny 승격 검토. FFI 묶음의 분할 비용 측정 단계 |
| `rust::unsafe_op_in_unsafe_fn` | `deny` | edition 2024 기본값과 동일, 명시적 정책 |

## 새 unsafe 영역 추가 후 회귀 방지

- 새 unsafe 블록 추가 PR은 본 체크리스트 5문에 답한 SAFETY 주석을 포함한다.
- 리뷰어는 SAFETY 본문 검증을 우선한다 — 코드 자체 검토보다 invariant 검토가 더 중요하다.
- OS 문서 링크가 있으면 가급적 SAFETY에 포함한다 (장기 변동 대응).
