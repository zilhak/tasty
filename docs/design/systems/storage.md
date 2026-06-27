# 저장소 시스템 (Storage System)

tasty 의 영속 데이터는 **텍스트 파일과 SQLite 하이브리드**로 나뉜다. 사용자가 직접 편집·버전관리할 대상은 텍스트(TOML / 쉘 스크립트)로, 앱이 자동 누적·갱신하는 데이터는 SQLite 로 담는다.

모든 데이터는 `~/.tasty/` 아래에 모인다(Windows: `%USERPROFILE%\.tasty\`). 홈 디렉터리 결정은 `tasty_utils::path::tasty_home()`.

## 저장 위치

| 경로 | 포맷 | 내용 | 관리 주체 | 코드 |
|------|------|------|-----------|------|
| `state.db` (+ `-wal`/`-shm`) | SQLite | 최근 markdown 파일 | 앱 | `src/db.rs` |
| `memory.db` (+ `-wal`/`-shm`) | SQLite | 에이전트 메모리 (별도 스키마·연결) | 앱 | `crates/tasty-memory/` |
| `config.toml` | TOML | 사용자 설정(셸·외관·단축키·언어 등) | 사용자 | `crates/tasty-settings/` |
| `ssh-profiles` (별도 파일) | TOML | SSH 프로필 — `config.toml` 전체 덮어쓰기와 분리해 손편집 보존 | 사용자 | `crates/tasty-ssh-profiles/` |
| `file-handlers.toml` | TOML | 파일 detector / handler / 확장자 매핑 | 사용자 | `src/file/handler/` |
| `themes/<id>.toml` | TOML | 테마 (id = 파일명 stem) | 사용자 / 앱 | `crates/tasty-settings/appearance.rs` |
| `bashrc` / `bashrc.default` | 쉘 스크립트 | 컴파일된 빌트인 rc (tasty 모드 / default 모드) — 셸을 `--rcfile` 로 띄움 | 앱 (빌드 산출물) | `crates/tasty-settings/general.rs` |
| `bashrc.user` | 쉘 스크립트 | 사용자가 직접 편집하는 fragment (빌트인 사이에 끼워짐) | 사용자 | 〃 |
| `presets/{workspace,tab,pane}/<name>.toml` | TOML | 레이아웃 프리셋 (탭/패인/서피스 구조) | 사용자 / 앱 | `crates/tasty-presets/` |

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

### 접근 규칙

- **메인 프로세스 단독 접근.** 자식 CLI 프로세스는 DB 를 직접 열지 않고 IPC 로 메인에 위임한다.
- 전역 싱글톤. `db::init()` 선행 호출 후 `db::with_db(|db| { ... })` 로 접근(미초기화면 `None`).
- PRAGMA: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`.
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

`config.toml` / `ssh-profiles` / `file-handlers.toml` / `themes/*.toml` / `bashrc.user` 는 **사용자 편집·버전관리 대상**이다. 주석·diff 추적에 텍스트가 적합하고 앱이 자동으로 덮어쓰지 않으므로 SQLite 로 옮길 이점이 없다. 반대로 최근 파일처럼 앱이 자동 누적하는 데이터는 SQLite 가 맞다.

## 백업

`state.db` 는 WAL 덕분에 단일 프로세스 종료 시점에 일관성이 보장된다. 별도 주기 백업은 만들지 않는다. 수동 복사 시 `state.db`, `state.db-wal`, `state.db-shm` 세 파일을 함께 복사한다.

## 테스트

- 스키마 로직: `:memory:` Connection 으로 단위 테스트(`src/db/migrations.rs` 의 `tests` — fresh init / no-op / mismatch).
- 에러 분류: `classify_sql` 단위 테스트(busy / corrupt / notadb), `user_message_i18n` key 안정성 테스트.

## 관련

- [memory.md](memory.md) — 에이전트 메모리(`memory.db`) 두 계층·소유 모델
- [`features/layout-presets`](../../features/layout-presets/index.md) — `presets/` 프리셋 적용
- [theme.md](theme.md) — `themes/*.toml` 토큰 모델
