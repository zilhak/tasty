# 에러 처리 정책

`Result` 를 무시하면 에러가 발생해도 흔적이 남지 않아 회귀 추적이 불가능해진다. 모든 `Result` 는 처리하거나 로그를 남긴다.

## 원칙

**`Result` 를 `let _ =` 로 무시하지 않는다.** 무시는 명시적인 정책 결정이어야 하고, 그 이유가 주석으로 남아야 한다.

## 패턴

### ✅ 에러 발생 시 로그 — 기본 패턴

```rust
if let Err(e) = self.state.split_surface(SplitDirection::Vertical) {
    tracing::warn!("split_surface failed: {e}");
}
```

### ✅ 에러를 상위로 전파 — 호출자가 처리 가능

```rust
self.state.split_surface(SplitDirection::Vertical)?;
```

### ❌ 무음 무시 — 금지

```rust
let _ = self.state.split_surface(SplitDirection::Vertical);  // 에러가 사라짐
```

## 로그 레벨 선택

| 레벨 | 사용 시점 |
|------|----------|
| `tracing::error!` | 복구 불가능한 실패. 사용자 작업이 의미를 잃은 상태. (예: 설정 저장 실패, layout.json 파싱 실패) |
| `tracing::warn!` | 무시해도 동작은 계속되는 실패. 부분 기능 손실. (예: 옵션 hook 실행 실패, 텔레메트리 전송 실패) |
| `tracing::debug!` | 정상 흐름의 한 분기로 실패가 예상되는 경우 (예: optional feature 미설치) |

기준선: 사용자가 "방금 동작 안 됐는데 왜?" 라고 물었을 때 답을 찾을 수 있는 로그가 남아 있어야 한다.

## 의도적 무시

진짜로 무시해야 하는 극소수 경우에만 `let _ =` 를 허용하되, **왜 무시하는지 한 줄 주석**을 남긴다.

```rust
// 채널 receiver 가 이미 drop 된 정상 종료 케이스 — 송신 실패는 무시.
let _ = tx.send(msg);
```

근거 없는 `let _ =` 는 리뷰에서 차단한다.

## 로그 메시지 작성

- **무엇이 실패했는지** + **원인** + **(가능하면) 영향**.
- 변수 보간으로 컨텍스트를 담는다.

```rust
// ❌ 컨텍스트 없음
tracing::warn!("failed");

// ✅ 무엇이 / 어떤 인자로 / 왜 실패했는지
tracing::warn!("hook {hook_id} failed for surface {surface_id}: {e}");
```

## anyhow / thiserror

라이브러리 사용법(에러 타입 정의, `?` 전파, context 첨부)은 [`docs/dev-guide/libs/error-handling.md`](libs/error-handling.md) 참조. 본 문서는 정책만 다룬다.
