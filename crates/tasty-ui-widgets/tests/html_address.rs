use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::*;

fn view() -> HtmlAddressView<'static> {
    HtmlAddressView {
        address: "https://example.com/current",
        committed_address: "https://example.com/current",
        kind: HtmlAddressKind::Web,
        can_back: false,
        can_forward: true,
        loading: false,
        disconnected: false,
        skipped: None,
        focus: false,
        labels: HtmlAddressLabels {
            placeholder: "Path or URL",
            back: "Back",
            forward: "Forward",
            reload: "Reload",
            disconnected: "Disconnected",
        },
    }
}
fn frame(
    ctx: &egui::Context,
    state: &mut HtmlAddressState,
    view: &HtmlAddressView<'_>,
    width: f32,
    events: Vec<egui::Event>,
) -> HtmlAddressOutput {
    let mut result = None;
    drop(ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 300.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                result = Some(html_address_bar(
                    ui,
                    &tasty_themes::mocha_fallback(),
                    egui::Id::new("address"),
                    LogicalPx(width),
                    state,
                    view,
                ));
            });
        },
    ));
    result.unwrap()
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn click(
    ctx: &egui::Context,
    state: &mut HtmlAddressState,
    v: &HtmlAddressView<'_>,
    p: egui::Pos2,
) -> HtmlAddressOutput {
    frame(
        ctx,
        state,
        v,
        460.0,
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        ctx,
        state,
        v,
        460.0,
        vec![egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    )
}
#[test]
fn geometry_threshold_field_floor_and_error_row() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let v = view();
    for (width, forward) in [(440.0, true), (439.0, false), (150.0, false)] {
        let out = frame(&ctx, &mut s, &v, width, vec![]);
        assert_eq!(out.rect.height(), 40.0);
        assert_eq!(out.forward_rect.is_some(), forward);
        assert!(out.field_rect.unwrap().width() >= 96.0);
        if width == 150.0 {
            assert_eq!(out.field_rect.unwrap().width(), 96.0);
        }
        assert_eq!(out.field_rect.unwrap().height(), 28.0);
        assert_eq!(out.reload_rect.unwrap().width(), 24.0);
        assert_eq!(
            out.field_rect.unwrap().left() - out.reload_rect.unwrap().right(),
            8.0
        );
    }
    s.reject("bad".into());
    assert_eq!(frame(&ctx, &mut s, &v, 460.0, vec![]).rect.height(), 64.0);
}
#[test]
fn agent_commit_preserves_draft_then_escape_uses_latest_commit() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let mut v = view();
    v.focus = true;
    frame(&ctx, &mut s, &v, 460.0, vec![]);
    v.focus = false;
    frame(
        &ctx,
        &mut s,
        &v,
        460.0,
        vec![egui::Event::Text("draft".into())],
    );
    assert_eq!(s.buffer, "draft");
    v.address = "https://example.com/agent";
    v.committed_address = v.address;
    let out = frame(&ctx, &mut s, &v, 460.0, vec![]);
    assert!(out.has_keyboard_focus);
    assert_eq!(s.buffer, "draft");
    let out = frame(&ctx, &mut s, &v, 460.0, vec![key(egui::Key::Escape)]);
    assert_eq!(out.action, HtmlAddressAction::Revert);
    assert_eq!(s.buffer, v.address);
    assert!(!out.has_keyboard_focus);
}
#[test]
fn rejected_enter_keeps_input_and_focus_then_typing_clears_error() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let mut v = view();
    v.focus = true;
    frame(&ctx, &mut s, &v, 460.0, vec![]);
    v.focus = false;
    frame(
        &ctx,
        &mut s,
        &v,
        460.0,
        vec![egui::Event::Text("/missing".into())],
    );
    let out = frame(&ctx, &mut s, &v, 460.0, vec![key(egui::Key::Enter)]);
    assert_eq!(out.action, HtmlAddressAction::Open("/missing".into()));
    assert!(out.has_keyboard_focus);
    s.reject("missing".into());
    frame(&ctx, &mut s, &v, 460.0, vec![]);
    assert_eq!(s.buffer, "/missing");
    assert!(s.error.is_some());
    frame(&ctx, &mut s, &v, 460.0, vec![egui::Event::Text("x".into())]);
    assert_eq!(s.buffer, "/missingx");
    assert!(s.error.is_none());
}
#[test]
fn mirror_is_read_only_and_placeholder_has_no_band() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let mut v = view();
    v.kind = HtmlAddressKind::Mirror;
    let out = frame(&ctx, &mut s, &v, 460.0, vec![]);
    assert!(out.forward_rect.is_none());
    click(&ctx, &mut s, &v, out.field_rect.unwrap().center());
    let out = frame(
        &ctx,
        &mut s,
        &v,
        460.0,
        vec![egui::Event::Text("replace".into()), key(egui::Key::Enter)],
    );
    assert_eq!(s.buffer, v.address);
    assert_eq!(out.action, HtmlAddressAction::None);
    v.kind = HtmlAddressKind::MirrorPlaceholder;
    let out = frame(&ctx, &mut s, &v, 460.0, vec![]);
    assert_eq!(out.rect.height(), 0.0);
    assert!(out.field_rect.is_none());
    assert!(out.reload_rect.is_none());
}
#[test]
fn loading_reload_is_not_clickable() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let mut v = view();
    v.loading = true;
    let out = frame(&ctx, &mut s, &v, 460.0, vec![]);
    let p = out.reload_rect.unwrap().center();
    assert_eq!(click(&ctx, &mut s, &v, p).action, HtmlAddressAction::None);
    v.loading = false;
    frame(&ctx, &mut s, &v, 460.0, vec![]);
    assert_eq!(click(&ctx, &mut s, &v, p).action, HtmlAddressAction::Reload);
    v.disconnected = true;
    assert_eq!(click(&ctx, &mut s, &v, p).action, HtmlAddressAction::None);
}
#[test]
fn inset_stack_has_one_top_margin_and_gaps_between_and_after() {
    let theme = tasty_themes::mocha_fallback();
    assert_eq!(
        html_address_content_inset(&theme, LogicalPx(40.0), &[]),
        LogicalPx(40.0)
    );
    assert_eq!(
        html_address_content_inset(&theme, LogicalPx(64.0), &[LogicalPx(60.0), LogicalPx(80.0)]),
        LogicalPx(228.0)
    );
}

#[test]
fn failed_attempt_does_not_become_escape_revert_address() {
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let mut v = view();
    v.address = "https://offline.example";
    v.focus = true;
    frame(&ctx, &mut s, &v, 460.0, vec![]);
    v.focus = false;
    assert_eq!(s.buffer, v.address);
    let out = frame(&ctx, &mut s, &v, 460.0, vec![key(egui::Key::Escape)]);
    assert_eq!(out.action, HtmlAddressAction::Revert);
    assert_eq!(s.buffer, v.committed_address);
}

#[test]
fn input_length_overrides_preserve_existing_defaults() {
    use std::cell::RefCell;
    let ctx = egui::Context::default();
    let theme = tasty_themes::mocha_fallback();
    let widths = RefCell::new(Vec::new());
    let icon = |_: &mut egui::Ui, r: egui::Rect, _: egui::Color32| {
        widths.borrow_mut().push(r.width());
    };
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            Input::new()
                .mono(true)
                .icon(&icon)
                .show(ui, &theme, &mut "default-input".into());
            Input::new()
                .mono(true)
                .font_size(theme.input_font_size())
                .icon_size(theme.icon_glyph_size_sm)
                .icon(&icon)
                .show(ui, &theme, &mut "html-input".into());
        });
    });
    assert_eq!(
        *widths.borrow(),
        vec![
            theme.icon_glyph_size_md.value(),
            theme.icon_glyph_size_sm.value()
        ]
    );
    let sizes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if ["default-input", "html-input"].contains(&t.galley.text()) => {
                Some(t.galley.job.sections[0].format.font_id.size)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes,
        vec![
            theme.font_size_caption.value(),
            theme.input_font_size().value()
        ]
    );
}

fn latte() -> tasty_type_appearance::theme::Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte.toml");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    tasty_type_appearance::theme::Theme::with_colors(colors, is_light.unwrap_or(true))
}

/// The ink of the painted address text, read from the frame's text shapes.
fn address_ink(
    theme: &tasty_type_appearance::theme::Theme,
    state: &mut HtmlAddressState,
    v: &HtmlAddressView<'_>,
) -> egui::Color32 {
    let ctx = egui::Context::default();
    let mut found = None;
    for _ in 0..2 {
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    html_address_bar(
                        ui,
                        theme,
                        egui::Id::new("address"),
                        LogicalPx(800.0),
                        state,
                        v,
                    );
                });
            },
        );
        found = out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == v.address => {
                Some(t.override_text_color.unwrap_or(t.fallback_color))
            }
            _ => None,
        });
    }
    found.expect("address text painted")
}

#[test]
fn idle_editing_and_mirror_values_use_their_own_ink_roles() {
    let theme = latte();
    // In Latte the idle value role differs from the old text-secondary ink.
    assert_ne!(theme.html_address_value_fg(), theme.text_secondary());
    let mut idle = HtmlAddressState::default();
    assert_eq!(
        address_ink(&theme, &mut idle, &view()),
        theme.html_address_value_fg().to_egui()
    );
    let mut editing = HtmlAddressState {
        buffer: "https://example.com/current".into(),
        editing: true,
        error: None,
    };
    assert_eq!(
        address_ink(&theme, &mut editing, &view()),
        theme.input_fg().to_egui()
    );
    let mirror = HtmlAddressView {
        kind: HtmlAddressKind::Mirror,
        ..view()
    };
    assert_eq!(
        address_ink(&theme, &mut HtmlAddressState::default(), &mirror),
        theme.input_readonly_fg().to_egui()
    );
}

/// 읽기 전용 mirror의 긴 주소는 말줄임 라벨이 아니라 원문 전체를 첫 포커스 프레임부터 복사한다.
/// 붙여넣기·Enter는 값을 바꾸거나 이동하지 않는다.
#[test]
fn a_long_mirror_address_copies_the_full_original_from_the_first_click() {
    let address = "https://example.com/문서/아주/긴/경로/with%20space/and/more/segments/report.html?q=한국어&x=1#section-끝";
    let mut v = view();
    v.kind = HtmlAddressKind::Mirror;
    v.address = address;
    v.committed_address = address;
    let ctx = egui::Context::default();
    let mut s = HtmlAddressState::default();
    let out = frame(&ctx, &mut s, &v, 460.0, vec![]);
    let p = out.field_rect.unwrap().center();
    let copied = |events: Vec<egui::Event>, s: &mut HtmlAddressState| {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    html_address_bar(
                        ui,
                        &tasty_themes::mocha_fallback(),
                        egui::Id::new("address"),
                        LogicalPx(460.0),
                        s,
                        &v,
                    );
                });
            },
        );
        output
            .platform_output
            .commands
            .iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
    };
    let press = |pressed| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    assert_eq!(
        copied(vec![egui::Event::PointerMoved(p), press(true)], &mut s),
        None
    );
    let select_all = egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    assert_eq!(
        copied(vec![press(false), select_all, egui::Event::Copy], &mut s).as_deref(),
        Some(address)
    );
    let out = frame(
        &ctx,
        &mut s,
        &v,
        460.0,
        vec![
            egui::Event::Paste("https://other.example".into()),
            egui::Event::Cut,
            key(egui::Key::Enter),
        ],
    );
    assert_eq!(s.buffer, address);
    assert_eq!(out.action, HtmlAddressAction::None);
}
