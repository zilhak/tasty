# 저장소 시스템 (Storage System)

Tasty의 영속 데이터는 **텍스트 파일과 SQLite 하이브리드**로 구성된다. 사용자가 편집·버전관리할 대상은 텍스트로, 앱이 자동으로 누적·갱신하는 데이터는 SQLite에 담는다.

## 저장 위치

모든 데이터는 `~/.tasty/` 아래에 모인다. (Windows: `%USERPROFILE%\.tasty\`.)

| 경로 | 포맷 | 내용 | 관리 주체 |
|------|------|------|-----------|
| `config.toml` | TOML | 사용자 설정(쉘, 외관, 단축키, 언어, 클립보드 히스토리 옵션 등) | 사용자 |
| `bashrc` | 쉘 스크립트 | Tasty 모드에서 source 되는 파생 스크립트 | 앱 (빌드 산출물) |
| `bashrc.user` | 쉘 스크립트 | 사용자가 직접 편집하는 영역 | 사용자 |
| `state.db` | SQLite | 북마크, 최근 파일, 클립보드 히스토리(향후) 등 | 앱 |
| `state.db-wal`, `state.db-shm` | SQLite | WAL 저널/공유 메모리 보조 파일 | 앱 (자동) |
| `bookmarks.json.bak`, `recent_files.json.bak` | JSON | 이전 버전에서 이관된 원본(참고 보관) | 앱 (1회성) |

## SQLite state.db

### 스키마 버전

`PRAGMA user_version`으로 추적. 앱 시작 시 `storage::init()` → `migrations::run()`이 현재 버전을 보고 필요한 마이그레이션을 순차 적용한다.

### v1 테이블

```sql
-- 스키마 메타데이터 (예: *_json_migrated 플래그)
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Explorer 북마크
CREATE TABLE bookmarks (
    path TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL  -- unix seconds
);

-- 최근 연 Markdown 파일 경로
CREATE TABLE recent_markdown (
    path TEXT PRIMARY KEY,
    opened_at INTEGER NOT NULL
);

-- 최근 연 HTML URL
CREATE TABLE recent_html (
    url TEXT PRIMARY KEY,
    opened_at INTEGER NOT NULL
);

-- 클립보드 히스토리 (스키마 자리 확보; 실제 write 연결은 별도 작업)
CREATE TABLE clipboard_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,       -- 'text' | 'image'
    text TEXT,                -- kind='text'
    data BLOB,                -- kind='image'
    source TEXT NOT NULL,     -- 'system' | 'internal'
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_clipboard_history_created_at
    ON clipboard_history(created_at DESC);
```

### 접근 규칙

- **메인 프로세스 단독 접근.** 자식 CLI 프로세스는 DB를 직접 열지 않고 IPC로 메인에 위임한다.
- 코드는 `crate::storage::with_db(|db| { ... })`로 싱글톤에 접근한다. `init()`이 선행 호출되어야 한다.
- DB 쓰기는 권장 패턴으로 `Connection::transaction()`을 쓴다. 실패 시 `tracing::warn!`·`error!`로 기록하고 진행한다.
- PRAGMA: WAL(`journal_mode=WAL`), `synchronous=NORMAL`, `foreign_keys=ON`.

### 실패 경로

`state.db`를 열지 못하면 `:memory:` DB로 폴백한다. 이 세션에서는 북마크/최근파일이 저장되지 않지만 기능 자체는 동작한다. `tracing::error!`로 원인이 기록된다.

## JSON → SQLite 1회성 이관

구 버전의 `bookmarks.json` / `recent_files.json`은 첫 실행 시 자동 이관된다.

1. `state.db`를 열고 마이그레이션을 돌린다.
2. `meta` 테이블에 `bookmarks_json_migrated` / `recent_files_json_migrated` 플래그가 없고 해당 JSON 파일이 존재하면:
   - JSON 내용을 파싱해 각 레코드를 테이블에 `INSERT OR IGNORE`(기존 DB 데이터 우선).
   - 커밋 성공 후에만 원본을 `*.bak`로 rename.
3. 플래그가 이미 있으면 잔존 JSON 파일은 그냥 `.bak`로 rename하고 끝낸다.

DB 커밋 실패 시 원본 JSON은 건드리지 않는다 — 복구 여지를 남긴다.

## 설정은 이관하지 않는 이유

`config.toml`은 **사용자 편집·버전관리 대상**이다. 주석을 달거나 diff로 변경 이력을 추적하기에 텍스트가 훨씬 적합하다. 앱이 자동으로 덮어쓰지 않는 이상 SQLite로 옮길 이점이 없다. `bashrc.user` / `bashrc`도 동일한 이유로 텍스트 파일.

## 백업

`state.db`는 WAL 덕분에 단일 프로세스 종료 시점에 일관성이 보장된다. 별도 주기 백업은 만들지 않는다. 사용자가 수동 복사를 원하면 `state.db`, `state.db-wal`, `state.db-shm` 세 파일을 함께 복사하면 된다.

## 테스트

- 마이그레이션 로직은 `:memory:` Connection으로 단위 테스트(`storage::migrations::tests`).
- 도메인 저장 로직은 `storage::init_with(Db::open_in_memory())`로 싱글톤을 주입할 수 있음(테스트 전용 API).
