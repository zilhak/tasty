# 저장소 시스템 (Storage System)

tasty 의 영속 데이터는 **텍스트 파일과 SQLite 하이브리드**로 나뉜다. 사용자가 직접 편집·버전관리할 대상은 텍스트(TOML / 쉘 스크립트)로, 앱이 자동 누적·갱신하는 데이터는 SQLite 로 담는다.

모든 데이터는 `~/.tasty/` 아래에 모인다(Windows: `%USERPROFILE%\.tasty\`). 홈 디렉터리 결정은 `tasty_utils::path::tasty_home()`.

## 저장 위치

| 경로 | 포맷 | 내용 | 관리 주체 | 코드 |
|------|------|------|-----------|------|
| `state.db` (+ `-wal`/`-shm`) | SQLite | 최근 markdown 파일 | 앱 | `src/db.rs` |
| `memory.db` (+ `-wal`/`-shm`) | SQLite | 에이전트 메모리 (별도 스키마·연결) | 앱 | `crates/tasty-memory/` |
| `config.toml` | TOML | 사용자 설정(셸·외관·단축키·언어 등) | 사용자 | `crates/tasty-settings/` |
| `remote-profiles.toml` (+ `passkeys.toml`) | TOML | 원격 접속 프로필(`ssh`/`tasty-attach` kind) + 자격증명 — `config.toml` 과 분리해 손편집 보존 | 사용자 | `crates/tasty-remote-profiles/` |
| `file-handlers.toml` | TOML | 파일 detector / handler / 확장자 매핑 | 사용자 | `src/file/handler/` |
| `themes/<id>.toml` | TOML | 테마 (id = 파일명 stem) | 사용자 / 앱 | `crates/tasty-settings/src/appearance.rs` |
| `bashrc` / `bashrc.default` | 쉘 스크립트 | 컴파일된 빌트인 rc (tasty 모드 / default 모드) — 셸을 `--rcfile` 로 띄움 | 앱 (빌드 산출물) | `crates/tasty-settings/src/general.rs` |
| `bashrc.user` | 쉘 스크립트 | 사용자가 직접 편집하는 fragment (빌트인 사이에 끼워짐) | 사용자 | 〃 |
| `presets/{workspace,tab,pane}/<name>.toml` | TOML | 레이아웃 프리셋 (탭/페인/서피스 구조) | 사용자 / 앱 | `crates/tasty-presets/` |

- **plugin 데이터는 여기 없다.** 각 plugin 은 자기 `TASTY_PLUGIN_DATA_DIR` 아래에 보관한다 (예: explorer 의 북마크 = `<data_dir>/bookmarks.json`). host `state.db` 에 plugin 데이터를 넣지 않는다.

## SQLite `state.db`

### 단일 schema 모델

증분 마이그레이션 체인은 **없다**(0.4 fresh-start 정책). `src/db/migrations.rs`:

- `SCHEMA_VERSION` 상수 하나. `ensure_schema()` 가 `PRAGMA user_version` 을 보고 분기:
  - `0`(새 DB) → `SCHEMA_SQL` 1회 적용 + `user_version` 을 `SCHEMA_VERSION` 으로 박음.
  - `== SCHEMA_VERSION` → no-op.
  - 그 외 → `SchemaMismatch{expected, found}` 에러 → 호출자가 사용자에게 안내 후 종료.

### v1 테이블

```sql
CREATE TABLE meta (              -- 스키마 메타데이터 (key-value)
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE recent_markdown (   -- 최근 연 Markdown 경로
    path TEXT PRIMARY KEY,
    opened_at INTEGER NOT NULL
);
```

- `recent_markdown` 만 실제로 read/write 된다(`src/store/recent_files.rs`).
- 북마크·recent HTML 테이블은 없다 — explorer / html 이 plugin 으로 분리되며 host DB 에서 빠졌다.
- **경로 dedup**: 같은 파일의 다른 표기(구분자 `\`↔`/`, `\\?\` verbatim, `.`/`..`,
  Windows 대소문자 차)를 정규화 키(`strip_verbatim_prefix`+`lexically_normalize`+Windows
  case fold)로 접는다. PK 는 여전히 raw path(표시·열기용)이며 정규화 키는 비교 전용.
  `RecentFiles::add` 가 같은 키의 옛 행을 제거 후 저장하고, `load()` 는 마이그레이션 체인이
  없는 fresh-start 정책이라 로드 시 1회 정규화 dedup 패스로 기존 중복을 접는다.
- **기록 진입점**: markdown-open 이 수렴하는 인텐트 계층(`Intent::NewTab`/
  `ConvertSurface`, file-dispatch 직접 `CreateTab`)에서 `AppState::record_recent_markdown`
  로 1회 기록한다 — 파일-열기 팝업·주소창 navigate·링크 클릭이 모두 반영된다.

### 접근 규칙

- **메인 프로세스 단독 접근.** 자식 CLI 프로세스는 DB 를 직접 열지 않고 IPC 로 메인에 위임한다.
- 전역 싱글톤. `db::init()` 선행 호출 후 `db::with_db(|db| { ... })` 로 접근(미초기화면 `None`).
- PRAGMA: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `journal_size_limit`.
  - `journal_size_limit` 은 WAL 파일 크기 상한이다. **이 pragma 가 없으면 WAL 은 한 번 커진 크기를 영구히 유지한다** — SQLite 가 재사용을 위해 체크포인트 후에도 파일을 줄이지 않기 때문이다. 그러면 `wal_autocheckpoint` 임계를 영구 초과한 상태가 되어 커밋마다 체크포인트가 트리거되고 그 비용은 WAL 크기에 비례한다. 값은 임계와 정확히 같은 `tasty_memory::WAL_SIZE_LIMIT_BYTES`(= 1000 페이지 × 4096B)이며, `memory.db` 와 **같은 상수를 공유**한다 — 두 DB 는 각자의 `prepare` 를 쓰므로(`src/db.rs` vs `crates/tasty-memory/`) 한쪽만 고치면 다른 쪽이 그대로 자란다.
- 쓰기는 `Connection::transaction()` 패턴. 실패 시 `tracing::warn!`/`error!` 기록 후 진행.

### 초기화 실패 = 인메모리 폴백 없음

`db::init()` 실패는 **치명적**이다. `:memory:` 폴백을 두지 않는다 — `DbInitError` 로 분류해 사용자에게 InfoModal 로 안내한 뒤 앱을 종료한다(`src/app/window_lifecycle.rs`). variant 별로 i18n key 를 가진다:

| variant | 의미 | i18n key |
|---------|------|----------|
| `HomeDirMissing` | 홈 디렉터리 미확인 | `db_error.home_missing` |
| `PermissionDenied(path)` | 권한 거부 / CANTOPEN | `db_error.permission_denied` |
| `Busy(path)` | DB lock/busy | `db_error.busy` |
| `DiskFull` | 디스크 가득 | `db_error.disk_full` |
| `Corrupt(path)` | 손상 / NotADatabase | `db_error.corrupt` |
| `SchemaMismatch{expected,found}` | user_version 불일치 | `db_error.schema_mismatch` |
| `Other(msg)` | 그 외 | `db_error.other` |

`memory.db`(`crates/tasty-memory/`)도 같은 분류 체계(`MemoryInitError`)를 별도로 가진다.

## 텍스트 파일을 SQLite 로 옮기지 않는 이유

`config.toml` / `remote-profiles.toml` / `file-handlers.toml` / `themes/*.toml` / `bashrc.user` 는 **사용자 편집·버전관리 대상**이다. 주석·diff 추적에 텍스트가 적합하고 앱이 자동으로 덮어쓰지 않으므로 SQLite 로 옮길 이점이 없다. 반대로 최근 파일처럼 앱이 자동 누적하는 데이터는 SQLite 가 맞다.

## 백업

`state.db` 는 WAL 덕분에 단일 프로세스 종료 시점에 일관성이 보장된다. 별도 주기 백업은 만들지 않는다. 수동 복사 시 `state.db`, `state.db-wal`, `state.db-shm` 세 파일을 함께 복사한다.

**해석하지 못한 사용자 파일은 덮어쓰기 전에 보존한다.** `config.toml` 과 `layouts/NN.json` 은 앱이 다시 쓰는 파일이라, 파싱에 실패한 뒤 기본값으로 폴백하면 다음 저장이 원본을 지운다. 그래서 **저장 직전에** 원본을 `<파일명>.bak`(중복이면 `.bak.2` … `.bak.9`)으로 **rename** 해 자리를 비운 뒤 쓴다. copy 가 아니라 rename 인 이유는 원본이 자리를 떠야 이어지는 write 가 데이터를 지우지 않기 때문이다.

**손상 슬롯이 하나라도 있으면 스크롤백 GC 는 통째로 멈춘다.** `gc_scrollback_orphans_all_slots_in` 은 슬롯 하나라도 읽거나 해석하지 못하면 그 회차의 GC 를 포기하고 모든 `.bin` 을 남긴다 — 그 슬롯이 무엇을 참조했는지 모르는 채로 지우면 백업(`NN.json.bak`)에서 되살릴 때 스크롤백만 빈 채로 복원되기 때문이다("모르면 지우지 않는다"). 절충은 사용자가 손상 슬롯을 방치하는 동안 스크롤백이 계속 쌓인다는 것이다. 디스크가 왜 줄지 않는지, GC 가 왜 안 도는지 의심될 때는 `layouts/` 에 해석되지 않는 슬롯이 남아 있는지부터 본다.

**보존 시점은 로드가 아니라 저장이다.** 로드는 "해석하지 못했다" 는 사실만 값에 실어 돌려주고 파일은 그대로 둔다. 부팅 중 같은 파일을 읽는 곳이 여럿이고(설정은 런처와 GUI 가 각각, 레이아웃 슬롯은 scrollback GC 와 engine 이 각각) 그 둘은 별개 프로세스라, 읽는 쪽이 파일을 옮기면 나중에 읽는 쪽은 "파일 없음" 만 보게 된다 — 사용자에게 알릴 주체가 사건을 모르게 되고, 경합에 진 쪽이 애먼 저장 금지를 걸기도 한다.

보존이 실패했거나 **읽기가 실패한**(권한 · IO) 경우에는 파일을 건드리지 않고 그 대상에 대한 저장을 막는다. 내용을 확인하지 못한 파일을 옮기면 일시적 오류에도 사용자 데이터가 자리를 뜨기 때문이다. 공용 헬퍼는 `tasty_utils::path::preserve_corrupt_file`.

**같은 `TASTY_HOME` 을 두 인스턴스가 쓰면 창이 남는다.** 부팅 판정과 첫 저장 사이(수 분)에 다른 인스턴스가 정상 파일을 써 넣을 수 있으므로, 옮기기 직전에 파일을 다시 읽어 지금도 해석되지 않는지 확인한다. 다만 그 **재확인(read)과 옮기기(rename)는 별개 syscall 이고 사이에 잠금이 없다** — 그 사이에 끼어든 write 는 여전히 정상 파일을 `.bak` 으로 흘린다. 파일 잠금을 도입하지 않은 것은 같은 홈의 다중 인스턴스가 지원 구성이 아니고(슬롯 점유는 프로세스 안에서만 본다), 남은 창의 폭이 두 syscall 사이라 실무상 도달하기 어렵기 때문이다. 데이터가 사라지는 것이 아니라 백업 예산(9개)이 한 칸 깎이는 형태로만 드러난다.

## 테스트

- 스키마 로직: `:memory:` Connection 으로 단위 테스트(`src/db/migrations.rs` 의 `tests` — fresh init / no-op / mismatch).
- 에러 분류: `classify_sql` 단위 테스트(busy / corrupt / notadb), `user_message_i18n` key 안정성 테스트.

## 관련

- [memory.md](memory.md) — 에이전트 메모리(`memory.db`) 두 계층·소유 모델
- [`features/layout-presets`](../../features/layout-presets/index.md) — `presets/` 프리셋 적용
- [theme.md](theme.md) — `themes/*.toml` 토큰 모델
