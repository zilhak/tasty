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

### 5. Explorer root fallback (host builtin)

Explorer 는 plugin 이 아니라 본체 builtin surface 다(`register_explorer` — `src/core/surface_registry/builtins.rs`). **root 는 어떤 경로로 생성되든 항상 절대경로다.** 결정 순서:

1. `params["path"]`
2. `SurfaceKindDef::create` 의 carry cwd (§2)
3. `$HOME` / `%USERPROFILE%`
4. (홈 조회 실패 시) 프로세스 cwd 를 **절대경로로 확정**해서
5. (그것도 실패 시) 파일시스템 루트

1·2 의 값이 **상대경로면 채택하지 않고** 3 단계로 내려간다. 상대 root 를 프로세스 cwd 기준으로 절대화하는 선택지는 이 불변식이 금지한 "호스트 시작 cwd 가 root 행세" 를 그대로 되살리므로 채택하지 않았다. `"."` 를 root 로 두는 것은 `std::env::current_dir()` 폴백을 **지연 평가**하는 것과 동작상 같으면서, 그 문자열이 주소창·경로 복사·attach `list_dir` wire 로 새어나가므로 더 나쁘다.

4 단계(프로세스 cwd)는 홈 조회가 실패하는 환경(HOME 없는 컨테이너 등)의 최후 수단이다 — 생성 시점에 절대경로로 확정하므로 상대경로가 UI·wire 로 새지 않는다.

같은 규칙이 **snapshot 복원**(`explorer_tab_from_json`)에도 적용된다 — `root` 키가 없거나 값이 상대경로인 구 `layout.json`(과거 폴백이 저장한 `"."` 포함)은 복원 시 홈으로 교정된다. 상대 `cwd` 키는 (이미 절대로 확정된) current 를 따른다.

구현의 단일 진실원천은 `tasty_model::explorer_panel::{default_root, resolve_root}` 이고, 생성(`create`)·복원(`explorer_tab_from_json`)·빈 탭 목록 복원(`ExplorerPanel::from_tabs`) 세 경계가 모두 이를 호출한다.

주의: 이 폴백은 "cwd 가 애초에 주어지지 않았을 때" 의 방어선이지 cwd carry(§2·§3)의 대체가 아니다. carry 할 cwd 가 있는데 전달하지 않아 홈으로 떨어지는 것은 여전히 해당 생성 경로의 버그다.

## 강제 / 위반 검출

| 메커니즘 | 효과 |
|----------|------|
| trait default 제거(`source_cwd`) | 새 Surface impl 추가 시 cwd 의미 명시 강제 |
| `SurfaceKindDef.create` cwd 인자 | 모든 builtin + plugin 등록자 강제 |
| `ConvertSurfaceTarget::Kind.cwd` 필드 | 변환 경로 cwd 누락 컴파일 차단 |
| `StructuralOp::ConvertSurface.cwd` 필드 | mirror forward 경로 cwd 누락 컴파일 차단 |
| explorer root 회귀 테스트 | `builtins.rs` 의 `explorer_create_*` / `explorer_restore_normalizes_relative_snapshot_root` · `explorer_panel.rs` 의 `default_root_is_always_absolute` / `resolve_root_*` — 상대 root 가 생성·복원 경계를 통과하면 실패 |

검출: `rg 'ConvertSurfaceTarget::Kind \{ cwd: None'` · `rg 'PathBuf::from\("\."\)' src/core/surface_registry crates/tasty-model/src/explorer_panel.rs` · 새 impl 의 source_cwd 누락은 컴파일 실패.

## 관련

- [features/work-area](../../features/work-area/index.md) — Surface 도메인 (`source_cwd` 가 Surface trait 핵심)
- [concepts/plugins](../../concepts/plugins.md) — RemoteSurface plugin
