# 개발자 경험 관점

라이브러리 분리가 기여자, 사용자, 개발 워크플로우에 미치는 영향을 분석한다.

---

## 기여자 관점: 격리된 컴포넌트에서 작업 가능?

### tasty-hooks

**완전 격리 가능.** hooks.rs는 tasty 내부 타입을 참조하지 않으므로, 기여자가 tasty 전체를 이해하지 않아도 훅 시스템만 수정할 수 있다.

기여 시나리오:
- "새 이벤트 타입 추가" → `tasty-hooks`만 수정
- "훅 실행 방식 변경 (셸 → 함수 콜백)" → `tasty-hooks`만 수정
- "정규식 매칭 성능 개선" → `tasty-hooks`만 수정

현재는 `cargo build`가 winit, wgpu, egui 등 전체 의존을 컴파일하므로, hooks만 수정하는 기여자가 불필요한 대기 시간을 겪는다.

### tasty-terminal

**대부분 격리 가능.** terminal.rs는 `portable-pty`와 `termwiz`에만 의존하므로, PTY/VTE 관련 작업은 tasty-terminal 내에서 완결.

기여 시나리오:
- "새 VTE 시퀀스 지원" → `tasty-terminal`만 수정
- "Read Mark API 개선" → `tasty-terminal`만 수정
- "대체 화면(alternate screen) 버그 수정" → `tasty-terminal`만 수정

예외: Terminal의 공개 API를 변경하면 `model.rs`와 `state.rs`도 수정 필요.

### 나머지 후보

| 후보 | 격리 가능성 | 이유 |
|------|-----------|------|
| `tasty-ipc-protocol` | 완전 | 단, 131줄이므로 격리 이점 미미 |
| `tasty-ipc-server` | 대부분 | 포트 파일 로직이 tasty 고유 |
| `tasty-notification` | 대부분 | 타입 별칭만 교체하면 독립 |
| `tasty-settings` | 불가능 | 설정 필드가 tasty 기능에 밀접 결합 |
| `tasty-model` | 제한적 | 모든 UI/렌더러 코드가 model에 의존 |
| `tasty-renderer` | 제한적 | Surface/Rect 등 model 의존 |

---

## 빌드/테스트 워크플로우 변화

### 현재 워크플로우

```bash
# hooks만 수정한 경우에도:
cargo build          # 전체 빌드 (winit+wgpu+egui+termwiz+...)
cargo test           # 전체 테스트
cargo clippy         # 전체 린트
```

### 분리 후 워크플로우

```bash
# hooks만 수정한 경우:
cargo test -p tasty-hooks    # 2초, regex만 빌드
cargo clippy -p tasty-hooks  # 1초

# terminal만 수정한 경우:
cargo test -p tasty-terminal     # 5초, portable-pty+termwiz만 빌드

# 통합 확인:
cargo build                  # 전체 빌드 (변경 없는 크레이트는 캐시)
cargo test                   # 전체 테스트
```

### 워크플로우 개선 요약

| 작업 | 현재 | 분리 후 | 개선 |
|------|------|--------|------|
| hooks 테스트 | ~15초 | ~2초 | 7.5x |
| terminal 테스트 | ~15초 | ~5초 | 3x |
| hooks만 린트 | ~10초 | ~1초 | 10x |
| 전체 빌드 (캐시) | ~15초 | ~10초 | 1.5x |

---

## IDE 지원 (rust-analyzer 성능)

### 현재

rust-analyzer가 단일 크레이트의 8,870줄을 분석. 프로젝트가 작으므로 문제 없음.

### 분리 후

rust-analyzer가 workspace의 각 크레이트를 독립적으로 분석. 영향:

- **완성(autocomplete)**: 변화 없음. workspace 내 크레이트를 자동 해석.
- **진단(diagnostics)**: 변화 없음.
- **Go-to-definition**: 분리된 크레이트의 소스로 점프 가능. `path` 의존이므로 소스 접근 보장.
- **인덱싱 속도**: workspace 크레이트가 캐시되므로, hooks/terminal 미변경 시 재인덱싱 불필요. 소폭 개선.
- **메모리 사용**: 미미한 증가 (크레이트별 분석 컨텍스트).

**결론**: IDE 경험에 유의미한 차이 없음.

---

## 코드 탐색 용이성

### 현재

모든 코드가 `src/` 아래에 평면적으로 배치:

```
src/
├── cli.rs
├── font.rs
├── gpu.rs
├── hooks.rs
├── main.rs
├── model.rs
├── notification.rs
├── renderer.rs
├── settings.rs
├── settings_ui.rs
├── state.rs
├── terminal.rs
├── ui.rs
└── ipc/
    ├── mod.rs
    ├── handler.rs
    ├── protocol.rs
    └── server.rs
```

장점: 모든 파일이 한 곳. `grep`, `rg` 등으로 전체 검색 용이.
단점: 파일 간 의존 관계가 명시적이지 않음.

### 분리 후

```
crates/
├── tasty-hooks/
│   └── src/lib.rs          (290줄)
├── tasty-terminal/
│   └── src/lib.rs          (1,358줄)
src/
├── cli.rs
├── font.rs
├── gpu.rs
├── main.rs
├── model.rs                (model.rs:1 → use tasty_terminal)
├── notification.rs
├── renderer.rs
├── settings.rs
├── settings_ui.rs
├── state.rs                (state.rs:6 → use tasty_terminal)
├── ui.rs
└── ipc/
    ├── mod.rs
    ├── handler.rs
    ├── protocol.rs
    └── server.rs
```

장점:
- `crates/` 디렉토리가 "이것들은 독립 라이브러리"라는 아키텍처 의도를 명시적으로 전달
- 새 기여자가 "hooks만 수정하고 싶다"면 `crates/tasty-hooks/`만 보면 됨
- 의존 방향이 `Cargo.toml`에 명시적으로 선언됨

단점:
- 파일이 두 곳 (`src/`, `crates/`)에 분산
- 전체 검색 시 `-p` 옵션이나 경로 지정 필요 (사실 `rg`는 기본적으로 재귀 검색)

---

## 학습 곡선

### 새 기여자가 알아야 할 것

#### 현재
- Rust 기본 + `mod` 시스템
- `cargo build`, `cargo test`

#### 분리 후 (추가)
- Cargo workspace 개념 (`[workspace]`, `members`)
- `cargo test -p <crate>` 사용법
- `path` 의존의 의미

이 추가 학습은 Rust 개발자에게 **매우 기초적인 수준**이다. Cargo workspace는 Rust 생태계의 표준 패턴이며, `serde`, `tokio`, `wgpu` 등 대부분의 대형 크레이트가 workspace를 사용한다.

### 외부 사용자 (tasty-hooks, tasty-terminal)

분리된 크레이트를 외부에서 사용하려는 사용자 관점:

```toml
# 사용자의 Cargo.toml
[dependencies]
tasty-hooks = { git = "https://github.com/zilhak/tasty" }
# 또는 crates.io 공개 후:
# tasty-hooks = "0.1"
```

```rust
use tasty_hooks::{HookManager, HookEvent};

let mut manager = HookManager::new();
manager.add_hook(1, HookEvent::ProcessExit, "echo done".into(), true);
```

API가 자기 설명적이므로 학습 곡선 최소.

---

## 종합 판정

| 후보 | 기여자 격리 | 빌드 개선 | IDE | 탐색성 | 학습 곡선 | 판정 |
|------|-----------|----------|-----|--------|----------|------|
| `tasty-hooks` | 완전 | 7.5x | 동일 | 개선 | 최소 | **O** |
| `tasty-terminal` | 대부분 | 3x | 동일 | 개선 | 최소 | **O** |
| `tasty-ipc-protocol` | 완전 | 무의미 | 동일 | 과분 | 최소 | **△** |
| `tasty-ipc-server` | 대부분 | 무의미 | 동일 | 과분 | 최소 | **△** |
| `tasty-notification` | 대부분 | 미미 | 동일 | 소폭 | 최소 | **△** |
| `tasty-settings` | 불가능 | 미미 | 동일 | 과분 | 최소 | **X** |
| `tasty-model` | 제한적 | 의미 있음 | 동일 | 개선 | 중간 | **△** |
| `tasty-renderer` | 제한적 | 의미 있음 | 동일 | 개선 | 중간 | **△** |
