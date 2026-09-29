//! 실제 그린 텍스트와 clip 영역을 비교해 버튼·라벨이 보이는지 확인한다.
use super::*;

const CONFIRM: &str = "CONFIRM-LABEL";
const CANCEL: &str = "CANCEL-LABEL";

/// 메인 피커 PopupDef 대로(셸 타이틀바 유무 포함) 만든 같은 크기의 popup.
fn picker_state(size: egui::Vec2) -> crate::adapters::ui::popup::PopupState {
    let def = crate::adapters::ui::popup::defs::find(FILE_PICKER_POPUP_ID)
        .expect("file picker PopupDef 가 등록돼 있어야 한다");
    crate::adapters::ui::popup::PopupState::new(FILE_PICKER_POPUP_ID, "t", size)
        .with_headless(def.headless)
}

fn deep_crumbs(depth: usize, seg: &str) -> Vec<CrumbView> {
    let mut out = vec![CrumbView { label: "/".into() }];
    out.extend((1..depth).map(|i| CrumbView {
        label: format!("{seg}-{i:02}"),
    }));
    out
}

fn entries() -> Vec<FilePickerEntryView> {
    (0..40)
        .map(|i| FilePickerEntryView {
            name: format!("file-{i}.toml"),
            is_dir: i % 5 == 0,
            size_display: "1 KB".into(),
            modified_display: "2026-09-14".into(),
        })
        .collect()
}

/// 한 프레임을 popup 매니저와 같은 조건(Area · 콘텐츠 크기 고정 · 콘텐츠로 clip)으로 그리고,
/// 칠해진 텍스트마다 (문자열, 화면 사각형, clip 사각형) 을 모은다.
fn painted(
    size: egui::Vec2,
    crumbs: &[CrumbView],
    mode: FilePickerMode<'_>,
) -> (egui::Rect, Vec<(String, egui::Rect, egui::Rect)>) {
    painted_entries(size, crumbs, mode, &entries())
}

fn painted_entries(
    size: egui::Vec2,
    crumbs: &[CrumbView],
    mode: FilePickerMode<'_>,
    entries: &[FilePickerEntryView],
) -> (egui::Rect, Vec<(String, egui::Rect, egui::Rect)>) {
    painted_selection(size, crumbs, mode, entries, "file-1.toml")
}

fn painted_selection(
    size: egui::Vec2,
    crumbs: &[CrumbView],
    mode: FilePickerMode<'_>,
    entries: &[FilePickerEntryView],
    selection: &str,
) -> (egui::Rect, Vec<(String, egui::Rect, egui::Rect)>) {
    let (content, shapes) = painted_shapes(size, crumbs, mode, entries, selection);
    let mut out = Vec::new();
    for clipped in shapes.iter() {
        if let egui::epaint::Shape::Text(t) = &clipped.shape {
            let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
            out.push((t.galley.text().to_string(), rect, clipped.clip_rect));
        }
    }
    (content, out)
}

/// 두 번째 프레임에 칠해진 모양 전체.
fn painted_shapes(
    size: egui::Vec2,
    crumbs: &[CrumbView],
    mode: FilePickerMode<'_>,
    entries: &[FilePickerEntryView],
    selection: &str,
) -> (egui::Rect, Vec<egui::epaint::ClippedShape>) {
    painted_shapes_in(&picker_state(size), crumbs, mode, entries, selection)
}

/// `popup` 이 내주는 콘텐츠 사각형에 뷰를 그린다.
fn painted_shapes_in(
    popup: &crate::adapters::ui::popup::PopupState,
    crumbs: &[CrumbView],
    mode: FilePickerMode<'_>,
    entries: &[FilePickerEntryView],
    selection: &str,
) -> (egui::Rect, Vec<egui::epaint::ClippedShape>) {
    let th = crate::theme::theme();
    let ctx = egui::Context::default();
    let content = popup.content_rect();
    let selected = vec![selection.to_string()];
    let props = FilePickerProps {
        theme: &th,
        remote_host: None,
        crumbs,
        state: FpViewState::Loaded,
        entries,
        selected: &selected,
        mode,
        owns_escape: false,
        title_label: "title",
        name_field_label: "File name",
        name_placeholder: "placeholder",
        cancel_label: CANCEL,
        confirm_label: CONFIRM,
        overwrite_warning: "{name} already exists in this folder. Saving replaces it.",
        hidden_folders_one: "1 hidden",
        hidden_folders_many: "{} hidden",
        folder_not_save_target: "not a save target: {name}",
        folder_open_enters: "{name} is a folder — {confirm} enters it.",
        empty_label: "",
        loading_label: "",
        loading_body_local: "",
        loading_body_remote: "",
        error_perm_title: "",
        error_perm_retry: "",
        error_conn_title: "",
        error_conn_reconnect: "",
    };
    let mut out = Vec::new();
    // 첫 프레임은 폰트가 확정되지 않아 galley 가 비는 경우가 있어 두 번 돈다.
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2000.0, 1500.0),
            )),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| {
            egui::Area::new(egui::Id::new("fp_layout_test"))
                // 등장 페이드가 색을 반투명하게 바꾸지 않도록 끈다.
                .fade_in(false)
                .fixed_pos(content.min)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_min_size(content.size());
                    ui.set_max_size(content.size());
                    ui.set_clip_rect(content);
                    draw_file_picker_view(ui, &props);
                });
        });
        out = full.shapes;
    }
    (content, out)
}

/// `label` 이 칠해졌고, 그 텍스트가 콘텐츠 영역과 자신의 clip 안에 **온전히** 들어간다.
fn assert_fully_visible(
    content: egui::Rect,
    shapes: &[(String, egui::Rect, egui::Rect)],
    label: &str,
    case: &str,
) {
    let hits: Vec<_> = shapes.iter().filter(|(t, _, _)| t == label).collect();
    assert!(!hits.is_empty(), "{case}: {label} 이 칠해지지 않았다");
    for (_, rect, clip) in hits {
        let visible = content.intersect(*clip);
        assert!(
            visible.expand(0.5).contains_rect(*rect),
            "{case}: {label} 이 잘렸다 — text {rect:?}, 보이는 영역 {visible:?}"
        );
    }
}

/// 경로 깊이·성분 길이·모드·popup 폭을 흔들어도 footer 의 취소·확정 버튼은 온전히 보인다.
#[test]
fn footer_buttons_are_never_clipped_at_any_path_length() {
    let long_seg = "a-very-long-directory-name-that-keeps-going-and-going";
    let long_name = "x".repeat(300);
    let modes = [
        (
            "open",
            FilePickerMode::Open {
                selection_text: "file-1.toml",
            },
        ),
        (
            "save",
            FilePickerMode::Save {
                name: "keybindings.toml",
                overwrite: false,
                can_confirm: true,
            },
        ),
        (
            "save-overwrite",
            FilePickerMode::Save {
                name: &long_name,
                overwrite: true,
                can_confirm: true,
            },
        ),
    ];
    for size in [egui::vec2(640.0, 480.0), egui::vec2(400.0, 360.0)] {
        for depth in [1, 4, 8, 30] {
            for seg in ["d", long_seg] {
                for (mode_name, mode) in modes {
                    let crumbs = deep_crumbs(depth, seg);
                    let case = format!("{size:?} depth={depth} seg={} mode={mode_name}", seg.len());
                    let (content, shapes) = painted(size, &crumbs, mode);
                    assert_fully_visible(content, &shapes, CONFIRM, &case);
                    assert_fully_visible(content, &shapes, CANCEL, &case);
                }
            }
        }
    }
}

/// 셸 타이틀바 없이 위젯 헤더 하나만 있다 — 제목은 한 번만 칠해지고, popup 윗변에서
/// 타이틀바 한 줄 높이 안에 들어간다. 셸 타이틀바가 있으면 위젯 헤더가 그만큼 내려간다.
#[test]
fn the_title_is_painted_once_in_the_top_strip_of_the_popup() {
    let size = egui::vec2(640.0, 480.0);
    let (_, shapes) = painted(
        size,
        &deep_crumbs(1, "d"),
        FilePickerMode::Open { selection_text: "" },
    );
    let hits: Vec<_> = shapes.iter().filter(|(t, _, _)| t == "title").collect();
    assert_eq!(hits.len(), 1, "제목은 한 번만 칠해진다: {hits:?}");
    let top = picker_state(size).pos.y;
    let strip = crate::adapters::ui::popup::title_bar_height().value();
    assert!(
        hits[0].1.bottom() <= top + strip,
        "제목이 popup 윗줄({top}..{}) 밖에 있다: {:?}",
        top + strip,
        hits[0].1
    );
}

/// 경로 막대 줄은 창 좌우 끝까지 bg-sidebar 로 칠해지고, 그 안에 breadcrumb 가 있다.
#[test]
fn the_path_bar_band_is_filled_edge_to_edge_with_bg_sidebar() {
    assert_path_band_is_full_width(&picker_state(egui::vec2(640.0, 480.0)));
}

/// 같은 뷰를 쓰는 설정 창 파일 선택기도 셸 공통 여백 없이 띠가 창 끝까지 닿는다.
#[test]
fn the_settings_chooser_path_bar_band_is_filled_edge_to_edge() {
    let popup = crate::adapters::ui::popup::PopupState::new(
        crate::view::settings::ui::file_chooser::FILE_CHOOSER_POPUP_ID,
        "t",
        egui::vec2(640.0, 480.0),
    )
    .with_headless(true);
    assert_path_band_is_full_width(&popup);
}

fn assert_path_band_is_full_width(popup_state: &crate::adapters::ui::popup::PopupState) {
    let (content, shapes) = painted_shapes_in(
        popup_state,
        &deep_crumbs(2, "crumb"),
        FilePickerMode::Open { selection_text: "" },
        &entries(),
        "file-1.toml",
    );
    let fill = crate::theme::theme().bg_sidebar().to_egui();
    let popup = popup_state.popup_rect();
    let bands: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .collect();
    let crumb = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == "crumb-01" => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .expect("breadcrumb 이 칠해져야 한다");
    assert!(
        bands.iter().any(|b| b.left() <= popup.left()
            && b.right() >= popup.right()
            && b.contains_rect(crumb)),
        "경로 막대를 덮는 전폭 bg-sidebar 띠가 없다: bands {bands:?}, crumb {crumb:?}, popup {popup:?}, content {content:?}"
    );
}

/// 깊은 경로는 가운데를 접고, 위치를 알려주는 둘(현재 폴더와 부모)과 root 는 보인다.
#[test]
fn deep_breadcrumb_elides_the_middle_and_keeps_root_and_last_two() {
    let crumbs = deep_crumbs(30, "segment");
    let (content, shapes) = painted(
        egui::vec2(640.0, 480.0),
        &crumbs,
        FilePickerMode::Open { selection_text: "" },
    );
    for label in ["/", "…", "segment-28", "segment-29"] {
        assert_fully_visible(content, &shapes, label, "deep");
    }
    assert!(
        !shapes.iter().any(|(t, _, _)| t == "segment-10"),
        "숨긴 조상은 칠해지지 않는다"
    );
}

/// 짧은 경로는 접지 않는다 — 생략은 넘칠 때만의 렌더 규칙이다.
#[test]
fn short_breadcrumb_is_not_elided() {
    let crumbs = deep_crumbs(4, "d");
    let (content, shapes) = painted(
        egui::vec2(640.0, 480.0),
        &crumbs,
        FilePickerMode::Open { selection_text: "" },
    );
    for c in &crumbs {
        assert_fully_visible(content, &shapes, &c.label, "short");
    }
    assert!(!shapes.iter().any(|(t, _, _)| t == "…"));
}

/// 보이는 경로와 생략 메뉴의 경로를 합치면 전체 조상이 빠짐없이 남아야 한다.
#[test]
fn every_ancestor_is_either_painted_or_behind_the_ellipsis() {
    let crumbs = deep_crumbs(30, "segment");
    let (_, shapes) = painted(
        egui::vec2(POPUP_WIDTH.value(), POPUP_HEIGHT.value()),
        &crumbs,
        FilePickerMode::Open { selection_text: "" },
    );
    let painted_labels: Vec<&str> = shapes.iter().map(|(t, _, _)| t.as_str()).collect();
    assert!(painted_labels.contains(&"…"), "접혔는데 `…` 가 없다");
    let hidden: Vec<&str> = crumbs
        .iter()
        .map(|c| c.label.as_str())
        .filter(|l| !painted_labels.contains(l))
        .collect();
    assert!(
        !hidden.is_empty(),
        "테스트 경로는 주어진 폭을 넘어 일부 조상 폴더가 접혀야 한다"
    );
    // 숨은 것은 전부 **가운데** 조상이다: root 와 현재 폴더는 접히지 않는 폭이다.
    assert!(
        !hidden.contains(&crumbs[0].label.as_str()),
        "이 폭에서는 루트 경로가 표시되어야 한다"
    );
    assert!(
        !hidden.contains(&crumbs[crumbs.len() - 1].label.as_str()),
        "현재 폴더가 접혔다"
    );
}

/// 긴 파일명과 짧은 파일명 모두 크기·날짜 열을 덮지 않고 온전히 보인다.
fn assert_filenames_stay_inside_the_name_column(size: egui::Vec2) {
    for name in ["x".repeat(120) + ".toml", "notes.md".to_string()] {
        let rows = vec![FilePickerEntryView {
            name: name.clone(),
            is_dir: false,
            size_display: "0 B".into(),
            modified_display: "2026-09-14".into(),
        }];
        let (content, shapes) = painted_entries(
            size,
            &deep_crumbs(1, "d"),
            FilePickerMode::Open { selection_text: "" },
            &rows,
        );
        let case = format!("{size:?} name_len={}", name.len());
        for label in [name.as_str(), "0 B", "2026-09-14"] {
            assert_fully_visible(content, &shapes, label, &case);
        }
        let name_rect = shapes.iter().find(|(text, _, _)| text == &name).unwrap().1;
        for label in ["0 B", "2026-09-14"] {
            let meta_rect = shapes.iter().find(|(text, _, _)| text == label).unwrap().1;
            assert!(
                name_rect.right() < meta_rect.left(),
                "{case}: {name_rect:?} overlaps {meta_rect:?}"
            );
        }
    }
}

#[test]
fn filenames_stay_inside_the_name_column_at_default_width() {
    assert_filenames_stay_inside_the_name_column(egui::vec2(640.0, 480.0));
}

#[test]
fn filenames_stay_inside_the_name_column_at_narrow_width() {
    assert_filenames_stay_inside_the_name_column(egui::vec2(400.0, 360.0));
}

/// 폴더 안내와 확정 버튼 이름이 일치하고 두 모드 모두 버튼이 잘리지 않는지 확인한다.
#[test]
fn a_selected_folder_puts_its_line_in_the_footer_without_pushing_the_buttons_out() {
    let entries = entries();
    let folder = entries
        .iter()
        .find(|e| e.is_dir)
        .expect("픽스처에 폴더가 있다")
        .name
        .clone();
    let size = egui::vec2(POPUP_WIDTH.value(), POPUP_HEIGHT.value());
    let crumbs = deep_crumbs(3, "seg");

    let (content, shapes) = painted_selection(
        size,
        &crumbs,
        FilePickerMode::Save {
            name: "keys.toml",
            overwrite: false,
            can_confirm: true,
        },
        &entries,
        &folder,
    );
    let line = format!("not a save target: {folder}");
    assert_fully_visible(content, &shapes, &line, "save + folder");
    assert_fully_visible(content, &shapes, CONFIRM, "save + folder");
    assert_fully_visible(content, &shapes, CANCEL, "save + folder");

    let (content, shapes) = painted_selection(
        size,
        &crumbs,
        FilePickerMode::Open { selection_text: "" },
        &entries,
        &folder,
    );
    let line = format!("{folder} is a folder — {CONFIRM} enters it.");
    assert_fully_visible(content, &shapes, &line, "open + folder");
    assert_fully_visible(content, &shapes, CONFIRM, "open + folder");

    // 파일 선택 시에는 폴더 안내가 없어야 한다.
    let file = entries
        .iter()
        .find(|e| !e.is_dir)
        .expect("픽스처에 파일이 있다")
        .name
        .clone();
    let (_, shapes) = painted_selection(
        size,
        &crumbs,
        FilePickerMode::Open {
            selection_text: &file,
        },
        &entries,
        &file,
    );
    assert!(
        !shapes.iter().any(|(t, _, _)| t.contains("is a folder")),
        "파일을 선택했는데 폴더 안내가 표시됐다"
    );
}

/// 현재 폴더는 구분에 필요한 끝부분을 남기고 앞에 생략 표시를 붙인다.
#[test]
fn the_current_folder_elides_at_the_front_and_never_clips_without_an_ellipsis() {
    let long = "a".repeat(53);
    let crumbs = vec![
        CrumbView { label: "/".into() },
        CrumbView {
            label: long.clone(),
        },
        CrumbView {
            label: format!("{long}-tail"),
        },
    ];
    let (content, shapes) = painted(
        egui::vec2(400.0, 360.0),
        &crumbs,
        FilePickerMode::Open { selection_text: "" },
    );
    let current = shapes
        .iter()
        .map(|(t, _, _)| t.as_str())
        .find(|t| t.ends_with("-tail"))
        .unwrap_or_else(|| {
            panic!(
                "현재 폴더의 꼬리가 사라졌다 — 칠해진 것: {:?}",
                shapes.iter().map(|(t, _, _)| t).collect::<Vec<_>>()
            )
        });
    assert_ne!(
        current,
        format!("{long}-tail"),
        "테스트 경로는 이 폭에서 말줄임되어야 한다"
    );
    assert!(
        current.starts_with('…'),
        "현재 폴더가 앞 말줄임 표지 없이 잘렸다: {current:?}"
    );
    assert_fully_visible(content, &shapes, CONFIRM, "narrow");
    assert_fully_visible(content, &shapes, CANCEL, "narrow");
}

/// 구역마다 `fp-*` 인셋을 쓴다. 헤더는 8 + sm IconButton 24 + 8 = 40, 경로 막대는
/// 4 + 24 + 4 = 32이고, 헤더 글리프·첫 crumb·행 아이콘·푸터 라벨이 창 안쪽 + 12에서 시작한다.
#[test]
fn every_section_starts_on_the_fp_inset_start_column() {
    let th = crate::theme::theme();
    let size = egui::vec2(640.0, 480.0);
    let (content, shapes) = painted_shapes(
        size,
        &deep_crumbs(2, "crumb"),
        FilePickerMode::Open { selection_text: "" },
        &entries(),
        "file-1.toml",
    );
    let text_rect = |label: &str| {
        shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} 이 칠해지지 않았다"))
    };
    let fill = th.bg_sidebar().to_egui();
    let band = shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .expect("경로 막대 띠가 칠해져야 한다");
    assert_eq!(band.top() - content.top(), 40.0, "헤더 높이");
    assert_eq!(band.height(), 32.0, "경로 막대 높이");

    let start = content.left() + th.fp_inset_start().value();
    let icon_col = th.icon_glyph_size_md.value() + th.fp_section_gap().value();
    let name_col = th.icon_glyph_size_md.value() + th.spacing_sm.value();
    let near = |got: f32, want: f32, what: &str| {
        assert!((got - want).abs() <= 1.0, "{what}: x {got}, 기대 {want}");
    };
    // 글리프는 텍스트 앞 한 칸이므로 제목·행 이름 위치에서 역산한다.
    near(text_rect("title").left() - icon_col, start, "헤더 글리프");
    near(text_rect("/").left(), start, "첫 crumb");
    near(
        text_rect("file-2.toml").left() - name_col,
        start,
        "행 아이콘",
    );
    near(text_rect("File name").left(), start, "푸터 라벨");
    // 행 높이는 4 + 이름 줄(13 × 1.4) + 4다. egui 가 좌표를 1/16 단위로 맞춘다.
    let pitch = text_rect("file-3.toml").center().y - text_rect("file-2.toml").center().y;
    assert!(
        (pitch - row_height(&th).value()).abs() < 0.05 && (pitch - 26.2).abs() < 0.05,
        "행 간격 {pitch}"
    );
    near(
        content.bottom() - text_rect(CONFIRM).center().y,
        th.fp_footer_pad_y().value() + th.button_height().value() * 0.5,
        "푸터 아래 여백",
    );
}
