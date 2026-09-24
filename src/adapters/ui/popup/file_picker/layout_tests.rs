//! 실제 그린 텍스트와 clip 영역을 비교해 버튼·라벨이 보이는지 확인한다.
use super::*;

const CONFIRM: &str = "CONFIRM-LABEL";
const CANCEL: &str = "CANCEL-LABEL";

/// 메인 피커 PopupDef 와 같은 크기의 popup 이 실제로 내주는 콘텐츠 사각형(타이틀바·여백 제외).
fn content_rect(size: egui::Vec2) -> egui::Rect {
    crate::adapters::ui::popup::PopupState::new(FILE_PICKER_POPUP_ID, "t", size).content_rect()
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
    let th = crate::theme::theme();
    let ctx = egui::Context::default();
    let content = content_rect(size);
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
        out.clear();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2000.0, 1500.0),
            )),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| {
            egui::Area::new(egui::Id::new("fp_layout_test"))
                .fixed_pos(content.min)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_min_size(content.size());
                    ui.set_max_size(content.size());
                    ui.set_clip_rect(content);
                    draw_file_picker_view(ui, &props);
                });
        });
        for clipped in full.shapes.iter() {
            if let egui::epaint::Shape::Text(t) = &clipped.shape {
                let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                out.push((t.galley.text().to_string(), rect, clipped.clip_rect));
            }
        }
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
