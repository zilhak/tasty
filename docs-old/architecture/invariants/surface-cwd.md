# Surface cwd invariant

Surface 의 `cwd` 정보가 변환 / 생성 경로 전구간에서 **손실 없이 carry** 되어야 한다는 규칙. 사용자 의도와 무관한 호스트 시작 cwd (예: `cargo run` 시점의 working dir) 가 새 surface 의 cwd 행세를 하지 않도록 한다.

## 동기

Terminal(/foo/bar) → Explorer 변환 시, 기존에는 `ConvertSurfaceTarget::Kind` 가 cwd 를 carry 하지 않아 Explorer plugin 이 `params["path"]` 가 없을 때 `std::env::current_dir()` 로 fallback 했다. 결과적으로 사용자가 `cd /foo/bar` 한 터미널에서 Explorer 로 변환해도 호스트 프로세스가 시작된 dir 이 root 로 표시되는 버그가 발생했다.

## 규칙

### 1. Surface trait::source_cwd 명시 의무

`Surface::source_cwd()` 는 **default 본문 없음**. 모든 `impl Surface` 가 명시적으로 의미를 결정해야 한다 — compile-time 강제.

| impl | source_cwd 정책 |
|------|----------------|
| `TerminalSurface` | `None` 반환. cwd 는 `engine.terminals.get(id).get_cwd()` 로 store 경유 — `cwd_from_surface()` 가 분기 처리 |
| `MarkdownPanel` | 자기 file 의 parent (자체 의미 우선) |
| `ImagePanel` | 자기 file 의 parent (자체 의미 우선; file 없으면 None) |
| `EmptySurface` | carry 한 `self.cwd` (없으면 None) |
| `RemoteSurface` | `None` 반환. plugin 측에서 ctx.cwd 로 받아 자체 surface state 에 보유하지만 호스트 trait 호출에는 노출하지 않는다 — host 가 carry 한 cwd 는 `SurfaceCreateCtx.cwd` 경로로 *한 번만* 전달 |

### 2. carry 경로 강제 — `SurfaceKindDef::create` 시그니처

```rust
pub create: Arc<
    dyn Fn(SurfaceId, Option<&Path>, &serde_json::Value)
        -> anyhow::Result<Box<dyn Surface>> + Send + Sync,
>,
```

두 번째 인자가 `Option<&Path>` cwd. 모든 builtin (`terminal` / `markdown` / `image` / `diff` / `empty`) + remote plugin kind 등록자가 본 시그니처를 따른다. host 가 cwd 를 *모든* surface 생성 경로에 일관되게 주입한다.

`CoreState::create_surface_via_registry` 호출자 (5 곳) 가 cwd 를 받아 그대로 전달:

| 위치 | cwd 출처 |
|------|---------|
| `CreateWorkspace` 첫 surface | `DomainIntent.cwd` |
| `CreateTab` 새 surface | `DomainIntent.cwd` |
| `ConvertSurface { Kind }` | `ConvertSurfaceTarget::Kind.cwd` (intent handler 가 source surface 에서 resolve) |
| `SplitPane` | `DomainIntent.cwd` |
| `SplitSurface` | `DomainIntent.cwd` |

### 3. `ConvertSurfaceTarget::Kind` 에 cwd 동봉

```rust
pub(crate) enum ConvertSurfaceTarget {
    Terminal { cwd: Option<PathBuf> },
    Kind { cwd: Option<PathBuf>, kind: String, params: Value },
}
```

intent 표면 (`ConvertTarget::Kind`) 도 동일하게 cwd 를 받는다. 호출자가 명시하지 않으면 (`cwd: None`) `src/intent/surface.rs::convert` 가 source surface 에서 `resolve_inherit_cwd_from_surface` 로 carry 한다. **호출자가 임의로 `None` 으로 고정하지 않는다** — fallback 결정은 항상 intent handler 가 담당.

### 4. Plugin SDK 계약 — `SurfaceCreateCtx.cwd`

```rust
pub struct SurfaceCreateCtx {
    pub surface_id: u32,
    pub kind: String,
    pub cwd: Option<PathBuf>,
    pub params: Value,
}
```

호스트 → plugin IPC `surface.create` payload 의 top-level `cwd` 키로 직렬화. plugin SDK runtime dispatch 가 `SurfaceCreateCtx.cwd` 로 매핑. plugin 측은 우선순위 ① `params` 명시 → ② `ctx.cwd` carry → ③ 자체 fallback (예: home dir) 순으로 결정.

옛 plugin SDK (cwd 키 모름) 는 무시 — JSON-RPC 호환성 유지.

### 5. Explorer plugin fallback 정책

```rust
fn root_from_ctx(ctx: &SurfaceCreateCtx) -> PathBuf {
    ctx.params.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
        .or_else(|| ctx.cwd.clone())
        .or_else(|| std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}
```

`std::env::current_dir()` 폴백을 **제거** — 호스트 시작 cwd 가 새 surface 의 root 행세하지 않도록.

## 강제 메커니즘 요약

| 메커니즘 | 위치 | 효과 |
|----------|-----|------|
| trait default 제거 | `surface_trait.rs::source_cwd` | 새 Surface impl 추가 시 cwd 의미 명시 강제 |
| `SurfaceKindDef.create` 시그니처에 cwd 인자 | `surface_registry.rs` | 모든 builtin + plugin kind 등록자 강제 |
| `ConvertSurfaceTarget::Kind` 에 cwd 필드 | `core/intent.rs` | 변환 경로의 cwd 누락 컴파일 차단 |
| `SurfaceCreateCtx.cwd` | `tasty-plugin-sdk/src/plugin.rs` | plugin 측이 ctx.cwd 를 인지하고 자체 fallback 정책 결정 |

## 위반 검출

| 검출 항목 | 명령 |
|-----------|------|
| ConvertSurface Kind 분기에서 cwd `None` 고정 | `rg 'ConvertSurfaceTarget::Kind \{ cwd: None'` |
| explorer 의 env::current_dir 잔존 | `rg 'env::current_dir' crates/tasty-plugin-explorer/` |
| 새 Surface impl 이 source_cwd 누락 | 컴파일 자체가 실패 (default 본문 없음) |
