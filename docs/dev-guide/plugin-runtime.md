# Plugin 런타임 계약

plugin 런타임 규칙의 위치를 안내한다. 상세 계약은 제작 가이드에 모아 관리한다. 호스트↔plugin 경계의 다른 축은 각자 문서가 있다: 권한은 [plugin-permissions](plugin-permissions.md), 렌더는 [egui-mesh-channel](egui-mesh-channel.md) · [webview](../design/systems/webview.md), 번들·배포·버전은 [plugin-packaging](plugin-packaging.md). plugin 을 **만드는 쪽**의 안내는 [plugin-development](plugin-development.md) 이고, 개별 번들 plugin 이 제공하는 동작은 [`docs/plugins/`](../plugins/index.md) 다.

## 수명주기

### 기동과 연결 대기

[제작 가이드의 기동과 연결](plugin-development.md#기동과-연결)을 따른다.
spawn·연결·hello 등록은 서로 다른 단계다.

### 종료

[종료와 재기동](plugin-development.md#종료와-재기동) 및
[호스트 종료 순서](../architecture/shutdown-sequence.md)에 요청 순서와 대기 제한이 있다.

### 재시작

[무응답과 늦은 응답](plugin-development.md#무응답과-늦은-응답)에 재시작 조건과 늦은 응답 처리가 있다.

## namespace

### 해소

[CLI와 IPC namespace](plugin-development.md#cli--ipc-namespace)에서 설치 소유권과 호출 경로를 설명한다.

### 예약

호스트 예약 prefix와 번들 예외도 같은 절에서 관리한다.

### 충돌

같은 prefix의 소유자 충돌과 CLI 이름 충돌을 구분한다. 전자는 설치를 거절하고,
후자는 충돌한 CLI 이름만 제외한다.

### 만료

namespace 요청의 만료와 연속 실패 처리는 [무응답과 늦은 응답](plugin-development.md#무응답과-늦은-응답)을 따른다.
만료 응답은 실행 취소를 뜻하지 않는다.

## 채널

### 포화

[채널 상한](plugin-development.md#채널-상한-개수--바이트--합계)에서 방향별 거절·대기를 설명한다.

### 바이트 상한

큐별 상한과 전체 합계, 빈 큐·제어 요청 예외도 같은 절에서 관리한다.

### 버린 수 통지

[큐 포화 통지](plugin-development.md#큐-포화-통지-호스트가-버린-요청을-plugin-이-안다)는 누적 개수를 알리며
어떤 요청이 버려졌는지까지 알려주지는 않는다.

### 요청 번호

요청·응답의 ID와 재시도 계약은 [API 규약](api-conventions.md)을 따른다.
호스트의 멱등 키 보장을 plugin 고유 요청에 자동 적용하지 않는다.

## 관련

- [plugin-development](plugin-development.md) — 작성과 런타임 계약
- [plugin-permissions](plugin-permissions.md) — 권한
- [plugin-packaging](plugin-packaging.md) — 번들·배포·버전
