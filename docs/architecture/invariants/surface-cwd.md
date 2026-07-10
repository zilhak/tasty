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

검출: `rg 'ConvertSurfaceTarget::Kind \{ cwd: None'` · `rg 'env::current_dir' crates/tasty-plugin-explorer/` · 새 impl 의 source_cwd 누락은 컴파일 실패.

## 관련

- [features/work-area](../../features/work-area/index.md) — Surface 도메인 (`source_cwd` 가 Surface trait 핵심)
- [concepts/plugins](../../concepts/plugins.md) — RemoteSurface plugin
