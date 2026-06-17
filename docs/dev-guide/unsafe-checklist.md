# Unsafe 작성 체크리스트

워크스페이스 lint 가 모든 `unsafe` 블록에 `// SAFETY:` 주석을 강제한다(`undocumented_unsafe_blocks = "deny"`). 본 문서는 그 주석을 무엇으로 채울지, 새 unsafe 전 무엇을 자문할지 정의한다.

## 형식

```rust
// SAFETY: <한 줄 요약 — "왜 안전한가">
// - <invariant 1>: 누가/어디서 보장하는지
// - <invariant 2>: 호출자 책임 / OS 문서 참조
unsafe { ... }
```

형식적이면 안 된다. **무엇이 안전을 보장하는지가 핵심** — 검토자가 SAFETY 주석만 보고 동의할 수 있어야 한다.

## 자가검토 5문

1. **OS thread affinity 가 있는가?** AppKit·Win32 GUI = main thread only. Xlib `Display*` = `XInitThreads()` 없이는 not thread-safe. POSIX 기본 syscall = thread-safe.
2. **raw pointer lifetime 이 어디까지 보장되나?** 호출 끝까지 / 함수 / `'static`? 누가 free/release 책임?
3. **이 FFI 가 panic safe 한가?** panic 시 invariant(lock 미해제 등) 깨지는가?
4. **Drop 순서가 의존성을 만족하나?** (예: webview controller 가 hwnd 보다 먼저 drop)
5. **같은 함수의 unsafe 블록 2개+ 를 합쳐도 되나?** (`multiple_unsafe_ops_per_block` lint 가 분할 여부를 가린다)

## 의심스러우면 `unsafe fn`

caller 에게 안전성 책임을 명시적으로 넘기는 게 깔끔하다. `unsafe fn` 내부 op 도 명시적 unsafe 블록 강제(`unsafe_op_in_unsafe_fn = "deny"`).

```rust
/// # Safety
/// 호출자는 클립보드가 열린 상태에서 호출해야 한다.
unsafe fn read_clipboard_inner() -> Option<...> {
    // SAFETY: 호출자가 OpenClipboard 성공을 보증 (function-level Safety doc 참조).
    let handle = unsafe { GetClipboardData(...) };
}
```

## SAFETY 주석 예시

```rust
// macOS — main thread 강제
// SAFETY: setAction 은 AppKit main thread only. NSApplicationDelegate 콜백 시그니처로
// ObjC 런타임이 main thread 에서만 호출.
unsafe { item.setAction(Some(sel!(tastyNewWindow:))) };

// Win32 — open/close 시퀀스
// SAFETY: OpenClipboard → SetClipboardData* → CloseClipboard 를 한 함수 안에서 완결.
// SetClipboardData 성공 시 HGLOBAL 소유권 OS 이전, 모든 분기에서 CloseClipboard.
unsafe { OpenClipboard(None)?; ...; CloseClipboard()?; }

// Xlib — Display 단일 thread
// SAFETY: PlatformWebView 는 main thread(winit event loop)에서만 생성/조작.
// XInitThreads 미호출 환경이라 Display* 호출은 main thread 한정.
unsafe { (xlib.XMapWindow)(display, x11_window); }
```

## 새 unsafe 절차

1. 정말 필요한지 재검토(`bytemuck::cast_slice`, `std::mem::take`, `pin-project` 등 safe 대안).
2. 5문에 답한다 → SAFETY 주석으로 작성.
3. `cargo clippy --workspace --all-targets` 통과 확인.
4. OS 의존이면 `docs/dev-guide/` 관련 문서에도 invariant 를 적는다.

## lint 정책

| lint | 수준 |
|------|------|
| `clippy::undocumented_unsafe_blocks` | `deny` (모든 unsafe 블록 SAFETY 필수) |
| `clippy::multiple_unsafe_ops_per_block` | `warn` (향후 deny 검토) |
| `rust::unsafe_op_in_unsafe_fn` | `deny` (edition 2024 기본) |

리뷰어는 코드보다 **SAFETY 본문(invariant) 검증을 우선** 한다. OS 문서 링크가 있으면 SAFETY 에 포함(장기 변동 대응).
