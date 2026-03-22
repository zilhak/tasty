# 실행 계획 + 로드맵

라이브러리 분리의 단계별 실행 계획. 각 Phase의 상세 단계, 변경 파일, 전제 조건, 위험 요소, 롤백 계획을 포함한다.

---

## Phase 1: tasty-hooks 분리 (즉시)

### 전제 조건

- 없음. 현재 상태에서 바로 실행 가능.

### 상세 단계

#### 1.1 디렉토리 및 Cargo.toml 생성

```bash
mkdir -p crates/tasty-hooks/src
```

`crates/tasty-hooks/Cargo.toml` 생성:

```toml
[package]
name = "tasty-hooks"
version = "0.1.0"
edition = "2024"
license = "MIT"

[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
```

#### 1.2 소스 이동

`src/hooks.rs` → `crates/tasty-hooks/src/lib.rs`

변경 사항:
- `use std::collections::HashSet;` — 유지
- `use std::process::Command;` — 유지
- 모든 `pub` 선언 유지 (이미 공개 API에 적합)
- `#[cfg(test)] mod tests` 블록 유지

#### 1.3 루트 Cargo.toml 수정

```toml
[workspace]
members = [".", "crates/tasty-hooks"]
resolver = "3"

[dependencies]
tasty-hooks = { path = "crates/tasty-hooks" }
# regex = "1"  ← 제거 (tasty-hooks가 소유)
```

주의: 루트에서 `regex` 의존이 남아 있다면 확인 필요. `terminal.rs`의 `read_since_mark`에서도 `regex`를 사용하므로, `regex`는 루트와 tasty-hooks 양쪽에 존재할 수 있다. workspace.dependencies로 관리.

#### 1.4 import 경로 갱신

변경 파일 목록:

| 파일 | 현재 | 변경 후 |
|------|------|--------|
| `src/main.rs:4` | `mod hooks;` | 제거 |
| `src/main.rs` | `use crate::hooks::...` | `use tasty_hooks::...` |
| `src/state.rs:1` | `use crate::hooks::HookManager;` | `use tasty_hooks::HookManager;` |
| `src/ipc/handler.rs:3` | `use crate::hooks::HookEvent;` | `use tasty_hooks::HookEvent;` |

#### 1.5 빌드 및 테스트 확인

```bash
cargo build
cargo test -p tasty-hooks    # 16개 테스트 통과 확인
cargo test                   # 전체 테스트 통과 확인
cargo clippy --workspace
```

#### 1.6 `src/hooks.rs` 삭제

빌드/테스트 통과 확인 후 원본 파일 삭제.

### 변경 파일 요약

| 파일 | 변경 유형 |
|------|----------|
| `crates/tasty-hooks/Cargo.toml` | 신규 |
| `crates/tasty-hooks/src/lib.rs` | 신규 (hooks.rs 이동) |
| `Cargo.toml` | workspace 추가, 의존 추가 |
| `src/main.rs` | `mod hooks` 제거, import 변경 |
| `src/state.rs` | import 변경 |
| `src/ipc/handler.rs` | import 변경 |
| `src/hooks.rs` | 삭제 |

### 예상 시간: 10~15분

### 위험 요소

- **없음.** 커플링 0. import 경로 변경만.

### 롤백 계획

```bash
git checkout -- .    # 모든 변경 취소
```

---

## Phase 2: tasty-terminal 분리 (단기)

### 전제 조건

- Phase 1 완료 (workspace 구조 확립)
- `terminal.rs`의 공개 API 안정화 확인

### 상세 단계

#### 2.1 디렉토리 및 Cargo.toml 생성

```bash
mkdir -p crates/tasty-terminal/src
```

`crates/tasty-terminal/Cargo.toml` 생성:

```toml
[package]
name = "tasty-terminal"
version = "0.1.0"
edition = "2024"
license = "MIT"

[dependencies]
portable-pty = "0.8"
termwiz = "0.22"
regex = "1"
anyhow = "1"
tracing = "0.1"
```

#### 2.2 소스 이동

`src/terminal.rs` → `crates/tasty-terminal/src/lib.rs`

변경 사항:
- 모든 import 유지 (외부 크레이트만 사용)
- 모든 `pub` 선언 유지
- `#[cfg(test)] mod tests` 블록이 있다면 유지

추가 공개 항목 확인:

```rust
// 이 타입들이 모두 pub인지 확인
pub type Waker = Arc<dyn Fn() + Send + Sync>;
pub struct Terminal { ... }
pub struct TerminalEvent { ... }
pub enum TerminalEventKind { ... }
pub enum MouseTrackingMode { ... }
```

`Terminal` 구조체의 필드는 현재 전부 private. 공개 API는 메서드를 통해서만 접근. 이것은 라이브러리 크레이트로서 좋은 설계.

#### 2.3 루트 Cargo.toml 수정

```toml
[workspace]
members = [".", "crates/tasty-hooks", "crates/tasty-terminal"]

[dependencies]
tasty-terminal = { path = "crates/tasty-terminal" }
# portable-pty = "0.8"  ← 제거 (tasty-terminal이 소유)
# regex는 유지 여부 확인 (다른 곳에서 사용하는지)
```

`portable-pty`는 `terminal.rs`에서만 사용하므로 루트에서 제거.

`regex`는 `terminal.rs`의 `read_since_mark`에서도 사용하므로, `tasty-terminal`이 소유하고 루트에서는 더 이상 직접 의존하지 않을 수 있다. 단, 루트에 다른 regex 사용이 있는지 확인 필요.

#### 2.4 import 경로 갱신

변경 파일 목록:

| 파일 | 현재 | 변경 후 |
|------|------|--------|
| `src/main.rs:12` | `mod terminal;` | 제거 |
| `src/main.rs` | `use crate::terminal::...` | `use tasty_terminal::...` |
| `src/model.rs:1` | `use crate::terminal::{Terminal, Waker};` | `use tasty_terminal::{Terminal, Waker};` |
| `src/state.rs:6` | `use crate::terminal::{Terminal, TerminalEvent, Waker};` | `use tasty_terminal::{Terminal, TerminalEvent, Waker};` |

#### 2.5 termwiz re-export 결정

`tasty-terminal`이 `termwiz::surface::Surface`를 공개 API에 노출한다:

```rust
// terminal.rs — 현재
pub fn surface(&self) -> &termwiz::surface::Surface;
```

옵션 A: termwiz를 re-export

```rust
// crates/tasty-terminal/src/lib.rs
pub use termwiz;  // 또는 pub use termwiz::surface::Surface;
```

이러면 tasty 바이너리가 `tasty_terminal::termwiz::surface::Surface`로 접근 가능. 단, termwiz 버전이 tasty-terminal에 고정됨.

옵션 B: tasty도 termwiz에 직접 의존 (현재 구조 유지)

```toml
# 루트 Cargo.toml
[dependencies]
termwiz = "0.22"  # renderer.rs에서도 사용
tasty-terminal = { path = "crates/tasty-terminal" }
```

**권장: 옵션 B.** renderer.rs가 `termwiz::surface::Surface`를 직접 사용하므로(`renderer.rs:3`), 루트에서 termwiz 의존을 유지해야 한다. workspace.dependencies로 버전 동기화.

#### 2.6 빌드 및 테스트 확인

```bash
cargo build
cargo test -p tasty-terminal    # 터미널 테스트 (있다면)
cargo test                      # 전체 통합 테스트
cargo clippy --workspace
```

#### 2.7 `src/terminal.rs` 삭제

빌드/테스트 통과 확인 후 원본 파일 삭제.

### 변경 파일 요약

| 파일 | 변경 유형 |
|------|----------|
| `crates/tasty-terminal/Cargo.toml` | 신규 |
| `crates/tasty-terminal/src/lib.rs` | 신규 (terminal.rs 이동) |
| `Cargo.toml` | workspace 멤버 추가, 의존 추가, portable-pty 제거 |
| `src/main.rs` | `mod terminal` 제거, import 변경 |
| `src/model.rs` | import 변경 |
| `src/state.rs` | import 변경 |
| `src/terminal.rs` | 삭제 |

### 예상 시간: 30분 ~ 1시간

### 위험 요소

1. **termwiz 버전 불일치**: tasty-terminal과 tasty 루트가 다른 termwiz 버전을 사용하면 `Surface` 타입이 호환되지 않음. workspace.dependencies로 방지.

2. **regex LazyLock**: `terminal.rs:2`에서 `LazyLock`을 사용. Rust edition 2024에서는 `std::sync::LazyLock`이 안정화되어 있으므로 문제 없음.

3. **PTY 테스트**: `Terminal::new()`가 실제 PTY를 생성하므로, CI 환경에서 PTY를 지원하지 않으면 테스트 실패. `#[cfg(not(ci))]` 또는 환경 변수로 분기.

### 롤백 계획

```bash
git checkout -- .
```

---

## Phase 3: tasty-ipc 분리 (선택)

**현재 판정: 비권장.** 외부 재사용 가치 부족. 여기서는 실행이 결정될 경우의 계획만 기술한다.

### 전제 조건

- Phase 1, 2 완료
- IPC API가 안정화되어 변경 빈도가 낮아진 시점

### 상세 단계

1. `ipc/protocol.rs` → `crates/tasty-ipc-protocol/src/lib.rs`
2. `ipc/server.rs` → `crates/tasty-ipc-server/src/lib.rs`
3. `port_file_path()` 로직을 매개변수로 리팩토링
4. `ipc/handler.rs`는 tasty 바이너리에 남김 (AppState에 의존)
5. `ipc/mod.rs`를 handler + re-export로 수정

### 예상 시간: 2시간

### 위험 요소

- `handler.rs`가 `AppState`에 깊이 결합되어 있어, 핸들러를 분리하면 대규모 리팩토링 필요.
- 196줄 + 131줄 = 327줄을 두 개의 크레이트로 분리하는 것은 과잉.

---

## Phase 4: tasty-renderer 분리 (장기)

**현재 판정: 장기 과제.** API가 안정되고 다른 VTE 백엔드 지원이 필요해지면 재검토.

### 전제 조건

- 코드베이스 15,000줄 이상 성장
- termwiz 외의 VTE 백엔드 지원 요구
- TerminalSurface trait 설계 완료

### 상세 단계

1. `TerminalSurface` trait 정의 (visitor 패턴 또는 중간 표현)
2. `CellColor` enum 정의 (ColorAttribute 추상화)
3. `Rect` 구조체를 렌더러 크레이트로 이동 또는 공유 `tasty-types` 크레이트
4. `font.rs` + `renderer.rs` → `crates/tasty-renderer/src/`
5. WGSL 셰이더를 크레이트에 포함 (`include_str!`)
6. `termwiz::surface::Surface`에 대한 `TerminalSurface` impl을 feature flag으로 제공
7. 벤치마크 작성 (prepare() 프레임 타임 측정)

### 예상 시간: 2~3일

### 위험 요소

1. **성능 회귀**: TerminalSurface 변환 비용이 예상보다 클 수 있음 (성능 문서 참조)
2. **wgpu API 변경**: wgpu가 major 버전을 올리면 공개 API가 깨질 수 있음
3. **셰이더 호환성**: WGSL 셰이더가 wgpu 버전에 종속적

### 롤백 계획

trait을 도입하되 `termwiz` feature를 기본 활성화하면, 기존 코드가 그대로 동작. 필요시 trait을 제거하고 직접 의존으로 복귀.

---

## model.rs 파일 분할 (크레이트 분리 대안)

model.rs는 1,775줄로 프로젝트에서 가장 큰 파일이다. 크레이트 분리 없이도 **파일 분할**로 유지보수성을 개선할 수 있다.

### 현재 model.rs 내 논리 단위

| 줄 범위 | 내용 | 줄 수 |
|---------|------|-------|
| 1-6 | import, 타입 별칭 | 6 |
| 8-71 | `Rect`, `SplitDirection` | 64 |
| 73-144 | `Workspace` | 72 |
| 146-477 | `PaneNode` (바이너리 트리) | 332 |
| 480-675 | `Pane` | 196 |
| 677-711 | `Tab` | 35 |
| 713-835 | `Panel` | 123 |
| 837-841 | `SurfaceNode` | 5 |
| 843-978 | `SurfaceGroupNode` | 136 |
| 980-1341 | `SurfaceGroupLayout` | 362 |
| 1344-1355 | `compute_terminal_rect()` | 12 |
| 1357-1370 | `DividerInfo`, `SplitDirection` | 14 |
| 1372-1775 | `#[cfg(test)] mod tests` | 403 |

### 권장 파일 분할

```
src/model/
├── mod.rs                  ← 타입 별칭, Rect, SplitDirection, DividerInfo, compute_terminal_rect
├── workspace.rs            ← Workspace
├── pane.rs                 ← PaneNode, Pane
├── tab.rs                  ← Tab
├── panel.rs                ← Panel
├── surface.rs              ← SurfaceNode, SurfaceGroupNode, SurfaceGroupLayout
└── tests.rs                ← #[cfg(test)] mod tests
```

### mod.rs 내용

```rust
// src/model/mod.rs

mod workspace;
mod pane;
mod tab;
mod panel;
mod surface;

#[cfg(test)]
mod tests;

use crate::terminal::{Terminal, Waker};

pub type WorkspaceId = u32;
pub type PaneId = u32;
pub type TabId = u32;
pub type SurfaceId = u32;

#[derive(Debug, Clone, Copy)]
pub struct Rect { ... }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection { Horizontal, Vertical }

#[derive(Debug, Clone, Copy)]
pub struct DividerInfo { ... }

pub fn compute_terminal_rect(...) -> Rect { ... }

// Re-export
pub use workspace::Workspace;
pub use pane::{PaneNode, Pane};
pub use tab::Tab;
pub use panel::Panel;
pub use surface::{SurfaceNode, SurfaceGroupNode, SurfaceGroupLayout};
```

### 이점

- 각 파일이 200~400줄로 관리 가능한 크기
- 구조체별 독립 수정 가능
- `git blame`이 더 유용해짐 (파일별 변경 이력)
- 크레이트 분리 없이도 **논리적 격리** 달성

### 비용

- `pub(crate)` 가시성 조정 필요
- import 경로 변경 (외부에서는 `model::Workspace`로 동일, re-export 덕분)
- 10개 이상의 파일에서 `use crate::model::...` import가 변경될 수 있음 (실제로는 re-export 덕분에 대부분 유지)

### 예상 시간: 1~2시간

이 작업은 **Phase 1, 2와 독립적**으로 실행할 수 있으며, 크레이트 분리보다 우선하거나 동시에 진행해도 된다.

---

## 전체 로드맵 타임라인

```
현재 ──────── Phase 1 ──── Phase 2 ──── (안정화) ──── Phase 3/4 (선택)
              즉시          단기           중기           장기
              10분          1시간                         2~3일

병렬: model.rs 파일 분할 (1~2시간, 언제든 실행 가능)
```

| Phase | 시기 | 작업 | 의존 |
|-------|------|------|------|
| 1 | 즉시 | tasty-hooks 분리 | 없음 |
| 2 | Phase 1 직후 | tasty-terminal 분리 | Phase 1 |
| model 분할 | 언제든 | model.rs → model/ 디렉토리 | 없음 |
| 3 | 비권장 | tasty-ipc 분리 | Phase 1, 2 |
| 4 | 15K줄 이상 시 | tasty-renderer 분리 | Phase 1, 2, trait 설계 |

---

## 각 Phase 완료 검증 체크리스트

### Phase 1 (tasty-hooks)

- [ ] `cargo build` 성공
- [ ] `cargo test -p tasty-hooks` — 16개 테스트 통과
- [ ] `cargo test` — 전체 테스트 통과
- [ ] `cargo clippy --workspace` — 경고 0
- [ ] `src/hooks.rs` 삭제 확인
- [ ] `crates/tasty-hooks/src/lib.rs` 존재 확인

### Phase 2 (tasty-terminal)

- [ ] `cargo build` 성공
- [ ] `cargo test -p tasty-terminal` — 테스트 통과 (있다면)
- [ ] `cargo test` — 전체 테스트 통과
- [ ] `cargo clippy --workspace` — 경고 0
- [ ] `src/terminal.rs` 삭제 확인
- [ ] `crates/tasty-terminal/src/lib.rs` 존재 확인
- [ ] termwiz 버전 일치 확인 (`cargo tree | grep termwiz`)
