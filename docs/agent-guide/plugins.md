# Plugin 시스템

Tasty는 외부 plugin을 별도 OS 프로세스로 실행하여 surface 종류를 추가할 수
있다. 호스트 ↔ plugin은 TCP + JSON 메시지로 통신한다.

> 단계 05까지 구현된 범위: plugin 디스커버리·spawn·핸드셰이크·헬스체크·생명주기 관리.
> Plugin이 surface를 그리는 동작(UI tree DSL, RemoteSurface 어댑터)은 단계 06.

## 설치 위치

| OS | Plugin 루트 |
|----|-------------|
| 모든 OS | `~/.tasty/plugins/` |

각 plugin 디렉터리:

```
~/.tasty/plugins/com.example.explorer/
  tasty-plugin.toml      # 매니페스트 (필수)
  tasty-plugin-explorer  # entry binary (또는 PATH 의존 가능)
```

비활성화/활성화 상태는 `~/.tasty/plugins.toml`에 영속화된다.

```toml
[disabled]
ids = ["com.example.broken"]
```

로그는 `~/.tasty/plugins-logs/<id>.log`에 누적 (stdout/stderr 자동 redirect).

## 매니페스트

`tasty-plugin.toml` 형식:

```toml
manifest_version = 1
id = "com.example.explorer"           # 역도메인, 전역 유일
name = "Explorer"
version = "1.2.0"
authors = ["alice@example.com"]
description = "File explorer surface for tasty"
homepage = "https://example.com/explorer"
api_version = "1"                     # 호스트 protocol 메이저 버전과 일치 필요
permissions = []                      # 단계 07에서 본격 적용

[entry]
type = "process"                      # 향후 "wasm" 추가 가능
command = "tasty-plugin-explorer"     # 매니페스트 디렉터리 기준 상대 또는 PATH
args = []

[[surface_kinds]]
kind = "explorer"                     # 소문자 + '_' + 숫자만
display_name_i18n_key = "surface.kind.explorer"
icon = "📁"

[[contributes.commands]]              # 단계 06에서 활용
id = "explorer.refresh"
title_i18n_key = "explorer.command.refresh"
default_keybinding = "F5"
```

검증 규칙 (위반 시 plugin 로드 거부):

- `manifest_version`은 정확히 `1`
- `api_version`은 호스트 버전과 일치 (현재 `"1"`)
- `id`는 역도메인 형식 (소문자 + 숫자 + `.-_`, `.` 포함 필수)
- `surface_kinds[].kind`는 소문자 + `_` + 숫자만

> **TOML 주의**: top-level 키(`permissions = [...]` 등)는 모든 `[table]` 헤더보다 *먼저* 와야 한다. 그렇지 않으면 가장 가까운 테이블 안의 키로 해석된다.

## CLI

```
tasty plugin list                          # 설치된 plugin 일람
tasty plugin install <path>                # 디렉터리를 plugins/로 복사
tasty plugin remove <id>                   # graceful shutdown + 디렉터리 삭제
tasty plugin enable <id>                   # 활성화 + spawn
tasty plugin disable <id>                  # graceful shutdown + plugins.toml 갱신
tasty plugin logs <id> [--follow]          # ~/.tasty/plugins-logs/<id>.log 출력
```

`logs`는 호스트 IPC를 거치지 않고 파일을 직접 읽는다 — 호스트가 죽었을 때도 동작.

## IPC

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `plugin.list` | 없음 | `{plugins: [{id,name,version,description,enabled,running,surface_kinds,log_path}]}` |
| `plugin.install` | `path: string` | 매니페스트 검증 후 `plugins/<id>/`로 재귀 복사 + 자동 활성화 시 spawn |
| `plugin.remove` | `id: string` | graceful shutdown + 디렉터리 삭제 |
| `plugin.enable` | `id: string` | 활성화 + spawn |
| `plugin.disable` | `id: string` | graceful shutdown |

## 생명주기 동작

- **부팅 시**: 호스트가 `~/.tasty/plugins/`를 스캔. enabled plugin 모두 spawn 시도.
- **헬스체크**: 15초마다 `ping` 송신. 60초 무응답 시 process를 강제 재시작.
- **자동 비활성화**: 10초 내 spawn 실패 3회면 사용자가 `tasty plugin enable`로 수동 재개할 때까지 정지.
- **종료 시**: 모든 plugin에 `shutdown` 메서드 송신 후 2초 timeout, timeout 시 kill.

## 보안

- 호스트는 부팅 시 `127.0.0.1:0` (랜덤 포트)로 listen.
- plugin spawn 시 환경변수로 `TASTY_HOST_IPC_PORT` + `TASTY_PLUGIN_TOKEN` 전달.
- plugin은 그 포트로 connect 후 첫 줄에 `{plugin_id, token}`을 보내야 인증 통과.
- 토큰 mismatch 시 connection을 즉시 끊는다.

## 단계 05의 한계 (단계 06+에서 해결)

- plugin이 hello 이외의 메시지(`SurfaceInvalidated`, `NotifyHost`)를 보내도 호스트는 로그만 남김 — surface 렌더링 처리는 단계 06.
- plugin 권한 모델 미적용 — plugin이 호출 가능한 IPC 제한이 단계 07.
- plugin 작성용 SDK 크레이트 (`tasty-plugin-sdk`)는 단계 08.
