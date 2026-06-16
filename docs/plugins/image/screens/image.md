# Image surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: host-rendered (egui + 텍스처) — `design-system/` 의 image surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 이미지 뷰어 / 그림판 surface.

## 트리거

이미지 파일 열기, `image` surface 생성/전환, 또는 빈 캔버스.

## UI 요소 인벤토리

- **이미지 뷰** — 로드된 이미지 표시(맞춤/확대 등).
- **빈 캔버스** — 파일 없이 시작한 그림판.
- 탭 표시명은 파일명(빈 캔버스면 기본 "Image").

## 상태별 시각

- 로드됨 / 빈 캔버스 / 로드 실패.

## 시각 소스

host-rendered 이므로 픽셀은 host 렌더 + 테마 토큰. design-system vendor 후 링크로 교체.
</content>
