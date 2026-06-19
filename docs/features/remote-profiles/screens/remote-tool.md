# Remote connections 창 화면 (remote_tool)

- **부모 기획**: [../index.md](../index.md)
- **트리거 위치**: [도구 메뉴](../../tools-menu/screens/tools-menu.md) `Remote connections`
- **시각 소스**: `design-system/ui_kits/terminal/overlays/remote_tool.jsx` — claude design.
- **구조**: 공통 헤더 + 상단 2탭 `[원격 접속 프로필] [Passkey]`, 각 탭이 List / Form / ConfirmDelete 라우팅. 520×460, headless.

## 트리거

도구 메뉴 `SSH profiles` 클릭 → SSH 프로필 popup.

## 레이아웃

```
┌──────────────────────────────────┐
│ SSH profiles            [+ 추가]  │
├──────────────────────────────────┤
│ ▸ prod-box   user@host:22   [✎][⌫]│  목록 — 편집/삭제
│ ▸ staging    …                   │
├──────────────────────────────────┤
│ 폼 (추가/편집):                    │
│  name / host / user / port        │
│  identity_file / remote_tasty     │
│  label / port_mode                │
│  [ 저장 ]  [ 취소 ]                │
└──────────────────────────────────┘
```

## UI 요소 인벤토리

- **프로필 목록**: 각 행 = 이름 + 요약(user@host:port) + 편집/삭제.
- **추가/편집 폼**: name · host · user · port · identity_file · remote_tasty · label · port_mode 입력 + 저장/취소.
  - (`use_agent` / `extra_options` / `remote_command` 은 폼에 없음 — 파일 직접 편집.)
- **검증 에러**: 이름 빈 값/중복, host 빈 값, port 형식, 저장 실패 메시지.

## 상태별 시각

- **목록 / 추가 / 편집**: 폼은 추가·편집 시 표시(편집이면 기존 값 채움).
- **검증 에러**: 입력 오류 시 해당 메시지.

## 시각 소스

`design-system/ui_kits/terminal/overlays/ssh_tool.jsx` — 창·목록·폼 배치의 단일 출처. (design-system vendor 시 SSH 도구 디자인 파일 존재 여부 확인 후 resolve.)
