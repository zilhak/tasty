# ADR-0030: 번들 도구의 데이터 범위와 보존 수준을 정한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

번들 도구마다 필요한 데이터와 보존 수준이 다르다.
현재 클립보드 확인에 과거 기록 DB가 필요한 것은 아니고 임시 이미지 편집을 복원하려면 원본 경로 외의 상태가 필요하다.
문서 경로와 이미지 읽기 범위도 호스트의 시작 디렉터리나 webview 엔진 기본값에 맡기면 플랫폼마다 달라진다.

## Decision

각 도구가 필요한 데이터만 소유하고 보존 수준을 사용자 기능에 맞춘다.
clipboard viewer는 열릴 때 현재 내용을 한 번 읽으며 호스트의 polling·history·재복사 API는 제공하지 않는다.
직접 OS 읽기는 현재 비샌드박스 모델의 기능이며 매니페스트 권한만으로 이를 차단하지 않는다.

image는 원본 경로를 복원한다. 저장하지 않은 그림과 붙여넣기는 임시 상태로 취급해 복원하지 않고 별도 알림도 내지 않는다.
영구 보존은 명시적 저장으로 한다. 다른 surface의 편집 정책으로 일반화하지 않는다.

Explorer root는 공통 함수가 절대경로로 확정한다.
상대 입력은 호스트 시작 cwd로 해석하지 않고 홈으로 대체하며 홈도 찾지 못할 때만 최후 fallback을 쓴다.

git-viewer는 직렬 worker에서 활성 Repository 하나를 재사용한다.
Refresh·worktree 전환·오류에는 버리고 다시 읽으며 ref와 worktree 목록은 최신 상태를 조회한다.

markdown 로컬 이미지는 렌더러가 문서 디렉터리 안의 허용 파일만 읽어 data URI로 넣는다.
전체 HTML에 파일 읽기 권한을 주거나 base href로 상대 URL을 해석시키지 않는다.
내부 앵커는 문서 스크립트가 직접 이동하고 파일 링크는 호스트의 파일 처리 경로로 보낸다.
번들 플러그인의 외부 URL 열기도 호스트에 맡겨 소유권 검사와 debug 기록 경로를 공유한다.

## Consequences

상시 수집과 불필요한 저장을 줄이고 도구별 경로 처리도 일관되게 만든다.
그 대신 클립보드 과거 항목과 미저장 이미지 편집은 되돌릴 수 없다.
Explorer의 상대 path가 조용히 홈으로 바뀌는 제약도 남는다.

git Repository 캐시는 Windows에서 popup 동안 packfile 삭제를 방해할 수 있다.
이미지 인라인은 HTML과 IPC 크기를 늘리고 읽기 범위·크기 상한 때문에 일부 이미지를 표시하지 않는다.
raw HTML 상대 링크와 앵커의 플랫폼 동작은 검증 범위를 구별해 기록한다.
제 3자 플러그인의 직접 OS 열기를 이 경로로 강제할 수는 없다.

## Alternatives Considered

- 클립보드 수집을 plugin으로 옮겨도 상시 수집과 민감정보 누적은 그대로다.
- 이미지 자동저장은 임시 파일 수명과 복원 충돌 관리가 필요하고 도구의 목적을 바꾼다.
- webview에 파일 읽기 권한을 직접 주면 플랫폼별 범위가 달라진다.
- git ref까지 캐시하면 Refresh가 최신 정보를 보여준다는 의미가 약해진다.
- 플러그인별 OS opener는 호스트의 검증용 실행 억제 기능을 공유하지 못한다.

## Reconsideration Triggers

클립보드 history나 장시간 이미지 편집 복원이 실제 사용 목적이 되면 보존 정책을 다시 정한다.
샌드박스 도입 때 직접 OS 읽기를 재검토한다.
큰 문서 이미지의 열기 지연, Windows git 정리 실패, 상대 경로 요구가 반복되면 각각 재현해 범위를 조정한다.
플러그인에서 직접 OS 열기가 다시 추가되면 호스트 경유와 검증 방법을 확인한다.

## References

- [클립보드 뷰어](../plugins/clipboard-viewer/index.md)
- [이미지 뷰어와 임시 편집](../plugins/image/index.md)
- [Explorer 경로](../features/explorer/index.md)
- [git 조회 캐시](../plugins/git-viewer/screens/git-viewer.md)
- [Markdown 로컬 리소스](../plugins/markdown/index.md)
