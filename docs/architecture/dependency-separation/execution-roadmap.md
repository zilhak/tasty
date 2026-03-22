# 실행 로드맵

파일 분할을 7단계로 실행한다. 각 단계는 독립적으로 커밋 가능하며, 단계 사이에 `cargo check`와 `cargo test`로 검증한다.

## 전제 조건

- 모든 테스트가 통과하는 상태에서 시작
- 기능 변경 없이 순수 구조 변경만 수행
- 각 단계 완료 후 `cargo check && cargo test && cargo clippy`

## 단계별 계획

### 1단계: model.rs → src/model/ (최우선)

**전제 조건:** 없음 (독립 실행 가능)

**작업 내용:**
1. `src/model/` 디렉토리 생성
2. `src/model/mod.rs` 작성 — type aliases, Rect, SplitDirection, DividerInfo, compute_terminal_rect, re-exports
3. `src/model/workspace.rs` — Workspace struct + impl
4. `src/model/pane.rs` — PaneNode + Pane + Tab
5. `src/model/panel.rs` — Panel + SurfaceNode
6. `src/model/surface_group.rs` — SurfaceGroupNode + SurfaceGroupLayout
7. `src/model/tests.rs` — 모든 테스트
8. `src/model.rs` 삭제
9. `main.rs`의 `mod model;`은 디렉토리 모듈로 자동 전환

**파일 변경:**
| 변경 유형 | 파일 |
|----------|------|
| 생성 | `src/model/mod.rs`, `workspace.rs`, `pane.rs`, `panel.rs`, `surface_group.rs`, `tests.rs` |
| 삭제 | `src/model.rs` |
| import 확인 | `main.rs`, `state.rs`, `gpu.rs`, `renderer.rs`, `ui.rs`, `notification.rs`, `ipc/handler.rs` |

**검증:**
```bash
cargo check
cargo test -- model
cargo test
cargo clippy
```

**예상 시간:** 30분

---

### 2단계: terminal.rs → src/terminal/

**전제 조건:** 없음 (1단계와 독립)

**작업 내용:**
1. `src/terminal/` 디렉토리 생성
2. `src/terminal/events.rs` — Waker, TerminalEvent, TerminalEventKind, MouseTrackingMode, OUTPUT_BUFFER_MAX
3. `src/terminal/mod.rs` — Terminal struct + 핵심 API + re-exports
4. `src/terminal/vte_handler.rs` — `impl Terminal`의 action_to_changes, map_* 함수들
5. `src/terminal/modes.rs` — `impl Terminal`의 handle_mode, set_dec_mode
6. `src/terminal/tests.rs` — 모든 테스트
7. `src/terminal.rs` 삭제

**파일 변경:**
| 변경 유형 | 파일 |
|----------|------|
| 생성 | `src/terminal/mod.rs`, `events.rs`, `vte_handler.rs`, `modes.rs`, `tests.rs` |
| 삭제 | `src/terminal.rs` |
| import 확인 | `main.rs`, `model.rs`(또는 `model/mod.rs`), `state.rs` |

**검증:**
```bash
cargo check
cargo test -- terminal
cargo test
cargo clippy
```

**예상 시간:** 25분

---

### 3단계: renderer.rs → src/renderer/

**전제 조건:** 없음 (독립)

**작업 내용:**
1. `src/renderer/` 디렉토리 생성
2. `src/renderer/types.rs` — Uniforms, BgInstance, GlyphInstance
3. `src/renderer/shaders.rs` — BG_SHADER, GLYPH_SHADER
4. `src/renderer/palette.rs` — DEFAULT_FG/BG, ANSI_COLORS, palette_index_to_rgb, color_attr_to_rgba
5. `src/renderer/mod.rs` — CellRenderer struct + impl
6. `src/renderer.rs` 삭제

**파일 변경:**
| 변경 유형 | 파일 |
|----------|------|
| 생성 | `src/renderer/mod.rs`, `types.rs`, `shaders.rs`, `palette.rs` |
| 삭제 | `src/renderer.rs` |
| import 확인 | `gpu.rs` |

**검증:**
```bash
cargo check
cargo clippy
```

**예상 시간:** 15분

---

### 4단계: ipc/handler.rs → src/ipc/handler/

**전제 조건:** 없음 (독립)

**작업 내용:**
1. `src/ipc/handler/` 디렉토리 생성
2. `src/ipc/handler/surface.rs` — surface 관련 핸들러 8개
3. `src/ipc/handler/hooks.rs` — hook 핸들러 3개 + claude.launch
4. `src/ipc/handler/mod.rs` — handle() 라우터 + workspace/pane/tab/system 핸들러
5. `src/ipc/handler.rs` 삭제
6. `src/ipc/mod.rs` 수정 (handler가 디렉토리 모듈이 되므로 변경 불필요할 수 있음)

**파일 변경:**
| 변경 유형 | 파일 |
|----------|------|
| 생성 | `src/ipc/handler/mod.rs`, `surface.rs`, `hooks.rs` |
| 삭제 | `src/ipc/handler.rs` |
| import 확인 | `main.rs` (ipc::handler::handle 호출) |

**검증:**
```bash
cargo check
cargo clippy
```

**예상 시간:** 15분

---

### 5단계: main.rs 분할

**전제 조건:** 1~4단계 완료 권장 (main.rs가 다른 모듈을 참조하므로)

**작업 내용:**
1. `src/shortcuts.rs` 생성 — handle_shortcut, paste_to_terminal
2. `src/event_handler.rs` 생성 — impl ApplicationHandler for App
3. `main.rs`에서 해당 코드 제거하고 `mod shortcuts;`, `mod event_handler;` 추가

**파일 변경:**
| 변경 유형 | 파일 |
|----------|------|
| 생성 | `src/shortcuts.rs`, `src/event_handler.rs` |
| 수정 | `src/main.rs` |

**검증:**
```bash
cargo check
cargo test
cargo clippy
```

**예상 시간:** 20분

---

### 6단계: 전체 검증

**전제 조건:** 1~5단계 완료

**작업 내용:**
1. 전체 빌드 확인: `cargo build`
2. 전체 테스트: `cargo test`
3. 린트: `cargo clippy -- -W clippy::all`
4. 포맷: `cargo fmt --check`
5. 실제 실행 테스트: 앱을 실행하여 기본 기능 동작 확인
   - 터미널 입출력
   - 워크스페이스 생성/전환
   - 탭 생성/닫기
   - 패인 분할/닫기
   - IPC 통신 (`tasty info`)

**예상 시간:** 15분

---

### 7단계: import 그래프 정리

**전제 조건:** 6단계 완료

**작업 내용:**
1. 불필요한 `pub` 가시성 축소 → `pub(crate)` 또는 `pub(super)`
2. 미사용 import 정리
3. `cargo clippy` 경고 0개 확인
4. 모듈별 문서 주석(//!) 추가

**구체적 검토 항목:**
- `Tab::take_panel` / `Tab::put_panel`: `pub(super)` 확인
- `SurfaceGroupNode::take_layout` / `put_layout`: `pub(super)` 확인
- `Terminal::action_to_changes`: `pub(super)` 확인
- `Terminal::handle_mode`: `pub(super)` 확인 (테스트에서 직접 호출하므로 `pub(crate)` 필요할 수 있음)
- renderer 내부 타입: `pub(super)` vs `pub(crate)` 결정

**예상 시간:** 20분

## 총 예상 시간

| 단계 | 시간 |
|------|------|
| 1단계 model | 30분 |
| 2단계 terminal | 25분 |
| 3단계 renderer | 15분 |
| 4단계 handler | 15분 |
| 5단계 main | 20분 |
| 6단계 검증 | 15분 |
| 7단계 정리 | 20분 |
| **합계** | **약 2시간 20분** |

## 라이브러리 분리 계획과의 통합 타임라인

파일 분할은 [라이브러리 분리 계획](../library-separation/index.md)의 Phase 0에 해당한다.

```
Phase 0: 파일 분할 (이 문서)
    ├── model.rs → src/model/
    ├── terminal.rs → src/terminal/
    ├── renderer.rs → src/renderer/
    ├── handler.rs → src/ipc/handler/
    └── main.rs 분할

Phase 1: tasty-terminal crate 추출
    └── src/terminal/ → crates/tasty-terminal/

Phase 2: tasty-model crate 추출
    └── src/model/ → crates/tasty-model/

Phase 3: tasty-renderer crate 추출
    └── src/renderer/ → crates/tasty-renderer/

Phase 4: tasty-ipc crate 추출
    └── src/ipc/ → crates/tasty-ipc/
```

Phase 0(파일 분할)을 먼저 완료하면, 각 Phase에서 디렉토리 모듈의 `mod.rs`를 `lib.rs`로 승격하는 것만으로 crate 추출이 가능해진다. pub 인터페이스가 이미 `mod.rs`의 re-export로 정리되어 있으므로 외부 코드 변경이 최소화된다.

## 병렬 실행 가능성

1~4단계는 서로 독립적이므로 병렬 작업이 가능하다. 단, 동일 브랜치에서 작업할 경우 merge conflict를 방지하기 위해 순차 실행을 권장한다. 별도 브랜치에서 작업한다면 다음 조합이 가능하다:

- 브랜치 A: 1단계 (model) + 2단계 (terminal)
- 브랜치 B: 3단계 (renderer) + 4단계 (handler)
- 메인 브랜치: A, B 순차 merge → 5단계 (main) → 6~7단계 (검증/정리)
