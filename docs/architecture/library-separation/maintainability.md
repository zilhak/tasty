# 유지보수 관점

크레이트 분리가 유지보수에 미치는 영향: 관리 부담, 버전 전략, 테스트 격리, CI/CD, 문서화.

---

## 크레이트 수 vs 관리 부담

```
관리 부담
  │
  │                                          ╱
  │                                        ╱    카오스 영역
  │                                      ╱      (너무 많은 크레이트)
  │                                    ╱
  │                                  ╱
  │                              ╱
  │                          ╱╱
  │                     ╱╱
  │              스위트 스팟
  │           ╱╱  (2~4개 크레이트)
  │        ╱
  │     ╱
  │  ╱   모노리스 영역
  │╱     (0개 분리)
  └────────────────────────────────────────── 크레이트 수
  0    1    2    3    4    5    6    7    8
```

현재 tasty의 규모(8,870줄)에서:
- **0개 분리 (현재)**: 문제 없음. 단일 파일 구조가 명확.
- **2개 분리 (권장)**: 관리 부담 미미. Cargo workspace가 처리.
- **4~5개 분리**: 관리 부담이 눈에 띄기 시작. 의존성 버전 동기화 필요.
- **8개 분리**: 8,870줄 코드에 8개 크레이트는 과분. 크레이트당 평균 1,100줄.

**결론**: 2개 분리가 최적. hooks(290줄)와 terminal(1,358줄)만 분리하고 나머지는 유지.

---

## 버전 관리 전략

### Workspace 내부 path 의존 (권장)

```toml
# crates/tasty-hooks/Cargo.toml
[package]
name = "tasty-hooks"
version = "0.1.0"  # workspace 멤버, 아직 publish 안 함

# Cargo.toml (root)
[dependencies]
tasty-hooks = { path = "crates/tasty-hooks" }
tasty-terminal = { path = "crates/tasty-terminal" }
```

장점:
- 바이너리와 라이브러리가 항상 같은 커밋에서 빌드
- semver 호환성 신경 쓸 필요 없음
- 단일 `cargo build`로 전체 빌드

단점:
- 외부 프로젝트에서 사용하려면 git 의존 또는 crates.io 공개 필요

### crates.io 공개 (장기)

crates.io에 공개하면 외부 사용자가 `cargo add tasty-hooks`로 설치 가능. 단, 이 경우:
- semver를 엄격히 준수해야 함
- 공개 API가 안정되기 전에 공개하면 0.x 버전 지옥
- 매 릴리스마다 각 크레이트 독립 publish 필요

**결론**: 처음에는 workspace path 의존, API가 안정된 후 (1.0) crates.io 공개.

---

## 테스트 격리 이점

### tasty-hooks (현재 16개 테스트)

현재 테스트 (`hooks.rs:172-290`):

```
hook_event_parse_process_exit
hook_event_parse_bell
hook_event_parse_notification
hook_event_parse_output_match
hook_event_parse_idle_timeout
hook_event_parse_unknown
hook_event_display_roundtrip
hook_event_matches_same_type
hook_event_matches_different_type
hook_event_output_match_regex
hook_manager_add_and_list
hook_manager_remove
hook_manager_remove_nonexistent
hook_manager_once_hook_removed_after_fire
hook_manager_persistent_hook_stays
```

분리 후: `cargo test -p tasty-hooks` 로 hooks만 독립 테스트 가능. 빌드 시간 2초 미만 (regex만 컴파일).

### tasty-terminal (현재 테스트 없음, 향후 추가)

분리하면 GUI 없이 헤드리스 터미널 테스트를 작성할 수 있다:

```rust
#[test]
fn terminal_echo() {
    let waker: Waker = Arc::new(|| {});
    let mut term = Terminal::new(80, 24, waker).unwrap();
    term.send_bytes(b"echo hello\n");
    std::thread::sleep(Duration::from_millis(100));
    term.process();
    let text = term.read_since_mark(true);
    assert!(text.contains("hello"));
}
```

현재는 `cargo test`가 전체 바이너리(GPU, winit 포함)를 빌드해야 하므로 CI에서 헤드리스 터미널 테스트가 어렵다. 분리하면 CI 서버(headless)에서도 터미널 테스트 실행 가능.

### 테스트 격리 요약

| 크레이트 | 기존 테스트 | 분리 후 독립 테스트 | CI 이점 |
|----------|-----------|-------------------|---------|
| `tasty-hooks` | 16개 | `cargo test -p tasty-hooks` (2초) | GPU 없이 실행 |
| `tasty-terminal` | 0개 → 추가 가능 | `cargo test -p tasty-terminal` | GUI 없이 실행 |

---

## CI/CD 영향

### 현재 (단일 크레이트)

```yaml
# CI 파이프라인
- cargo build          # 전체 빌드 (GPU 포함)
- cargo test           # 전체 테스트
- cargo clippy         # 전체 린트
```

### 분리 후 (workspace)

```yaml
# CI 파이프라인 — 변경된 크레이트만 빌드/테스트
- cargo build -p tasty-hooks     # hooks만 변경 시
- cargo test -p tasty-hooks
- cargo test -p tasty-terminal   # terminal만 변경 시
- cargo build                    # 전체 통합 빌드
- cargo test                     # 전체 통합 테스트
```

장점:
- PR에서 변경된 크레이트만 빠르게 테스트
- tasty-hooks/tasty-terminal은 GPU 없는 CI 러너에서도 테스트 가능
- 증분 빌드로 CI 시간 단축

단점:
- CI 설정 복잡도 소폭 증가 (workspace 인식 필요)
- 크레이트 간 호환성 테스트를 위해 전체 빌드도 필요

---

## 문서화 부담

### 현재

모든 `///` 문서가 `cargo doc`으로 한 번에 생성.

### 분리 후

각 크레이트가 독립적인 `cargo doc` 페이지를 가짐. crates.io 공개 시 각 크레이트에 README.md, 예제 코드 필요.

| 크레이트 | 추가 문서화 작업 |
|----------|-----------------|
| `tasty-hooks` | README.md + 사용 예제 1개 |
| `tasty-terminal` | README.md + 사용 예제 2~3개 |

부담: 경미. 두 크레이트의 API가 이미 명확하고 자기 설명적.

---

## 크레이트 간 Breaking Change 전파

### 분리 후 의존 그래프

```
tasty (바이너리)
  ├── tasty-hooks       (독립, 0 내부 의존)
  └── tasty-terminal    (독립, 0 내부 의존)
```

`tasty-hooks`와 `tasty-terminal`은 서로 의존하지 않고, tasty 바이너리만 이 둘에 의존한다. 따라서:

- `tasty-hooks`의 API 변경 → tasty 바이너리만 수정
- `tasty-terminal`의 API 변경 → tasty 바이너리 + `model.rs` 수정
- **크레이트 간 전파 없음** (서로 의존하지 않으므로)

이것이 2개만 분리하는 이유이기도 하다. 더 많이 분리하면 크레이트 간 의존이 생기고, 한 곳의 변경이 연쇄적으로 전파될 수 있다.

### 만약 7개 전부 분리했다면의 전파 경로

```
tasty-renderer → tasty-model → tasty-terminal
             \→ model::Rect (공유 타입)
tasty-ipc-server → tasty-ipc-protocol
tasty-notification → model::SurfaceId (공유 타입)
```

`tasty-terminal`의 API가 바뀌면:
1. `tasty-model` 업데이트
2. `tasty-renderer` 업데이트 (model 경유)
3. tasty 바이너리 업데이트

이런 3-depth 전파는 8,870줄 규모에서 비합리적.

---

## 종합 판정

| 후보 | 유지보수 이점 | 유지보수 비용 | 판정 |
|------|-------------|-------------|------|
| `tasty-hooks` | 테스트 격리, CI 분리 | Cargo.toml 1개 추가 | **O** |
| `tasty-terminal` | 테스트 격리, headless 테스트, CI 분리 | Cargo.toml 1개 추가 | **O** |
| `tasty-ipc-protocol` | 없음 | 크레이트 관리 오버헤드 | **△** |
| `tasty-ipc-server` | 없음 | 크레이트 관리 + 리팩토링 | **△** |
| `tasty-notification` | 테스트 격리 (이미 8개) | 크레이트 관리 | **△** |
| `tasty-settings` | 없음 | 크레이트 관리 | **X** |
| `tasty-model` | mock 터미널 주입 | 제네릭 전파 복잡도 | **△** |
| `tasty-renderer` | 독립 벤치마크 | trait 설계 유지보수 | **△** |
