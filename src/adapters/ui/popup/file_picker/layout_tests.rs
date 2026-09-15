//! 레이아웃 성질을 **칠해진 결과**로 고정한다 — 버튼이 보이는가는 view 가 돌려주는 값이
//! 아니라 화면에 칠해진 텍스트가 popup 콘텐츠 영역 안에 온전히 들어갔는가로만 정직하게 잴 수
//! 있다(`remote_tool` 의 `painted_text` 와 같은 관찰점).
use super::path_bar::{CrumbSlot, crumb_slots};
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
    let th = crate::theme::theme();
    let ctx = egui::Context::default();
    let content = content_rect(size);
    let selected = vec!["file-1.toml".to_string()];
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
        hidden_folders_label: "{} hidden",
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

#[test]
fn crumb_slots_cover_every_ancestor_exactly_once() {
    for len in 0..12 {
        for elide in [false, true] {
            let mut seen = Vec::new();
            for slot in crumb_slots(len, elide) {
                match slot {
                    CrumbSlot::Crumb(i) => seen.push(i),
                    CrumbSlot::Hidden(r) => seen.extend(r),
                }
            }
            assert_eq!(
                seen,
                (0..len).collect::<Vec<_>>(),
                "len={len} elide={elide}"
            );
        }
    }
    assert_eq!(
        crumb_slots(8, true),
        vec![
            CrumbSlot::Crumb(0),
            CrumbSlot::Hidden(1..6),
            CrumbSlot::Crumb(6),
            CrumbSlot::Crumb(7),
        ]
    );
    assert_eq!(
        crumb_slots(3, true).len(),
        3,
        "접을 것이 없으면 접지 않는다"
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
