# Surface cwd invariant

Surface 의 `cwd` 가 변환/생성 경로 전구간에서 **손실 없이 carry** 되어야 한다는 규칙. 사용자 의도와 무관한 *호스트 시작 cwd*(예: `cargo run` 시점의 working dir)가 새 surface 의 cwd 행세를 하지 않도록 한다.

## 동기

Terminal(`/foo/bar`) → Explorer 변환 시, 예전엔 변환 타깃이 cwd 를 carry 하지 않아 Explorer 가 `std::env::current_dir()` 로 fallback 했다. 결과적으로 사용자가 `cd /foo/bar` 한 터미널에서 변환해도 호스트 프로세스 시작 dir 이 root 로 표시되는 버그가 났다.

## 규칙

### 1. `Surface::source_cwd()` 명시 의무 (default 없음)

`source_cwd()` 는 **default 본문이 없다**(`crates/tasty-model/src/surface_trait.rs`) — 모든 `impl Surface` 가 의미를 명시해야 한다(compile-time 강제).

| impl | source_cwd |
|------|-----------|
| `TerminalSurface` | `None` — cwd 는 terminal store(`get_cwd()`) 경유, `cwd_from_surface()` 가 분기 |
| `MarkdownPanel` / `ImagePanel` | 자기 file 의 parent (자체 의미 우선; file 없으면 None) |
| `EmptySurface` | carry 한 `self.cwd` (없으면 None) |
| `ExplorerPanel` | 활성 탭의 **고정 cwd**(프로젝트 루트) — 현재 폴더(current)를 하위로 오가도 스폰 cwd 는 cwd 불변. cwd↔current 분리는 [features/explorer](../../features/explorer/index.md) |
| `RemoteSurface`(plugin surface) | `None` — plugin 이 `ctx.cwd` 로 받아 자체 보유, host trait 에는 비노출. host carry 는 `SurfaceCreateCtx.cwd` 로 *한 번만* 전달 |

### 2. carry 경로 강제 — `SurfaceKindDef::create` 시그니처

```rust
pub create: Arc<dyn Fn(SurfaceId, Option<&Path>, &serde_json::Value)
    -> anyhow::Result<Box<dyn Surface>> + Send + Sync>,
```

두 번째 인자 `Option<&Path>` 가 cwd. 모든 builtin + remote plugin kind 등록자가 이 시그니처를 따라 host 가 cwd 를 *모든* 생성 경로에 일관 주입한다. `CoreState::create_surface_via_registry` 호출자(워크스페이스 첫 surface · 새 탭 · ConvertSurface · SplitPane · SplitSurface)가 cwd 를 받아 전달.

### 3. `ConvertSurfaceTarget::Kind` 에 cwd 동봉

```rust
pub(crate) enum ConvertSurfaceTarget {
    Terminal { cwd: Option<PathBuf> },
    Kind { cwd: Option<PathBuf>, kind: String, params: Value },
}
```

호출자가 명시 안 하면(`cwd: None`) intent handler(`src/intent/surface.rs::convert`)가 source surface 에서 carry 한다 — **fallback 결정은 항상 intent handler 가 담당**, 호출자가 임의로 `None` 고정 금지.

#### 3-1. mirror(원격 attach) forward 경로도 같은 불변식 대상

mirror 워크스페이스의 convert 는 로컬에서 실행되지 않고 `StructuralOp::ConvertSurface` 로 원격에 forward 된다([features/remote-attach](../../features/remote-attach/index.md)). 이 경로에서도 cwd 는 손실되지 않는다 — 우선순위는 **op 의 `cwd` > 원격 서버의 자체 resolve** 다.

| 단계 | 담당 | 값 |
|------|------|----|
| client → wire | `src/core/impl_mirror.rs` (`build_mirror_forward_op`) | intent handler 가 carry 해둔 cwd 를 `StructuralOp::ConvertSurface.cwd`(경로 문자열, `#[serde(default)]`) 로 실어 보낸다. mirror 터미널은 PTY 없는 detached 라 원격 셸의 OSC 7 이 있을 때만 값이 있다 |
| 원격 실행 | `src/core/attach_runtime.rs` (`execute_forwarded_structural_op`) | op 의 `cwd` 가 비어 있으면 `AppState::resolve_inherit_cwd_from_surface` 로 **실제 원격 PTY** 기준(OSC 7 캐시 → Linux `/proc`·macOS `proc_pidinfo`) cwd 를 직접 판정한다 |

서버측 resolve 는 로컬 convert 와 같은 헬퍼를 쓰므로 **원격 인스턴스의 `inherit_cwd` 설정 게이트를 그대로 적용**한다(실행 주체의 설정 의미론을 따르는 쪽이 로컬 실행과 대칭). `cwd` 키가 없는 구버전 client 의 op 도 이 서버측 resolve 로 커버된다.

### 4. Plugin SDK 계약 — `SurfaceCreateCtx.cwd`

```rust
pub struct SurfaceCreateCtx { pub surface_id: u32, pub kind: String, pub cwd: Option<PathBuf>, pub params: Value }
```

host→plugin IPC `surface.create` payload 의 top-level `cwd` 키로 직렬화. plugin 은 ① `params` 명시 → ② `ctx.cwd` carry → ③ 자체 fallback(예: home) 순으로 결정. 옛 SDK(cwd 키 모름)는 무시 — JSON-RPC 호환.

### 5. Explorer plugin fallback

`ctx.params["path"]` → `ctx.cwd` → `$HOME`/`$USERPROFILE` → `"."` 순. **`std::env::current_dir()` 폴백 제거** — 호스트 시작 cwd 가 root 행세 못 하게.

## 강제 / 위반 검출

| 메커니즘 | 효과 |
|----------|------|
| trait default 제거(`source_cwd`) | 새 Surface impl 추가 시 cwd 의미 명시 강제 |
| `SurfaceKindDef.create` cwd 인자 | 모든 builtin + plugin 등록자 강제 |
| `ConvertSurfaceTarget::Kind.cwd` 필드 | 변환 경로 cwd 누락 컴파일 차단 |
| `StructuralOp::ConvertSurface.cwd` 필드 | mirror forward 경로 cwd 누락 컴파일 차단 |

검출: `rg 'ConvertSurfaceTarget::Kind \{ cwd: None'` · `rg 'env::current_dir' crates/tasty-plugin-explorer/` · 새 impl 의 source_cwd 누락은 컴파일 실패.

## 관련

- [features/work-area](../../features/work-area/index.md) — Surface 도메인 (`source_cwd` 가 Surface trait 핵심)
- [concepts/plugins](../../concepts/plugins.md) — RemoteSurface plugin
