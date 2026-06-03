# Plugin 생태계 정책

이 문서는 Tasty plugin 시스템의 1.0 전까지의 정책을 명문화한다. 작성 형식·배포·신뢰·호환성·hot reload 5개 영역에 대해 현재 시점의 결정과 그 근거, 그리고 재검토 trigger를 적는다.

배경: 현재 동봉 plugin 7개(`tasty-plugin-explorer`, `tasty-plugin-codex`, `tasty-plugin-claude`, `tasty-plugin-image`, `tasty-plugin-clipboard-history`, `tasty-plugin-html`, `tasty-plugin-git-viewer`), 외부 plugin 0개. plugin 생태계가 자생하기 전이 정책 결정 비용 최저점이다.

## 1. 작성 형식

**결정**: 1.0까지 **Rust crate + Process entry만** 지원. WASM·Lua·기타 스크립팅은 별도 layer로 분리.

**근거**:

- **WASM 보류**: WASM의 진짜 가치는 "가벼움"이 아니라 **강제 가능한 sandbox**다. 현재 권한 모델은 IPC 호출만 게이트하고 plugin이 자기 프로세스에서 직접 `std::fs::write`/네트워크 접근하는 것은 OS process privilege로만 결정된다. WASM은 이 한계를 메울 수 있지만, **1.0 전에 보안 모델·도구체인·디버깅 인프라를 모두 갖추는 비용이 너무 크다**. 1.0 이후 사건 기반 trigger(아래 참조)가 발생하면 재검토.
- **Lua/Rhai는 별도 layer**: Tasty plugin은 매니페스트 contributes(CLI/단축키/i18n) + IPC namespace 점유 + 별 프로세스 수명주기로 묶인 "시스템 확장"이다. Lua는 problem1.md(스크립팅 층 부재)의 답이지만, "사용자 일상 커스터마이징"이라는 책임이 다르다. plugin과 스크립팅을 같은 entry 메커니즘에 묶으면 schema가 폭발하고 책임이 흐려진다. Lua는 plugin-protocol과 호환되되 host 안에 임베드되는 별 시스템으로.

**1.0 이후 재검토 trigger** (수량보다 사건):

- 외부에서 WASM 또는 비-Rust plugin 작성 요청이 2건 이상
- 권한 게이트 한계에서 비롯한 보안 이슈 1건
- 첫 외부 plugin 출시 직후 1년 운영 데이터

→ 평가 상세: [architecture/plugin-sandbox-evaluation.md](../architecture/plugin-sandbox-evaluation.md)

## 2. 배포 채널

**결정**: 1.0까지 **로컬 디렉터리 path install + 동봉 builtin**만. marketplace는 1.0 이후 RFC.

설치 흐름:

```bash
git clone <plugin-repo>        # 또는 zip checkout
cd <plugin-repo>
cargo build --release
tasty plugin install ./target/release   # 또는 매니페스트 디렉터리
```

**근거**:

- `tasty plugin install <path>`가 이미 구현되어 있다 (`docs/agent-guide/plugins.md` 설치 절). git URL fetch는 호스트가 하지 않으며 사용자가 별도로 clone한다.
- marketplace는 plugin 자생적 10+ 시점 가치. 그 전에는 비어 있는 인프라.
- crates.io는 binary plugin 배포에 부적합 (매니페스트가 별도, host 의존 path가 사용자 환경).

**1.0 이후 재검토 trigger**:

- 첫 외부 plugin 출시
- 수동 설치/업데이트 불편 사례 반복 보고
- 외부 plugin 5+개 자생

## 3. 신뢰 모델

**결정**: **매니페스트 `permissions[]` + 사용자 grant + IPC method_meta 게이트**. 추가 sandbox(seccomp/AppContainer)·서명·marketplace 검토는 1.0까지 미지원.

**한계의 명문화**:

- 모든 권한 게이트는 **IPC method 단위**로 동작한다. plugin이 자기 프로세스에서 직접 fs/network에 접근하면 OS process privilege에 의존한다.
- 따라서 매니페스트의 `permissions`는 **"호스트 API를 호출할 권한"**이지, "OS 자원에 대한 시스템 권한"이 아니다. 사용자에게 grant를 요청할 때도 이 표현을 유지한다.
- 이 한계를 투명하게 명시하는 것이 false security보다 낫다.

상세는 `docs/dev-guide/plugin-permissions.md`.

**1.0 이후 재검토 trigger**:

- 권한 오해로 인한 보안 이슈 1건
- 비-trusted plugin을 일반 사용자가 설치하는 시나리오가 현실화되는 시점

## 4. api_version과 호환성

**결정**: `HOST_API_VERSION` **메이저 매치 강제** + `crates/tasty-plugin-protocol`의 schema 변경 정책 명문화.

**호환성 규칙**:

| 변경 | 분류 |
|------|------|
| 새 메시지 타입 추가 | minor |
| 기존 메시지에 **optional + default**가 있는 필드 추가 | minor |
| 기존 메시지에 required 필드 추가 | major |
| 필드 의미 변경/제거 / 타입 변경 / nullability 변경 | major |
| 에러 코드 의미 변경 | major |
| 새 enum variant 추가 (`#[serde(other)]` 없을 때 deserialize fail 유발) | major |

새 필드는 **반드시 optional + default**만 허용한다. 이로써 minor 안에서는 호환이 유지된다.

**근거**:

- plugin은 별 OS 프로세스 + JSON 메시지이므로 ABI 호환성은 무관, JSON schema 호환성이 본질이다.
- `api_version` 메이저 매치는 호스트와 plugin 사이 충분히 강제력 있는 단순 규칙.
- minor 추가만 정책이라면 plugin은 자기가 빌드된 시점의 `api_version=1.x`를 가정해도 무방.

변경 이력은 `crates/tasty-plugin-protocol/CHANGELOG.md`에 기록한다 (별도 plan 04).

## 5. Hot reload

**결정**: 1.0까지 **seamless hot reload 미지원**. `tasty plugin disable <id>` → `tasty plugin enable <id>` 재시작을 **개발자 재시작 워크플로용 대안**으로 안내.

**근거**:

- plugin 상태는 surface별로 snapshot/restore 메커니즘이 이미 있다 (`docs/dev-guide/plugin-development.md` 5절). 재시작 후 layout 복원으로 사용자 가시 상태를 회복할 수 있다.
- 단, **layout 복원은 plugin kind가 등록된 후에만 수행된다** — disable/enable 사이에 surface는 일시적으로 missing 표시된다. seamless reload는 약속하지 않는다.
- state preservation 포함 hot reload(c)는 plugin 작성자에게 큰 부담 (모든 상태가 serializable + 마이그레이션 가능)이라 1.0 전 의무화는 과한 비용.

**1.0 이후 재검토 trigger**:

- 개발자가 plugin을 자주 재빌드하는 워크플로에서 disable/enable의 비용이 명백히 큰 사례

## 6. Built-in plugin 자동 upgrade

호스트와 함께 배포되는 built-in plugin (`com.tasty.explorer` 등) 은 사용자
디렉토리에 한 번 복사된 후에도 부팅 시점에 bundle 의 새 버전이 있으면 자동
갱신된다. 기준은 **manifest 의 `version` (semver)** 이다 — mtime 은 dist
tarball 압축 해제 시 보존되어 부정적으로 작동할 수 있어 1차 신호로 쓰지
않는다.

### 6.1 동작

부팅 시 `install_builtins_if_needed` 가 BUILTINS 각 항목에 대해 bundle 과
설치본의 manifest version 을 비교한다.

- `bundle > installed` → bundle 디렉토리를 mtime 무시하고 사용자 디렉토리에
  덮어쓴다. 옛 버전에만 있던 잔존 파일은 *제거*. 로그: `upgrading builtin
  '<id>' v<old> → v<new>`.
- `bundle == installed` → 종전 mtime 기반 sync 만 수행. dev workspace 의
  hotfix (매니페스트만 수정) 가 즉시 반영되는 경로.
- `bundle < installed` → skip. 자동 다운그레이드 금지.
- manifest 파싱 실패: bundle corrupt → skip, installed corrupt + bundle ok →
  mtime sync 로 복구.

### 6.2 수동 재설치

복구/실험 용도로 `tasty plugin upgrade-builtins [--force]` 를 제공한다.

- 기본 동작은 자동 upgrade 와 동일 (semver 기반).
- `--force` 는 동일/하위 버전도 강제 덮어쓰기. 설치본 corruption 복구 시 사용.
- 응답은 항목별 `BuiltinUpgradeReport` JSON. action: `Upgraded { from, to }` /
  `Reinstalled { version }` / `Skipped { reason }` / `NotInBundle` / `Failed`.
- 실행 중 plugin 의 binary 가 교체되면 *현재 실행 process 는 옛 binary 메모리
  를 유지* 한다 (POSIX inode 교체). 새 binary 가 효과를 보려면 `plugin
  disable` → `plugin enable` 시퀀스가 필요. Windows 에서는 실행 중 binary 의
  in-place 교체가 sharing violation 으로 실패 → `Failed` 항목으로 보고된다.

### 6.3 사용자 수정 영역

builtin plugin 디렉토리는 **host-owned** 다. 사용자가 그 안에 직접 만든
파일은 자동/수동 upgrade 가 `overwrite_builtin_dir` 로 제거할 수 있다.
사용자가 보존해야 할 상태 (grants, disabled, removed_builtins, 단축키
override) 는 builtin 디렉토리 *밖* 의 `~/.tasty/plugins.toml` 에 저장되므로
영향을 받지 않는다.

### 6.4 plugin manifest version bump 정책

자동 upgrade 메커니즘이 동작하려면 plugin 작성자가 plugin 의 의미적 변경 시
`tasty-plugin.toml::version` 을 수동 bump 해야 한다. **루트 앱의 자동 패치
+1 정책과는 분리** — plugin 단위 의미 변화 (매니페스트/permission 추가,
behavior 변경) 가 있을 때 그 plugin 의 manifest 만 별도 bump 한다. version
이 그대로면 동일 버전 분기 (mtime resync) 로 떨어지므로 dev workspace 외
에선 사용자에게 새 binary 가 노출되지 않는다.

## 부가 정책: i18n 키 충돌

plugin이 contribute하는 `lang/` 디렉터리의 키는 **plugin id prefix**를 권장한다. 예: `com.example.explorer.menu.refresh`. 호스트와 다른 plugin의 키와 충돌하면 마지막 로드가 이긴다.

1.0 시점에 prefix 강제 여부 결정. 현재는 권장이지만, 외부 plugin이 늘면 강한 규칙으로 격상한다.

## 정책 갱신

이 문서의 결정은 **시점 의존**이다. 위에서 명시한 사건 기반 trigger가 발생하면 해당 항목만 새 RFC로 재개한다. 수량 지표(plugin 10개 등)는 보조적 신호로만 쓴다.
