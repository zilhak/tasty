use super::*;
use crate::adapters::ui::preset::demo_layout::DemoLayout;
use serde_json::json;

fn cfg(kind: &str, leaf: usize) -> SurfaceCfg {
    let preset: tasty_presets::TabPreset = serde_json::from_value(json!({"name":"test","tab":{"layout":{"type":"leaf","surface":{"id":leaf,"kind":kind,"cwd":"/original"}}}})).unwrap();
    let model = DemoLayout::from_tab(&preset, &KindCatalog::default());
    SurfaceCfg::open("tab:test".into(), leaf, model.leaf_draft(leaf).unwrap())
}
fn request(cfg: &mut SurfaceCfg) -> FieldBrowseRequest {
    let fields = KindCatalog::default().fields(cfg.draft.kind());
    let field = fields
        .iter()
        .find(|f| matches!(f.input, PresetFieldInput::Dir | PresetFieldInput::FilePath))
        .unwrap();
    cfg.request_browse(field);
    cfg.take_browse_request().unwrap()
}
#[test]
fn preset_field_receipt_updates_only_its_target_once() {
    let catalog = KindCatalog::default();
    let mut cfg = cfg("terminal", 7);
    let request = request(&mut cfg);
    assert!(cfg.apply_browse(&request, "/chosen".into(), &catalog));
    assert_eq!(cfg.draft.value(&PresetFieldTarget::Cwd), "/chosen");
    assert_eq!(cfg.draft.value(&PresetFieldTarget::Startup), "");
    assert!(!cfg.apply_browse(&request, "/replay".into(), &catalog));
}
#[test]
fn preset_field_result_arriving_after_kind_change_is_dropped() {
    let catalog = KindCatalog::default();
    let mut cfg = cfg("terminal", 7);
    let request = request(&mut cfg);
    cfg.switch_kind("image", &catalog);
    cfg.switch_kind("terminal", &catalog);
    let before = cfg.draft.clone();
    assert!(!cfg.apply_browse(&request, "/late".into(), &catalog));
    assert_eq!(cfg.draft, before);
}
#[test]
fn rename_reopen_and_other_leaf_cannot_receive_old_field_results() {
    let catalog = KindCatalog::default();
    let mut original = cfg("terminal", 7);
    let request = request(&mut original);
    original.remap_preset(tasty_presets::PresetKind::Tab, "test", "renamed");
    assert!(!original.apply_browse(&request, "/late".into(), &catalog));
    for mut next in [cfg("terminal", 7), cfg("terminal", 8)] {
        let before = next.draft.clone();
        assert!(!next.apply_browse(&request, "/late".into(), &catalog));
        assert_eq!(next.draft, before);
    }
}
#[test]
fn a_new_browse_supersedes_the_old_request_and_file_fields_use_open_mode() {
    let catalog = KindCatalog::default();
    let mut cfg = cfg("image", 7);
    let old = request(&mut cfg);
    let new = request(&mut cfg);
    assert_eq!(new.mode, crate::view::file_chooser::FileChooserMode::Open);
    assert!(!cfg.apply_browse(&old, "old.png".into(), &catalog));
    assert!(cfg.apply_browse(&new, "chosen.png".into(), &catalog));
    assert_eq!(
        cfg.draft.value(&PresetFieldTarget::Params("file".into())),
        "chosen.png"
    );
}

#[test]
fn escape_in_the_chooser_does_not_cancel_the_surface_settings() {
    let ctx = egui::Context::default();
    let mut cfg = cfg("terminal", 7);
    let catalog = KindCatalog::default();
    let mut chooser = crate::view::file_chooser::WindowFileChooser::default();
    assert!(chooser.open(
        "preset.field",
        crate::view::file_chooser::FileChooserMode::Folder,
        ""
    ));
    for pressed in [false, true] {
        let mut outcome = CfgOutcome::None;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 720.0),
            )),
            ..Default::default()
        };
        if pressed {
            raw.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        drop(ctx.run(raw, |ctx| {
            chooser.mark_input_owner(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                outcome = draw_surface_settings(
                    ui,
                    &crate::theme::theme(),
                    ui.available_rect_before_wrap(),
                    &mut cfg,
                    &catalog,
                    &[],
                );
            });
            chooser.draw(ctx);
        }));
        assert!(matches!(outcome, CfgOutcome::None));
    }
    assert!(!chooser.is_open());
    assert_eq!(cfg.draft.value(&PresetFieldTarget::Cwd), "/original");
}
