# 이미지 뷰어 & 그림판

- **Status**: Implemented

`com.tasty.image` 번들 plugin이 제공하는 image surface kind. plugin이 surface kind
등록(`rendering = "host"`)과 `image.*` IPC 네임스페이스를 점유하고, 픽셀 렌더링과
편집은 호스트 본문이 직접 담당한다. plugin이 비활성화되면 image surface 항목은
convert popup / pane context menu에서 사라진다.

### 뷰어 기능
- **이미지 표시**: PNG, JPEG, BMP, WebP, ICO, TIFF 포맷 로드 및 egui 텍스처 렌더링
- **폴더 내 탐색**: 같은 디렉토리의 이미지 파일을 자동 인식하여 이전(◀)/다음(▶) 버튼으로 이동
- **새로고침**: 디스크에서 이미지를 다시 로드
- **줌**: 마우스 휠로 확대/축소 (0.1x ~ 20x), 줌 1.0 이하에서는 fit-to-window
- **팬**: 확대 상태에서 드래그로 위치 이동
- **더블 클릭**: 줌 리셋 (1.0으로 복귀)

### 그림판 기능
- **편집 모드 토글**: 편집 버튼(✏)으로 드로잉 모드 진입
- **연필 드로잉**: 마우스 드래그로 자유 곡선 그리기 (Bresenham 알고리즘 기반)
- **브러시 조절**: 크기 슬라이더 (1~20px), 색상 선택기
- **저장**: 원본 + 오버레이 합성 후 PNG로 저장 (원본 포맷 무관)
- **취소**: 편집 내용 폐기, 원본으로 복귀
- **새 이미지**: 빈 이미지 surface는 800×600 흰 캔버스가 채워진 상태로 즉시 시작하며, 별도의 크기 입력 단계를 거치지 않는다. 사용자가 다른 크기로 새 캔버스를 만들고 싶을 때만 `+` 버튼으로 크기 입력 팝업을 띄울 수 있다 (팝업 기본값도 800×600).

### 저장 규칙
- 저장 형식은 항상 PNG
- 기존 파일 편집 시: 같은 경로에 `.png` 확장자로 저장
- 새 이미지: 사용자가 파일 경로 지정

### CLI/IPC

`tasty image <sub>` CLI (plugin `[[contributes.cli]]`이 노출):

| 서브커맨드 | IPC 메서드 | 설명 |
|------------|------------|------|
| `tasty image open <path> --surface ID` | `image.open` | surface를 image kind로 변환하고 파일 로드 |
| `tasty image save --surface ID [--path PATH]` | `image.save` | 현재 image surface를 PNG로 저장 (path 생략 시 원본 경로의 `.png`) |
| `tasty image export <path> --surface ID` | `image.export_png` | 명시한 경로로 PNG 내보내기 |
| `tasty image next --surface ID` | `image.next` | 같은 폴더의 다음 이미지로 이동 |
| `tasty image prev --surface ID` | `image.prev` | 이전 이미지로 이동 |
| `tasty image paste --surface ID` | `image.paste` | 클립보드의 이미지를 floating selection으로 붙여넣기 |
| `tasty image list` | `image.list` | 열려 있는 모든 image surface의 ID + 경로 |

기존 호스트 CLI도 그대로 동작 (변환/생성 경로):
- `tasty split --type image --file <path>`: 이미지 뷰어로 분할
- `tasty split --type image`: 새 이미지 (빈 캔버스)
- `tasty new tab --pane ID --type image --file <path>`: 이미지 탭 생성
- `tasty new workspace --type image --file <path>`: 이미지 워크스페이스 생성
- Surface 타입 변환 팝업에서 Image 옵션 선택 가능

### 닫기/복원
- 이미지 탭 닫기 시 ClosedItem에 surface kind + snapshot이 저장됨 (generic 경로)
- Ctrl+Shift+T로 복원 시 surface registry의 image kind `restore`가 호출되어 같은 이미지를 다시 로드
