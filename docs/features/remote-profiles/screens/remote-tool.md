# Remote connections 창 화면 (remote_tool)

- **부모 기획**: [../index.md](../index.md)
- **트리거 위치**: [도구 메뉴](../../tools-menu/screens/tools-menu.md) `Remote connections`
- **시각 소스**: `design-system/ui_kits/terminal/overlays/remote_tool.jsx` — claude design.
- **구조**: 공통 헤더 + 상단 2탭 `[원격 접속 프로필] [Passkey]`, 각 탭이 List / Form / ConfirmDelete 라우팅. 520×460, headless.

## 트리거

도구 메뉴 `Remote connections` 클릭 → 원격 접속 popup.

## 레이아웃

```
┌──────────────────────────────────┐
│ Remote connections          [×]   │  공통 헤더
├──────────────────────────────────┤
│ [원격 접속 프로필] [Passkey]       │  상단 2탭
├──────────────────────────────────┤
│ 프로필 목록         [+ 추가][⤓필터]│  add-bar
│ ▸ prod-box   user@host:22   [✎][⌫]│  목록 — 편집/삭제
│ ▸ staging    …                   │
├──────────────────────────────────┤
│ 폼 (추가/편집):                    │
│  name / host / user / port        │
│  label / remote_tasty             │
│  shell ▾ / passkey ▾              │
│  [ 저장 ]  [ 취소 ]                │
└──────────────────────────────────┘
```

## UI 요소 인벤토리

- **프로토콜 필터** (원격 접속 프로필 탭 전용): add-bar 우측의 `Filter` 버튼(funnel 아이콘). 현재 프로필에 존재하는 프로토콜(`kind`)이 2종 이상일 때만 표시. 클릭 시 체크박스 드롭다운(프로토콜 목록 + `모두 선택`/`모두 해제`/`초기화`/`적용`). Apply-on-confirm(적용 눌러야 반영), 선택된 프로토콜만 목록에 표시. 결과 0건이면 "선택한 프로토콜에 해당하는 프로필이 없습니다" 빈 상태. 필터 상태는 **세션 한정·비영속**(popup 재오픈에는 유지, tasty 재시작 시 전체 선택으로 리셋).
- **프로필 목록**: 각 행 = 이름 + 요약(user@host:port) + 편집/삭제.
- **추가/편집 폼**: name · host · user · port · label · remote_tasty(원격 tasty 경로) 텍스트 입력 + **shell**(콤보박스, 선택 가능 셸 목록 + `auto`) + **passkey**(저장된 passkey 선택 드롭다운, `passkey_ref`) + 저장/취소. (라벨 키 `field_name`/`field_host`/`field_user`/`field_port`/`field_label`/`field_remote_tasty`/`field_shell`/`field_passkey`.)
  - **인증 = passkey**: GUI 폼에 `identity_file` 입력은 없다 — passkey 를 고르면 실제 ssh `-i` 경로는 내부에서 `passkey_ref → Passkey.path` 로 resolve 한다(`crates/tasty-cli/src/ssh.rs`).
  - **`port_mode` 는 폼 입력이 아니다**: shell 선택에서 자동 도출되는 내부 필드(`shell_to_port_mode`). `auto` 면 저장 후 워커가 감지해 채운다.
  - (`use_agent` / `extra_options` / `remote_command` 은 폼에 없음 — 파일 직접 편집.)
- **검증 에러**: 이름 빈 값/중복, host 빈 값, port 형식, 저장 실패 메시지.

### 폼 레이아웃 (디자인 `ProfileForm`/`PasskeyForm` 구조 전사)

- **2컬럼 행** `[112px 1fr]`: 모든 행(Type 포함)이 고정폭 112 라벨 컬럼(우측정렬, `subtext0`, 13px) + columnGap `space-md`(12) + 입력(1fr). `egui::Grid` 의 컬럼 협상이 라벨 폭을 붕괴시켜 truncate 되던 문제 때문에 수동 `ui.horizontal` 2컬럼(`form_row`)으로 통일했다. 행 간 rowGap `space-sm`(8).
- **본문/footer 분리**: 본문은 `rtScrollPad`(flex:1) = `CentralPanel` + `ScrollArea`(가용 높이를 채움)로, footer 는 `rtFooter`(flex:none) = `TopBottomPanel::bottom` 으로 패널 하단에 고정. footer 위 separator 는 팝업 전체폭(`clip_rect`)에 그어지고 버튼만 패딩으로 들여쓴다.
- **footer**: 우측정렬 `[취소 ghost][저장 primary]`, padding `space-md`/`space-lg`. passkey 폼도 동일.
- **패딩**: 폼 좌우 `space-lg`(16). (리스트 뷰는 14 — 폼 뷰일 때만 외곽 콘텐츠 margin 0 으로 두고 폼이 패딩을 소유한다.)
- generic(비-ssh) 폼의 커스텀 필드 행은 디자인 `[112 1fr control-height(28)]` grid 정렬(key 112 / value 1fr / 삭제 버튼 28).

## 상태별 시각

- **목록 / 추가 / 편집**: 폼은 추가·편집 시 표시(편집이면 기존 값 채움).
- **검증 에러**: 입력 오류 시 해당 메시지.

## 시각 소스

`design-system/ui_kits/terminal/overlays/remote_tool.jsx` — 창·2탭·목록·폼 배치의 단일 출처. (design-system vendor 시 원격 접속 도구 디자인 파일 존재 여부 확인 후 resolve.)
