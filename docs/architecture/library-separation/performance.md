# 성능 관점

라이브러리 분리가 런타임 성능과 빌드 성능에 미치는 영향을 분석한다.

---

## Dynamic Dispatch 영향

### 현재: 정적 디스패치 (zero-cost)

현재 `model.rs`는 `Terminal` 타입을 직접 소유한다:

```rust
// model.rs:838-841
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,  // 구체 타입, 정적 디스패치
}
```

`terminal.process()`, `terminal.resize()`, `terminal.surface()` 등 모든 호출이 컴파일 타임에 인라이닝 가능.

### tasty-hooks, tasty-terminal 분리: 영향 없음

이 두 크레이트의 분리는 trait 추상화를 도입하지 않는다. `Terminal`은 여전히 구체 타입으로 사용되고, `HookManager`도 구체 타입. **런타임 성능 변화 0.**

### tasty-model 분리 시: trait object vs generics 트레이드오프

#### 옵션 A: 제네릭 (성능 유지, 코드 복잡도 증가)

```rust
pub struct SurfaceNode<T: TerminalBackend> {
    pub id: SurfaceId,
    pub terminal: T,
}
```

- 정적 디스패치 유지 (zero-cost)
- Monomorphization: `SurfaceNode<Terminal>` 전용 코드 생성
- Binary size 영향: 없음 (단일 구체 타입만 사용되므로 monomorphization은 1회)

#### 옵션 B: trait object (코드 단순, 미세 오버헤드)

```rust
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Box<dyn TerminalBackend>,
}
```

vtable lookup 비용이 발생하는 핫 경로:

| 호출 | 빈도 | vtable 비용 |
|------|------|------------|
| `terminal.process()` | 매 프레임 (60fps) × 터미널 수 | ~1ns/call |
| `terminal.surface()` | 매 프레임 × 터미널 수 | ~1ns/call |
| `terminal.resize()` | 윈도우 리사이즈 시 | 무시 가능 |
| `terminal.send_key()` | 키 입력 시 | 무시 가능 |
| `terminal.take_events()` | 매 프레임 × 터미널 수 | ~1ns/call |

60fps, 4개 터미널 기준:
- `process()`: 60 × 4 = 240 calls/sec × 1ns = **240ns/sec**
- `surface()`: 60 × 4 = 240 calls/sec × 1ns = **240ns/sec**
- `take_events()`: 60 × 4 = 240 calls/sec × 1ns = **240ns/sec**
- **총 추가 비용: ~720ns/sec** — 프레임 16.7ms 대비 완전히 무시 가능

추가 비용:
- `Box` 힙 할당: Terminal 생성 시 1회. 무시 가능.
- 캐시 미스: `Box<dyn>` 간접 참조로 포인터 1단계 추가. L1 캐시에 거의 항상 적중.

**결론**: trait object 사용 시에도 성능 영향은 실측 불가능한 수준.

---

### tasty-renderer 분리 시: TerminalSurface trait

```rust
pub fn prepare(&mut self, surface: &dyn TerminalSurface, queue: &wgpu::Queue) {
    let (cols, rows) = surface.dimensions();    // vtable
    let lines = surface.screen_lines();          // vtable + Vec 할당
    for line in lines {
        for cell in line.visible_cells() {       // Vec 순회
            // ...
        }
    }
}
```

이 경우의 핵심 비용은 vtable이 아니라 **중간 표현 변환**이다:

| 항목 | 현재 비용 | trait 분리 후 비용 |
|------|----------|-------------------|
| `screen_lines()` | 참조 반환 (0 alloc) | `Vec<ScreenLine>` 생성 (N alloc) |
| 셀 순회 | iterator (0 alloc) | `Vec<ScreenCell>` (N×M alloc) |
| 색상 접근 | 직접 참조 | `CellColor` enum 변환 |

80×24 터미널 기준:
- `Vec<ScreenLine>`: 24개 할당
- `Vec<ScreenCell>` × 24 라인: 24 × 80 = 1,920 셀 복사
- 매 프레임 (60fps): 초당 115,200 셀 변환

**추정 비용**: ~50μs/frame (프레임 16.7ms 대비 0.3%). 단일 터미널이면 무시 가능하지만, 4개 분할 시 ~200μs (1.2%).

**최적화 방안**: 중간 표현 대신 visitor 패턴 사용:

```rust
pub trait TerminalSurface {
    fn visit_cells<V: CellVisitor>(&self, visitor: &mut V);
}

pub trait CellVisitor {
    fn visit_cell(&mut self, col: usize, row: usize, text: &str, fg: [f32; 4], bg: [f32; 4], bold: bool, italic: bool);
}
```

이러면 할당 0, vtable lookup은 `visit_cells()` 1회만. 단, 설계 복잡도 증가.

---

## 증분 컴파일 이점

### 현재 (단일 크레이트)

`hooks.rs` 1줄 변경 → 전체 바이너리 재링크. `model.rs`, `renderer.rs`, `main.rs` 등 모든 코드 유닛이 재검사.

### 분리 후

| 변경 파일 | 재컴파일 범위 | 예상 시간 |
|-----------|-------------|-----------|
| `tasty-hooks` 내부 | tasty-hooks만 | ~2초 |
| `tasty-terminal` 내부 | tasty-terminal만 | ~5초 |
| `tasty-hooks` 공개 API | tasty-hooks + tasty 바이너리 | ~15초 |
| `tasty-terminal` 공개 API | tasty-terminal + model.rs 재컴파일 | ~20초 |
| `model.rs`, `main.rs` 등 | tasty 바이너리만 | ~15초 |

현재 전체 빌드 시간 기준:
- 클린 빌드 (debug): ~60초 (추정)
- 증분 빌드 (hooks 변경): 현재 ~15초 → 분리 후 ~2초 (**7.5배 개선**)
- 증분 빌드 (terminal 변경): 현재 ~15초 → 분리 후 ~5초 (**3배 개선**)

---

## Binary Size 영향

### 분리가 binary size에 미치는 영향: 없음

Cargo workspace의 `path` 의존은 정적 링킹되므로, 분리 전후 동일한 바이너리가 생성된다.

- 분리 전: 모든 코드가 하나의 컴파일 유닛
- 분리 후: 각 크레이트가 `.rlib`로 컴파일 후 정적 링크
- LTO (`lto = true` in Cargo.toml:64) 적용 시 차이 0

### Monomorphization 비용

제네릭 도입 시에도, 실제 사용되는 구체 타입이 1개(`Terminal`)이므로 monomorphization 인스턴스는 1개. Binary size 증가 없음.

trait object 사용 시에도 vtable은 `TerminalBackend` 1개 타입에 대해 생성되므로, 수십 바이트 수준의 vtable 오버헤드만 추가. 무의미.

---

## 실측 vs 이론

위 분석은 모두 이론적 추정이다. 실측이 필요한 항목:

| 항목 | 측정 방법 | 예상 결과 |
|------|----------|-----------|
| 증분 컴파일 시간 | `cargo build` 전후 `time` 비교 | hooks: 7배 개선 |
| dynamic dispatch 비용 | `criterion` 벤치마크 | 측정 불가능한 수준 |
| TerminalSurface 변환 비용 | `criterion` + `prepare()` 벤치마크 | 50~200μs/frame |
| binary size | `ls -la target/release/tasty` 전후 | 변화 없음 |
| LTO 효과 | profile.release LTO on/off 비교 | 동일 |

**권장**: 분리 후 실제 빌드 시간과 벤치마크를 측정하여 이 문서를 업데이트.

---

## 종합 판정

| 후보 | 런타임 성능 | 빌드 성능 | 판정 |
|------|-----------|----------|------|
| `tasty-hooks` | 영향 없음 | 증분 컴파일 7배 개선 | **O** |
| `tasty-terminal` | 영향 없음 | 증분 컴파일 3배 개선 | **O** |
| `tasty-ipc-protocol` | 영향 없음 | 무의미 (131줄) | **O** |
| `tasty-ipc-server` | 영향 없음 | 무의미 (196줄) | **O** |
| `tasty-notification` | 영향 없음 | 미미 | **O** |
| `tasty-settings` | 영향 없음 | 미미 | **O** |
| `tasty-model` | trait object ~720ns/sec | 의미 있는 개선 | **△** |
| `tasty-renderer` | TerminalSurface 변환 50~200μs/frame | 의미 있는 개선 | **△** |
