//! 길이 생성자와 등록된 egui 기하 호출에서 size-* 값과 같은 숫자 리터럴을 센다.
//! 길이 타입 사용 여부를 검사하는 length_constant_frontier와 달리, 이름 있는 토큰 대신 숫자를 쓴 후보를 찾는다.
//! 반경·폰트 크기는 다른 스케일이므로 여기서 검사하지 않는다. 토큰 목록은 생성 파일에서 읽는다.
//!
//! 0, clamp·비교의 1, 등록된 정규화 좌표·전시용 값, test 전용 코드와 토큰 선언은 별도로 집계한다.
//! 지역 변수의 숫자는 길이 문맥이 없으면 찾지 못하며 스케일 밖 값도 검사하지 않는다.
//! 숫자 양옆의 곱셈·나눗셈은 배율로 추정해 제외한다. 이름·텍스트 문맥만으로 실제 단위를 입증하지는 못한다.
//!
//! 영역별 남은 수는 정확히 맞아야 한다. 줄면 기록도 낮추고, 늘면 실제 새 사용·스케일 확대·
//! 수집 방식 변경을 구별한다. 당장 토큰을 쓸 수 없다면 역할과 남긴 이유를 해당 선언에 적는다.
//! 값이 같다는 이유만으로 역할이 다른 토큰을 적용하거나 스케일 밖 값을 반올림해서는 안 된다.
//! 기준은 [ADR-0035]와 [ADR-0039]를 따른다.
//!
//! [ADR-0035]: ../../docs/adr/0035-shared-design-and-theme.md
//! [ADR-0039]: ../../docs/adr/0039-typed-length-and-dpi-boundaries.md

use super::test_gate::blank_test_modules;
use super::{mask_non_code, repo_root, rust_sources};

const SIZE_SCALE_SOURCE: &str = "crates/tasty-design-tokens/src/generated/primitive.rs";

const LEN_CTOR: &[&str] = &["LogicalPx", "PhysicalPx"];

/// 길이 후보를 찾을 egui/emath 호출 이름. 목록 밖의 호출은 검사하지 않는다.
const EGUI_LENGTH_HEADS: &[&str] = &[
    "vec2",
    "pos2",
    "splat",
    "shrink",
    "expand",
    "expand2",
    "from_min_size",
    "from_min_max",
    "add_space",
    "max_height",
    "min_height",
    "max_width",
    "min_width",
    "desired_width",
    "exact_width",
    "fixed_size",
    "at_least",
    "at_most",
    "set_width",
    "set_height",
    "set_min_width",
    "set_min_height",
    "set_max_width",
    "set_max_height",
    "set_min_size",
    "size",
    "inner_margin",
    "outer_margin",
    "symmetric",
    "same",
    "circle",
    "circle_filled",
    "circle_stroke",
];

/// 다른 사용처가 참조할 값을 정의하는 파일은 제외한다. 제외한 자리의 상한은 별도 검사한다.
const DECLARATION_SITES: &[(&str, usize, &str)] = &[
    (
        "crates/tasty-design-tokens/src/generated/",
        // size-540·700·1100 선언과 letter-spacing-n1px 의 LogicalPx(-1.0)(값 1로 읽힌다)이 네 자리를 더한다.
        // size-20·40 선언 두 자리와, 값이 스케일에 들어온 기존 font-size-20 선언 한 자리를 더한다.
        // 상태 점 고리 굵기의 primitive size-1-5 선언 LogicalPx(1.5) 한 자리를 더한다.
        64,
        "스케일 자신 — 이 파일이 곧 size-* 의 정본이다",
    ),
    (
        "crates/tasty-type-appearance/src/theme.rs",
        // 스케일에 size-20이 들어오면서 font_size_prose_h1 의 기존 값 20이 새로 집계됐다.
        // 점선 무늬 border_dash·border_dash_gap 의 기본값 4 두 개가 더해졌다.
        // 레일 점 고리 굵기 status_dot_ring_width 의 기본값 1.5 와 배율 무관 단언 두 자리가 더해졌다.
        58,
        "Theme 의 값표 — 다른 자리가 참조해야 할 이름(border_width 등)이 여기 산다",
    ),
];

/// 전시용 값을 (파일, 호출 이름, 상한, 사유)로 등록한다. 줄 번호는 무관한 편집에도 바뀌므로 사용하지 않는다.
const DISPLAY_SPECIMENS: &[(&str, &str, usize, &str)] = &[(
    "crates/tasty-gallery/src/catalog/components/prim_spinner.rs",
    "size",
    // 스케일에 size-20·40이 들어오면서 스피너 크기 견본 하나가 새로 집계됐다.
    6,
    "스피너를 여러 크기로 보여주는 것이 이 카드의 목적이다 — 토큰으로 바꾸면 전시가 사라진다",
)];

/// 픽셀이 아닌 정규화 좌표를 파일·호출 이름별로 등록하고 상한을 둔다.
const UNIT_SPACE_SITES: &[(&str, &str, usize, &str)] = &[(
    "crates/tasty-ui-widgets/src/popup_title.rs",
    "pos2",
    2,
    "popup 타이틀바 글리프 텍스처 UV — 전체 텍스처를 가리키는 `pos2(1.0, 1.0)` 은 100% 다",
)];

fn is_in_unit_space(hit: &Hit) -> bool {
    UNIT_SPACE_SITES
        .iter()
        .any(|(path, head, _, _)| hit.rel == *path && hit.head == *head)
}

/// 영역별 상한과 사유. 수가 상한을 넘으면 실패하고, 줄어든 것은 상한을 고치지 않아도 된다.
/// 기록은 정확한 현재 수가 아니라 상한이다([검사 대상 관리](../../docs/dev-guide/guard-population.md)).
const AREAS: &[(&str, usize, &str)] = &[
    (
        "src/adapters/ui/popup/",
        // 명령 팔레트 폭은 정의와 등록 시점의 기본 크기표 모두 palette-width 토큰을 읽어 집계에서 빠졌다.
        // 포트 스캐너 열 하한 140·120·140은 공용 위젯의 ports_table로 옮겨 빠졌다.
        44,
        // popup의 기본 크기·열 최소폭·스크롤 상한 중 대응하는 역할의 토큰이 없는 값이 남아 있다.
        // 같은 숫자의 폭·점 크기 토큰을 높이·간격에 대신 쓰지 않는다.
        // 스케일에 size-140·360·440·620이 들어오면서 기존 popup 크기표의 360·440·140이 새로 집계됐다.
        // transfer popup의 여백·줄 높이 상수는 transfer-* 토큰과 줄 높이 계산으로 옮겨 집계에서 빠졌다.
        // 스케일에 size-20·40이 들어오면서 기존 값 20·40 세 자리가 새로 집계됐다.
        // 탐색기 Properties 의 여백 14와 줄 20·최소 24·위 2는 explorer-props-* 토큰으로 옮겨 빠졌다.
        "popup 기본 크기표 — vec2(400.0, 320.0) 처럼 정의 옆에 값이 그대로 박혀 있다",
    ),
    (
        "src/adapters/ui/",
        // 본체 chrome의 역할별 치수다. 테마 접근자가 생긴 값은 옮기되 같은 숫자의 다른 역할과 혼동하지 않는다.
        // 튜토리얼 popup 360·탐색기 열 140 같은 기존 값은 새 스케일 값이라 집계된다.
        // 알림 popup 기본 높이 400은 대응 토큰이 없어 notification.rs 의 sizer 에 남는다(폭은 토큰).
        // 스케일에 size-10이 들어오면서 기존 값 10이 새로 집계됐다.
        // 사이드바 접기 버튼이 공용 IconButton sm 을 쓰면서 24×24 두 자리가 빠졌다.
        // 탐색기 상태 화면 보조 줄 최대 폭 200은 시안 ExpState 의 raw maxWidth 라 대응 토큰이 없다.
        // 마우스 캡처 배너 메뉴 폭은 banner-more-menu 토큰으로 옮겨 빠졌다.
        // 스케일에 size-20·40이 들어오면서 기존 값 20·40 여섯 자리가 새로 집계됐다.
        // 도구 메뉴 폭 160은 tools-menu-min-width·max-width 토큰과 내용 폭 계산으로 옮겨 빠졌다.
        // 도구 메뉴 행 높이 28은 공용 menu_item 의 menu-item-height 토큰으로 옮겨 빠졌다.
        // 탐색기 미리보기 패널 머리 높이 40은 explorer-preview-header-height 토큰으로 옮겨 빠졌다.
        // 탐색기 상태 화면의 보조 줄 최대 폭 200은 공용 state_screen 으로 옮겨 빠졌다.
        // 탐색기 상세 표 열 폭은 공용 explorer_columns 로 옮겨 두 자리가 빠졌다.
        // DAG 빈 상태 글리프 24는 공용 dag_empty 로 옮겨 빠졌다.
        51,
        "나머지 host chrome(사이드바·타이틀바·서피스 장식)",
    ),
    (
        "src/view/",
        // 설정·플러그인 화면에서 역할에 맞는 토큰이 아직 없는 치수와 폰트 값이 남아 있다.
        // 스케일에 size-10이 들어오면서 기존 값 10이 새로 집계됐다.
        // 플러그인 추가 경로 입력이 공용 view로 옮겨 가며 입력 폭 90 빼기가 없어졌다.
        // 스케일에 size-20·40이 들어오면서 기존 값 20·40 두 자리가 새로 집계됐다.
        // 단축키 가져오기 표 선택 열·원래 조합 열은 kb-ie-* 토큰으로 옮겨 빠졌다.
        // 단축키 Plugins 서브탭이 공용 view로 옮겨 가며 키 입력칸 폭 180이 빠졌다.
        // 설정 행이 공용 SettingsRow 로 모이며 원격 전송 라벨 열 150과 단축키 탭 라벨 gap 12 셋이 빠졌다.
        // 플러그인 Attention 이 공용 Tag 위젯을 쓰며 손으로 그리던 Tag 의 여백 7 이 빠졌다.
        // 단축키 탭 라벨 열 288은 서브탭마다 재는 clamp 로 바뀌어 빠졌다.
        // 단축키 녹화 버튼 140·24·추가 32(엔트리·quick-switch·스크립트 일곱 자리)는 kb-record-* 토큰으로 옮겨 빠졌다.
        20,
        "설정 화면의 폼 레이아웃",
    ),
    (
        "src/",
        // 부팅 오류 화면의 Quit 버튼 폭 120은 공용 boot_error 로 옮겨 빠졌다.
        6,
        "그 밖의 본체 gfx·state·app 치수. 역할에 맞는 토큰과 외부 API 경계 여부를 위치별로 검토한다.",
    ),
    (
        "crates/tasty-gallery/",
        // 전시용 프레임·스크롤 위치와 본체를 재현하는 치수를 구별해 해당 선언의 근거를 유지한다.
        // 권한 화면 예제의 콘텐츠 폭 560처럼 예제별 폭에 대응하는 Theme 값은 없다.
        // specimen 폭 360·440과 표 열 140은 새 스케일 값이라 집계되며, 같은 숫자의 토큰과 역할이 다르다.
        // HTML 스크립트 배너 예제의 surface 액자 240·360·120·300, 상태 카드 폭 600, 배너 버튼 패널 폭 560은
        // 디자인 Stage 값이며
        // 대응하는 Theme 접근자가 없다.
        // 이동 대기 예제의 화면 196·사이드바 160·행 28·아바타 28·겹침 칸 88은 디자인 specimen 값이다.
        // CenterState 무대 카드 288×220·288×160은 디자인 무대 액자라 토큰으로 옮기지 않는다.
        // 탭 스트립 스크롤 화살표 예제의 스트립 폭 288은 디자인 C4 행의 `--tasty-size-288`이며 역할 토큰이 없다.
        // Foundations disabled ink 예제의 C4 스트립 폭 288도 같은 디자인 값이다.
        // 페인 탭 스트립 시안 예제의 스트립 폭 560과 좁은 탭 칸 폭 120은 디자인 `Strip`의 `--tasty-size-560`과
        // cluster 예제의 `--tasty-size-120`이며 역할 토큰이 없다.
        // 튜토리얼 주제 목록의 스크롤 상한 200은 DTCG 토큰이 없는 화면 전용 치수다. 배율을 타므로
        // 본체와 같은 수기 Theme 접근자를 써서 이 수에 들어가지 않는다.
        // 원격 도구 예제의 SSH config 빈 상태 카드 폭 300은 디자인 Stage 액자 값이다.
        // 탐색기 사이드바 예제의 body 620·300과 pin 비교 줄의 560은 디자인 `ExpSidebar` specimen 높이이며
        // 역할 토큰이 없다.
        // 탭 스트립 툴팁 예제의 창 폭 320과 WebView 자리 높이 96은 디자인 Spec의 `--tasty-size-320`·`--tasty-size-96`이며
        // 역할 토큰이 없다.
        // ListCtrl disabled trailing 예제의 테마 패널 바깥 폭 320은 디자인 Spec의 `--tasty-size-320`이며
        // 공개 역할 토큰이 없다.
        // Appearance 색 행 예제와 Extension Mapping 예제의 테마 패널 바깥 폭 360은 디자인 Spec의
        // `--tasty-size-360`이며 공개 역할 토큰이 없다.
        // 스케일에 size-10이 들어오면서 기존 값 10이 새로 집계됐다.
        // html pane을 쌓은 탭 스트립 툴팁 예제의 WebView 자리 높이 64는 디자인 Spec의 `--tasty-size-64`이며
        // 역할 토큰이 없다.
        // transfer 예제의 여백 상수는 본체와 같은 transfer-* 토큰으로 옮겨 집계에서 빠졌다.
        // HTML 로드 실패 예제의 surface 높이 220은 디자인 `HtmlSurfaceG`의 `height={220}`이며 역할 토큰이 없다.
        // 파일 선택 필터 칩 예제의 카드 높이 300은 디자인 Spec의 `--tasty-size-300`이며 역할 토큰이 없다.
        // Font override 예제의 좁은 짝 폭 360은 디자인 Spec의 `--tasty-size-360`이며 역할 토큰이 없다.
        // 파일 선택 path bar 예제의 폭 사다리 520·440·360과 카드 높이 300, … 메뉴 예제 카드 높이 420은
        // 디자인 Spec `FilePickerFrame w/h`이며 역할 토큰이 없다.
        // 스케일에 size-540·700·1100이 들어오면서 팔레트 폭과 설정 창 크기 세 자리가 새로 집계됐다.
        // 시안의 결정 기록 Spec(*_settled.rs, 원격 폼)은 디자인 Stage의 액자·카드·견본 치수와
        // 표 열 폭을 지역 상수로 둔다. 같은 숫자의 토큰이 있어도 역할이 다르다.
        // 탭 스트립 툴팁 Stage의 오른쪽 여백 120은 디자인 Stage의 `--tasty-size-120`이며 역할 토큰이 없다.
        // 명령 팔레트 카드 폭은 palette-width 접근자를, 설정 창 예제 크기는 settings-window-* 토큰을 읽어
        // 갤러리 상수에서 빠졌다.
        // 시안 Spec 전사(역할 색 표·C1 보정 카드·8방향 크기 조절·경계와 겹침·사이드바 우클릭 메뉴)와
        // 설정 푸터 좌우 여백, 메뉴 바깥 폭이 시안 Stage 치수를 지역 상수로 더한다.
        // 설정 창 예제는 시안 갤러리 SettingsFrame 의 620×380 전시 치수(L2 168, 탭·구분선·카드 여백)를 둔다.
        // IpcSequence 편집기 예제의 카드 폭 460은 디자인 `HookSeqEditorG`의 `--tasty-size-460`이며 역할 토큰이 없다.
        // 시안 Spec 보완 스무 건(MultiSelect 상태 격자, 키캡 표시 방식, DrillDown 데모, 테마 칸, PresetView,
        // 마우스 캡처 배너, 더보기 메뉴 탄력 폭, 수정자 안내 패널, 접힌 레일, 포트 Process 열, 세그먼트 규칙,
        // 스크립트 트리거, 원격 attach 비교, 원격 표시 후보, 탐색기 화면)이 시안 Stage·액자 치수를 지역 상수로 둔다.
        // Gate 4 반영(Scripts 프레임·plan A 중앙 블록·Explorer 팝업·PresetView 입력 등)이 시안 값 상수를 더한다.
        // 원격 도구 예제의 경고 배지 높이 16은 공용 배지가 tag-size 토큰으로 그리면서 빠졌다.
        // 이미지 편집 모드 예제(paint bar·floating selection·New Image·Save As)가 시안 Stage 치수를
        // 지역 상수로 둔다. 그중 스케일 값 여섯 자리가 집계된다. 손잡이·팝업·입력·zoom % 치수 일곱 자리는
        // image-* 토큰으로 옮겨 빠졌다.
        // 스케일에 size-20·40이 들어오면서 기존 값 20·40 열여덟 자리가 새로 집계됐다.
        // 메뉴 예제(레일 팝업·사이드바 컨텍스트 메뉴)의 안쪽 여백 6 둘이 popup-content-margin 토큰으로 옮겨 빠졌다.
        // Layouts › Attention kinds 무대가 시안 행 간격 6과 레일 설명 칸 높이 24를 지역 상수로 둔다.
        // 탐색기 사이드바 Short cell 비교 줄이 시안 body 높이 240·200 두 자리를 더한다(하한 표본 84 는 스케일 값이 아니다).
        // 단축키 가져오기 예제의 선택·라벨·원래 조합 열 세 자리는 kb-ie-* 토큰으로 옮겨 빠졌다.
        // 자동 attach 거절 예제가 시안 Stage 치수 여섯 자리(사이드바·열 폭, 행 높이·간격 등)를 지역 상수로 둔다.
        // 플러그인 상세 설치 경로 Spec의 열 폭 380·540 중 스케일 값 540 한 자리가 집계된다(시안 Spec 무대 폭).
        // 포트 스캐너 Process 열 예제가 공용 Table과 공용 열 정의로 바뀌며 개략도 치수와 표 열 폭이 빠졌다.
        // 원격 전송 예제의 라벨 열 150(디자인 언급 있음)이 공용 SettingsRow 로 옮겨 빠졌다.
        // 단축키 행 예제가 본체 라벨 열 288·버튼 140×24·추가 버튼 32·프레임 600 다섯 자리를 더한다.
        // 그 가운데 라벨 열 288은 서브탭마다 재는 clamp 로 바뀌어 빠졌다.
        // attach 크기 동기 실패 예제가 시안 Stage 묶음 폭 460 한 자리를 지역 상수로 둔다.
        // 거절·크기 동기 실패 배너 예제의 좁은 스코프 표본 폭 360 두 자리(html 좁은 surface 시안 폭)를 더한다.
        // 스크립트 변경 확인 예제가 시안 좌우 여백 14 한 자리를 지역 상수로 둔다.
        // 탐색기 상태 칸 예제의 보조 줄 최대 폭 200은 공용 state_screen 으로 옮겨 빠졌다.
        // 이미지 상태 예제가 시안 상태 칸 높이 180과 compact 무대 폭 440 두 자리를 지역 상수로 둔다.
        // 스크립트 변경 확인 예제의 좌우 여백 14는 공용 script_confirm 으로 옮겨 빠졌다.
        // 탐색기 파일 조작 예제(명령 툴바·이름 입력·Find 바)가 시안 Stage 폭과 칸 높이를 지역 상수로 두며 다섯 자리를 더한다.
        // 탐색기 파일 작업 예제가 표본 칸 폭·본문 높이·대기열 본문 높이·드래그 행 폭을 지역 상수로 두며 세 자리가 집계된다.
        // 거절 배너의 좁은 표본이 크기 동기 실패 예제의 Narrow Spec 으로 옮겨 거절 예제의 360 한 자리가 빠졌다.
        // 탐색기 Properties·미리보기 예제의 여백 14·줄 20·최소 24·위 2·머리 40(이름 있음, 디자인 언급 있음) 다섯 자리가 explorer-props-*·preview-header-height 토큰으로 옮겨 빠졌다.
        // 단축키 행 예제의 녹화 버튼 140·24·추가 32 세 자리는 kb-record-* 토큰으로 옮겨 빠졌다.
        // 단축키 가져오기 예제의 option 대체 카드 높이 440 한 자리가 카드 삭제로 빠졌다.
        // DAG 빈 상태 글리프 24의 사본은 공용 dag_empty 를 불러 빠졌다.
        306,
        "갤러리 specimen은 배율 검사에서 제외돼도 스케일 검사는 받는다(ADR-0039). 이름 붙은 치수와 인라인 값, 전시 목적을 별도로 분류한다.",
    ),
    (
        "crates/tasty-ui-widgets/",
        // 아바타·파일 선택기·원격 도구의 치수는 같은 값의 다른 토큰으로 대체할 수 없다.
        // FH_TARGET_MONO_ADVANCE는 한 글자를 더할 때 잰 폭 증가분이므로 폰트가 바뀌면 다시 측정한다.
        // 스케일 밖 값은 이 집계에 포함되지 않는다.
        // 스케일에 size-10이 들어오면서 상태바 여백 10 등 기존 값 10이 새로 집계됐다.
        // 첫 실행 셸 설정 폼 폭 360은 디자인 `ShellSetupFrame`의 `--tasty-size-360`이며 역할 토큰이 없다.
        // 플러그인 추가 프리뷰의 가로 여백 14는 디자인 `--tasty-size-14`이며 역할 토큰이 없다.
        // 스케일에 size-20·40이 들어오면서 기존 값 20·40 한 자리가 새로 집계됐다.
        // 포트 스캐너 열 하한 120·140(Workspace·State) 둘은 popup과 갤러리가 함께 쓰는 ports_table의 표 전용 값이다.
        // Address 하한 140은 port-addr-col-min-width 토큰을 읽어 집계에서 빠졌다.
        // 플러그인 Attention 바의 상태 문구 12(디자인 fontSize 12, semantic 없음)가 공용 바로 들어왔다.
        // 공용 상태 화면(state_screen)의 보조 줄 최대 폭 200은 시안 `ExpState` 의 `maxWidth: 200` 이며 역할 토큰이 없다.
        // 스크립트 변경 확인의 좌우 여백 14는 시안 `Padding 12/14` 이며 본체 popup 과 갤러리가 함께 쓴다. 역할 토큰이 없다.
        // 탐색기 상세 표 열의 이름 열 하한 140·크기 열 하한 64 는 본체와 갤러리가 함께 쓰는 explorer_columns 로 옮겨 왔다.
        // DAG 빈 상태 글리프 24는 본체와 갤러리가 함께 쓰는 dag_empty 로 옮겨 왔다. 24px 아이콘 토큰이 없다.
        // 부팅 오류 카드의 내용 폭 460과 Quit 폭 120은 공용 boot_error 로 옮겨 왔다. 디자인 시안이 없다.
        // 본체에서 460은 f32 지역 값이라 집계되지 않았고 이름 붙은 LogicalPx 가 되며 집계된다.
        32,
        "공용 위젯",
    ),
    (
        "crates/",
        13,
        "나머지 크레이트(dag-layout·model·plugin 뷰어·settings·geometry)",
    ),
];

struct Hit {
    rel: String,
    line: usize,
    head: String,
    value: f32,
    /// 제외된 하한의 수도 따로 검증하므로 찾은 항목에서 삭제하지 않고 표시한다.
    floor: bool,
}

fn size_scale() -> Vec<f32> {
    let text = std::fs::read_to_string(repo_root().join(SIZE_SCALE_SOURCE)).unwrap_or_default();
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.trim_start().split_once("const SIZE_") else {
            continue;
        };
        let Some((_, rest)) = rest.split_once("LogicalPx(") else {
            continue;
        };
        let Some((value, _)) = rest.split_once(')') else {
            continue;
        };
        if let Ok(v) = value.trim().parse::<f32>() {
            out.push(v);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup();
    out
}

/// Theme 기본값과 생성 component 접근자에서 숫자를 읽는다.
/// 같은 값의 이름은 치환 후보일 뿐 해당 위치에 맞는 역할인지는 사람이 판단한다.
/// 여기서 읽지 못한 계산식·값까지 이름이 없다고 증명하는 것은 아니다.
fn theme_named_values() -> Vec<f32> {
    let mut out = Vec::new();
    for (path, prefix) in [
        ("crates/tasty-type-appearance/src/theme.rs", ": LogicalPx("),
        (
            "crates/tasty-type-appearance/src/generated_component.rs",
            "LogicalPx((",
        ),
    ] {
        let text = std::fs::read_to_string(repo_root().join(path)).unwrap_or_default();
        for line in text.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.split_once(prefix).map(|(_, r)| r) else {
                continue;
            };
            let head: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = head.parse::<f32>() {
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup();
    out
}

/// 리터럴을 감싼 가장 안쪽 호출의 위치와 이름. 블록·구조체·첨자 내부는 제외하며 바깥 clamp도 확인할 수 있게 괄호 위치를 돌려준다.
fn head_span_of(text: &[char], at: usize) -> Option<(usize, usize, String)> {
    let (mut paren, mut brace, mut bracket) = (0usize, 0usize, 0usize);
    let mut j = at;
    let open = loop {
        if j == 0 {
            return None;
        }
        j -= 1;
        match text[j] {
            ')' => paren += 1,
            '}' => brace += 1,
            ']' => bracket += 1,
            '(' => {
                if paren == 0 {
                    break j;
                }
                paren -= 1;
            }
            '{' => {
                if brace == 0 {
                    return None;
                }
                brace -= 1;
            }
            '[' => {
                if bracket == 0 {
                    return None;
                }
                bracket -= 1;
            }
            _ => {}
        }
    };
    let mut k = open;
    while k > 0 && (text[k - 1].is_ascii_alphanumeric() || matches!(text[k - 1], '_' | ':')) {
        k -= 1;
    }
    let name: String = text[k..open].iter().collect();
    name.rsplit("::")
        .next()
        .filter(|s| !s.is_empty())
        .map(|n| (k, open, n.to_owned()))
}

fn head_of(text: &[char], at: usize) -> Option<String> {
    head_span_of(text, at).map(|(_, _, name)| name)
}

/// 값 1이 clamp나 비교 문턱에 쓰였는지 찾는다. 같은 위치의 다른 값은 제외하지 않는다.
/// 이름 붙인 치수의 const 선언은 하한 예외로 빼지 않는다.
fn is_the_degenerate_floor(text: &[char], at: usize, value: f32) -> bool {
    if value != 1.0 {
        return false;
    }
    let Some((name_start, open, _)) = head_span_of(text, at) else {
        return false;
    };
    // 비교의 >와 매치 =>·반환 ->를 구별한다. 단위 환산의 1을 하한으로 오인하면 안 된다.
    let mut i = name_start;
    while i > 0 && matches!(text[i - 1], ' ' | '\t' | '\n') {
        i -= 1;
    }
    if i > 0 {
        let c = text[i - 1];
        let prev = if i >= 2 { text[i - 2] } else { ' ' };
        let comparison = match c {
            '<' => true,
            '>' => !matches!(prev, '=' | '-'),
            '=' => matches!(prev, '<' | '>' | '!' | '='),
            _ => false,
        };
        if comparison {
            return true;
        }
    }
    matches!(
        head_span_of(text, open)
            .as_ref()
            .map(|(_, _, n)| n.as_str()),
        Some("max" | "min" | "clamp")
    )
}

/// 양옆의 곱셈·나눗셈은 배율로 추정해 제외한다. 호출 자체가 길이를 받아도 그 안의 모든 숫자가 길이는 아니다.
fn is_length_operand(text: &[char], start: usize, end: usize) -> bool {
    let mut i = start;
    while i > 0 && matches!(text[i - 1], ' ' | '\t' | '\n') {
        i -= 1;
    }
    if i > 0 && matches!(text[i - 1], '*' | '/') {
        return false;
    }
    let mut j = end;
    while j < text.len() && matches!(text[j], ' ' | '\t' | '\n') {
        j += 1;
    }
    !(j < text.len() && matches!(text[j], '*' | '/'))
}

/// 스케일에 있는 리터럴의 줄 번호·호출 이름·값·하한 여부. 별도 제외 집계를 위해 0과 하한도 반환한다.
fn on_scale_literals(masked: &str, scale: &[f32]) -> Vec<(usize, String, f32, bool)> {
    let text: Vec<char> = masked.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < text.len() {
        if text[i] == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if !text[i].is_ascii_digit()
            || (i > 0 && (text[i - 1].is_ascii_alphanumeric() || matches!(text[i - 1], '_' | '.')))
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
        }
        if i + 1 < text.len() && text[i] == '.' && text[i + 1].is_ascii_digit() {
            i += 1;
            while i < text.len() && text[i].is_ascii_digit() {
                i += 1;
            }
        }
        let end = i;
        if end < text.len() && (text[end].is_ascii_alphanumeric() || matches!(text[end], '_' | '.'))
        {
            continue;
        }
        let Ok(value) = text[start..end].iter().collect::<String>().parse::<f32>() else {
            continue;
        };
        if !scale.contains(&value) || !is_length_operand(&text, start, end) {
            continue;
        }
        let Some(head) = head_of(&text, start) else {
            continue;
        };
        if LEN_CTOR.contains(&head.as_str()) || EGUI_LENGTH_HEADS.contains(&head.as_str()) {
            let floor = is_the_degenerate_floor(&text, start, value);
            out.push((line, head, value, floor));
        }
    }
    out
}

/// blank_tests에 따라 test 전용 코드를 제외한다. 두 결과의 차이로 제외 수를 센다.
fn scan(blank_tests: bool) -> Vec<Hit> {
    let scale = size_scale();
    let files = rust_sources();
    // test 전용 파일의 분류는 공용 shipping_scope를 사용한다.
    let gated = tasty_doc_guards::shipping_scope::test_only_files(&super::repo_root(), &files);
    let mut out = Vec::new();
    for (path, text) in &files {
        if blank_tests && gated.contains(path) {
            continue;
        }
        let rel = path.to_string_lossy().replace('\\', "/");
        let masked = mask_non_code(text);
        let masked = if blank_tests {
            blank_test_modules(&masked)
        } else {
            masked
        };
        for (line, head, value, floor) in on_scale_literals(&masked, &scale) {
            out.push(Hit {
                rel: rel.clone(),
                line,
                head,
                value,
                floor,
            });
        }
    }
    out
}

/// 실제 비교 대상에서는 등록된 전시용 값을 제외한다. 제외 수와 명부는 별도 검사한다.
fn judged() -> Vec<Hit> {
    considered()
        .into_iter()
        .filter(|h| !is_a_registered_display_specimen(h))
        .collect()
}

fn is_a_registered_display_specimen(hit: &Hit) -> bool {
    DISPLAY_SPECIMENS
        .iter()
        .any(|(path, head, _, _)| hit.rel == *path && hit.head == *head)
}

/// 전시용 값까지 포함한 비교 후보.
fn considered() -> Vec<Hit> {
    scan(true)
        .into_iter()
        .filter(|h| h.value != 0.0)
        .filter(|h| !h.floor)
        .filter(|h| !is_in_unit_space(h))
        .filter(|h| {
            !DECLARATION_SITES
                .iter()
                .any(|(p, _, _)| h.rel.starts_with(p))
        })
        .collect()
}

fn masked_sources() -> std::collections::BTreeMap<String, String> {
    rust_sources()
        .iter()
        .map(|(p, t)| (p.to_string_lossy().replace('\\', "/"), mask_non_code(t)))
        .collect()
}

fn area_of(rel: &str) -> Option<&'static str> {
    AREAS
        .iter()
        .filter(|(p, _, _)| rel.starts_with(p))
        .max_by_key(|(p, _, _)| p.len())
        .map(|(p, _, _)| *p)
}

/// 선언 바로 위의 연속 //·/// 주석을 읽는다. 입력은 리터럴만 가리고 주석은 남겨야 한다.
fn attached_comment(masked_literals: &str, line: usize) -> String {
    let lines: Vec<&str> = masked_literals.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = line.saturating_sub(1);
    while i > 0 {
        let t = lines.get(i - 1).map_or("", |l| l.trim());
        if t.starts_with("//") {
            out.push(t);
            i -= 1;
        } else {
            break;
        }
    }
    out.reverse();
    out.join("\n")
}

/// 붙은 주석에 디자인 출처를 나타내는 단어가 있는지 확인한다. 실제 출처의 진위는 검증하지 않는다.
fn cites_the_design(comment: &str) -> bool {
    const MARKS: &[&str] = &["jsx", "디자인", "시안", "design"];
    let lower = comment.to_lowercase();
    MARKS.iter().any(|m| lower.contains(m))
}

/// 해당 줄이 LogicalPx·PhysicalPx const 선언인지 확인한다. 이름을 붙였어도 대응하는 토큰과 역할이 같은지는 별도 검토가 필요하다.
fn declares_a_named_dimension(masked: &str, line: usize) -> bool {
    let Some(text) = masked.lines().nth(line.saturating_sub(1)) else {
        return false;
    };
    text.contains("const ") && (text.contains(": LogicalPx =") || text.contains(": PhysicalPx ="))
}

#[test]
fn every_on_scale_literal_lives_inside_a_recorded_area() {
    let hits = judged();
    let stray: Vec<String> = hits
        .iter()
        .filter(|h| area_of(&h.rel).is_none())
        .map(|h| format!("  {}:{}  {}({})", h.rel, h.line, h.head, h.value))
        .collect();
    assert!(
        stray.is_empty(),
        "등록된 영역 밖에 size-*와 같은 숫자를 쓴 길이 후보가 있다. 역할에 맞는 토큰을 사용하거나 AREAS에 영역과 남긴 이유를 기록한다:\n{}",
        stray.join("\n")
    );
}

#[test]
fn no_area_grows_past_its_budget() {
    let hits = judged();
    let mut lines: Vec<String> = Vec::new();
    for (area, budget, why) in AREAS {
        let n = hits
            .iter()
            .filter(|h| area_of(&h.rel) == Some(*area))
            .count();
        if n > *budget || n == 0 {
            lines.push(format!("  {area}  상한 {budget} · 실측 {n}  ({why})"));
            // 같은 수집 결과로 남은 위치를 출력한다. 로그가 잘리지 않도록 영역별 출력 수를 제한한다.
            const PER_AREA: usize = 40;
            let mine: Vec<&Hit> = hits
                .iter()
                .filter(|h| area_of(&h.rel) == Some(*area))
                .collect();
            for h in mine.iter().take(PER_AREA) {
                lines.push(format!(
                    "      {}:{}  {}({})",
                    h.rel, h.line, h.head, h.value
                ));
            }
            if mine.len() > PER_AREA {
                lines.push(format!(
                    "      … 외 {} 자리(메시지 길이 상한)",
                    mine.len() - PER_AREA
                ));
            }
        }
    }
    assert!(
        lines.is_empty(),
        "영역의 수가 상한을 넘었거나 0이다. 늘었다면 실제 새 사용인지 스케일·수집 범위가 바뀐 것인지 확인한다. 고칠 수 없는 치수는 해당 선언에 이유를 남긴 뒤 상한을 올린다. 0이면 수집이 무너졌는지 확인하고, 실제로 비었다면 그 영역을 지운다. 줄어든 것은 실패가 아니며 상한을 낮추지 않아도 된다. 증감 이력을 주석에 적지 않는다:\n{}",
        lines.join("\n")
    );
}

/// 같은 setter가 세 번 이상, 두 가지 이상의 값으로 나타나면 전시 후보로 분류한다. 실제 전시 목적은 사람이 확인한다.
fn is_a_varied_specimen_value(masked: &str, hit: &Hit) -> bool {
    // 일반 기하 생성자는 반복 사용이 흔하므로 전시 후보로 분류하지 않는다.
    const SETTERS: &[&str] = &[
        "size",
        "desired_width",
        "exact_width",
        "fixed_size",
        "max_height",
        "min_height",
        "max_width",
        "min_width",
    ];
    if !SETTERS.contains(&hit.head.as_str()) {
        return false;
    }
    let scale = size_scale();
    let same_head: Vec<f32> = on_scale_literals(masked, &scale)
        .into_iter()
        .filter(|(_, head, ..)| *head == hit.head)
        .map(|(_, _, v, _)| v)
        .collect();
    if same_head.len() < 3 {
        return false;
    }
    let mut distinct = same_head.clone();
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    distinct.dedup();
    distinct.len() >= 2
}

/// 갤러리 후보를 이름 붙은 선언 여부와 디자인 출처 단어의 유무로 나눈다. 주석의 내용이 검사 입력이다.
#[test]
fn the_gallery_share_is_one_question_or_it_is_not() {
    let files = rust_sources();
    let hits = considered();
    let (mut named_cited, mut named_plain) = (0usize, 0usize);
    let (mut inline_cited, mut inline_plain) = (0usize, 0usize);
    let mut with_comment = 0usize;
    for h in hits
        .iter()
        .filter(|h| h.rel.starts_with("crates/tasty-gallery/"))
    {
        let raw = files
            .iter()
            .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == h.rel)
            .map_or("", |(_, t)| t.as_str());
        let comment = attached_comment(&tasty_doc_guards::source_text::mask_literals(raw), h.line);
        if !comment.is_empty() {
            with_comment += 1;
        }
        let named = declares_a_named_dimension(&mask_non_code(raw), h.line);
        match (named, cites_the_design(&comment)) {
            (true, true) => named_cited += 1,
            (true, false) => named_plain += 1,
            (false, true) => inline_cited += 1,
            (false, false) => inline_plain += 1,
        }
    }
    assert!(
        with_comment >= 40,
        "주석이 붙은 후보가 {with_comment}개뿐이다. 실제 주석 감소와 추출 누락을 확인한다."
    );
    let counted = [
        ("이름 있음·디자인 언급", named_cited, 115),
        ("이름 있음·언급 없음", named_plain, 174),
        ("이름 없음·디자인 언급", inline_cited, 1),
        ("이름 없음·언급 없음", inline_plain, 24),
    ];
    let over: Vec<String> = counted
        .iter()
        .filter(|(_, n, cap)| n > cap)
        .map(|(kind, n, cap)| format!("  {kind}: 상한 {cap} · 실측 {n}"))
        .collect();
    assert!(
        over.is_empty(),
        "갤러리 후보의 (이름 있음/없음, 디자인 언급 있음/없음) 분류가 상한을 넘었다. 해당 선언과 주석을 확인하고, 남길 치수면 상한을 올린다. 줄어든 것은 실패가 아니다:\n{}",
        over.join("\n")
    );
}

/// 전시 후보·같은 Theme 값 존재·값 없음·이름 붙은 치수로 분류한다. 같은 값의 토큰이 있어도 해당 역할에 맞는다는 뜻은 아니다.
#[test]
fn the_gallery_share_splits_into_four_kinds() {
    let files = rust_sources();
    let named_values = theme_named_values();
    assert!(
        named_values.len() > 30 && named_values.contains(&28.0),
        "Theme 숫자 값 목록의 크기 또는 대조 값이 다르다(수집 {}개). 파싱 범위를 확인한다.",
        named_values.len()
    );

    let hits = considered();
    let (mut displayed, mut named_value, mut nameless, mut undecided) =
        (0usize, 0usize, 0usize, 0usize);
    for h in hits
        .iter()
        .filter(|h| h.rel.starts_with("crates/tasty-gallery/"))
    {
        let raw = files
            .iter()
            .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == h.rel)
            .map_or("", |(_, t)| t.as_str());
        let masked = mask_non_code(raw);
        if is_a_varied_specimen_value(&masked, h) {
            displayed += 1;
        } else if !named_values.contains(&h.value) {
            nameless += 1;
        } else if declares_a_named_dimension(&masked, h.line) {
            undecided += 1;
        } else {
            named_value += 1;
        }
    }
    // 전시 상한은 기존 명부에서 읽어 중복 기록하지 않는다.
    let roster: usize = DISPLAY_SPECIMENS.iter().map(|(.., n, _)| n).sum();
    let gallery = hits
        .iter()
        .filter(|h| h.rel.starts_with("crates/tasty-gallery/"))
        .count();
    assert_eq!(
        displayed + named_value + nameless + undecided,
        gallery,
        "갤러리 후보가 네 분류 중 하나에 들어가지 않았다"
    );
    assert_eq!(
        nameless, 0,
        "같은 숫자의 Theme 값이 없는 갤러리 후보가 있다. 토큰을 쓰거나 이름 붙은 치수로 선언한다"
    );
    // 같은 Theme 값과 숫자만 같은 자리의 상한이다. 늘면 그 자리가 토큰을 써야 하는지 확인한다.
    assert!(
        named_value <= 19 && displayed <= roster,
        "갤러리의 같은 Theme 값 자리 {named_value}(상한 19) 또는 전시 후보 {displayed}(전시 명부 {roster})가 상한을 넘었다. 줄어든 것은 실패가 아니다"
    );
}

/// 등록된 전시용 값의 개수를 맞추고 미등록 전시 후보도 찾는다.
#[test]
fn the_display_roster_points_at_real_sites_and_covers_the_ones_that_look_like_it() {
    assert!(
        !DISPLAY_SPECIMENS.is_empty(),
        "전시 명부가 비어 제외 범위를 대조할 수 없다"
    );
    let all = considered();
    for (path, head, budget, why) in DISPLAY_SPECIMENS {
        let n = all
            .iter()
            .filter(|h| h.rel == *path && h.head == *head)
            .count();
        assert!(
            (1..=*budget).contains(&n),
            "전시 명부 `{path}` 의 `{head}`(사유: {why}) 자리가 {n} 개다(상한 {budget}). 늘었으면 \
             전시가 아닌 것이 섞였을 수 있다. 0이면 자리가 옮겨졌는지 확인하고 명부에서 지운다"
        );
    }

    let masked = masked_sources();
    let unregistered: Vec<String> = all
        .iter()
        .filter(|h| !is_a_registered_display_specimen(h))
        .filter(|h| {
            masked
                .get(&h.rel)
                .is_some_and(|m| is_a_varied_specimen_value(m, h))
        })
        .map(|h| format!("  {}:{}  {}({})", h.rel, h.line, h.head, h.value))
        .collect();
    assert!(
        unregistered.is_empty(),
        "전시로 보이는데 명부에 없는 자리가 있다 — 전시가 맞으면 사유와 함께 등록하고, \
         아니면 토큰으로 바꿔라:\n{}",
        unregistered.join("\n")
    );
}

/// 빈 스케일이면 모든 후보가 사라지므로 먼저 크기를 확인한다.
#[test]
fn the_size_scale_is_not_empty() {
    let scale = size_scale();
    assert!(
        scale.len() > 10 && scale.contains(&24.0),
        "size-* 값을 {}개만 읽었다. `{SIZE_SCALE_SOURCE}`의 위치·선언 형식·수집 결과를 확인한다.",
        scale.len()
    );
}

/// 비교에서 제외한 0·test·하한·정규화 좌표·선언의 수가 상한을 넘지 않고 비지 않았는지 확인한다.
#[test]
fn the_blind_spots_stay_within_their_budgets() {
    let all = scan(false);
    let shipped = scan(true);
    let zeros = shipped.iter().filter(|h| h.value == 0.0).count();
    let in_tests = all.len() - shipped.len();
    let floors = shipped.iter().filter(|h| h.value != 0.0 && h.floor).count();
    let unit_space = shipped
        .iter()
        .filter(|h| h.value != 0.0 && !h.floor && is_in_unit_space(h))
        .count();
    let roster: usize = UNIT_SPACE_SITES.iter().map(|(.., n, _)| n).sum();
    let counted = [
        ("제외한 0", zeros, 234),
        ("test 전용 코드", in_tests, 612),
        ("값 1의 하한", floors, 21),
        ("정규화 좌표", unit_space, roster),
    ];
    let off: Vec<String> = counted
        .iter()
        .filter(|(_, n, cap)| *n == 0 || n > cap)
        .map(|(kind, n, cap)| format!("  {kind}: 상한 {cap} · 실측 {n}"))
        .collect();
    assert!(
        off.is_empty(),
        "비교에서 제외한 자리의 수가 상한을 넘었거나 0이다. 늘었다면 실제 사용과 수집 범위의 변경을 확인하고 상한을 올린다. 0이면 수집·판정이 무너졌는지 확인한다. 줄어든 것은 실패가 아니다:\n{}",
        off.join("\n")
    );
    for (path, head, budget, why) in UNIT_SPACE_SITES {
        let n = shipped
            .iter()
            .filter(|h| h.value != 0.0 && h.rel == *path && h.head == *head)
            .count();
        assert!(
            (1..=*budget).contains(&n),
            "정규화 좌표 명부 `{path}` 의 `{head}`(사유: {why}) 자리가 {n} 개다(상한 {budget}). \
             늘었으면 픽셀인 것이 섞였을 수 있다. 0이면 자리가 옮겨졌는지 확인하고 명부에서 지운다"
        );
    }
    for (site, budget, why) in DECLARATION_SITES {
        let n = shipped.iter().filter(|h| h.rel.starts_with(site)).count();
        assert!(
            (1..=*budget).contains(&n),
            "선언 자리 `{site}`(사유: {why})의 건수 {n}이 상한 {budget}을 넘었거나 0이다"
        );
    }
}

#[cfg(test)]
mod detector {
    use super::*;

    const SCALE: &[f32] = &[0.0, 1.0, 4.0, 24.0, 400.0];

    fn hits(src: &str) -> Vec<(usize, String, f32)> {
        on_scale_literals(&mask_non_code(src), SCALE)
            .into_iter()
            .map(|(line, head, value, _)| (line, head, value))
            .collect()
    }

    #[test]
    fn it_reads_a_literal_sitting_in_a_length_position() {
        assert_eq!(
            hits("let s = egui::vec2(400.0, 24.0);"),
            vec![(1, "vec2".to_owned(), 400.0), (1, "vec2".to_owned(), 24.0)]
        );
        assert_eq!(
            hits("LogicalPx(24.0)"),
            vec![(1, "LogicalPx".to_owned(), 24.0)]
        );
    }

    #[test]
    fn a_zero_width_bound_is_counted_and_a_nonzero_bound_is_not_a_floor_exemption() {
        let zero = "(name_right - name_left).max(LogicalPx(0.0))";
        assert_eq!(hits(zero), vec![(1, "LogicalPx".to_owned(), 0.0)]);
        let dimension = "(name_right - name_left).max(LogicalPx(24.0))";
        assert_eq!(hits(dimension), vec![(1, "LogicalPx".to_owned(), 24.0)]);
        assert!(floors(dimension).is_empty());
    }

    #[test]
    fn a_test_viewport_is_counted_before_gating_but_not_as_shipped_geometry() {
        let test =
            "#[cfg(test)]\nmod layout_tests { fn narrow_width() { egui::vec2(400.0, 360.0); } }";
        assert_eq!(hits(test), vec![(2, "vec2".to_owned(), 400.0)]);
        assert!(hits(&blank_test_modules(&mask_non_code(test))).is_empty());
        let shipped = test.replace("#[cfg(test)]\n", "");
        assert_eq!(
            hits(&blank_test_modules(&mask_non_code(&shipped))),
            vec![(1, "vec2".to_owned(), 400.0)]
        );
    }

    #[test]
    fn a_factor_is_not_a_length() {
        assert!(hits("egui::vec2(d * 4.0, 1.0 / n)").is_empty());
        assert!(hits("egui::pos2(w / 4.0, 24.0 * k)").is_empty());
        assert_eq!(
            hits("egui::vec2(4.0, h)"),
            vec![(1, "vec2".to_owned(), 4.0)]
        );
    }

    /// 스케일 밖 값을 통과시킨다고 가까운 토큰으로 바꾸라는 뜻은 아니다. ADR-0035의 디자인 절차를 따른다.
    #[test]
    fn a_value_off_the_scale_is_not_this_guards_business() {
        assert!(hits("egui::vec2(17.0, 13.0)").is_empty());
    }

    #[test]
    fn another_familys_position_is_not_measured_here() {
        assert!(hits(".corner_radius(4.0)").is_empty());
        assert!(hits("FontId::proportional(24.0)").is_empty());
    }

    #[test]
    fn a_number_inside_prose_is_not_code() {
        assert!(hits("// vec2(24.0) 이라고 쓰면 된다").is_empty());
        assert!(hits("let doc = \"size(24.0) 원형\";").is_empty());
    }

    #[test]
    fn a_value_outside_an_argument_list_has_no_head() {
        assert!(hits("ui.horizontal(|ui| { let x = 24.0; });").is_empty());
    }

    #[test]
    fn a_subscript_is_not_a_length() {
        assert!(hits("egui::vec2(p[1], q)").is_empty());
        assert!(hits("egui::vec2(pts[0][1], q)").is_empty());
        assert_eq!(
            hits("egui::vec2(p[1], 24.0)"),
            vec![(1, "vec2".to_owned(), 24.0)]
        );
    }

    fn floors(src: &str) -> Vec<f32> {
        on_scale_literals(&mask_non_code(src), SCALE)
            .into_iter()
            .filter(|(.., floor)| *floor)
            .map(|(_, _, v, _)| v)
            .collect()
    }

    /// clamp 위치만으로 모든 값을 제외하지 않도록 값 1인 경우와 다른 치수를 대조한다.
    #[test]
    fn the_degenerate_floor_is_not_a_dimension() {
        assert_eq!(floors("(h - t).max(PhysicalPx(1.0))"), vec![1.0]);
        assert_eq!(floors("w.min(LogicalPx(1.0))"), vec![1.0]);
        assert_eq!(floors("(a - b).abs() < PhysicalPx(1.0)"), vec![1.0]);
        assert_eq!(floors("d <= LogicalPx(1.0)"), vec![1.0]);
        assert!(floors("w.max(LogicalPx(24.0))").is_empty());
        assert!(floors("let x = LogicalPx(1.0);").is_empty());
        assert!(floors("MouseWheelUnit::Point => LogicalPx(1.0),").is_empty());
        assert!(floors("fn f() -> LogicalPx(1.0)").is_empty());
        assert!(floors("const GLYPH_STROKE: LogicalPx = LogicalPx(1.0);").is_empty());
    }
}
