use egui::{Context, Id, RawInput, Rect, Shape, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    NotificationLabels, NotificationPanelState, NotificationRow, NotificationTone,
    notification_panel,
};

type ScrollState = egui::containers::scroll_area::State;

fn row(id: u64, title: &'static str) -> NotificationRow<'static> {
    NotificationRow {
        id,
        title,
        tone: NotificationTone::Info,
        unread: true,
        body: "A body that wraps inside the narrow popup and can be expanded.",
        source: "workspace",
        source_closed: false,
        time: "1m",
    }
}

fn frame(
    ctx: &Context,
    panel: &mut NotificationPanelState,
    rows: &[NotificationRow<'_>],
) -> (Id, Vec<egui::epaint::ClippedShape>) {
    let th = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let labels = NotificationLabels {
        levels: ["Errors", "Warnings", "Info"],
        no_unread: "No unread",
        empty: "Empty",
        mark_all: "Mark all read",
        mark_read: "Read",
        jump: "Jump",
        closed: "Closed",
        source_closed: "Source closed",
        more: "More",
        less: "Less",
    };
    let mut id = Id::NULL;
    let output = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(352.0, 300.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                id = ui.make_persistent_id(Id::new("scroll_area"));
                assert!(
                    notification_panel(ui, &th, rows, &labels, panel, false).is_empty(),
                    "rendering must not acknowledge notifications"
                );
            });
        },
    );
    (id, output.shapes)
}

fn title_y(shapes: &[egui::epaint::ClippedShape], title: &str) -> f32 {
    shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.job.text == title => Some(text.pos.y),
            _ => None,
        })
        .expect("the anchored title must be painted")
}

#[test]
fn arrival_keeps_the_visible_item_and_closing_the_stage_discards_the_anchor() {
    let ctx = Context::default();
    let mut panel = NotificationPanelState::default();
    let mut rows = vec![
        row(4, "Fourth"),
        row(3, "Third"),
        row(2, "Second"),
        row(1, "First"),
    ];
    let (id, _) = frame(&ctx, &mut panel, &rows);
    let mut scroll = ScrollState::load(&ctx, id).expect("scroll state");
    scroll.offset.y = 100.0;
    scroll.store(&ctx, id);
    let (_, before) = frame(&ctx, &mut panel, &rows);
    assert!(ScrollState::load(&ctx, id).unwrap().offset.y > 0.0);
    rows.insert(0, row(5, "Fifth"));
    rows.pop();
    let (_, after) = frame(&ctx, &mut panel, &rows);
    assert!(
        (title_y(&before, "Third") - title_y(&after, "Third")).abs() < 0.1,
        "arrival and FIFO eviction must preserve the visible row"
    );
    ctx.data_mut(|data| data.remove::<ScrollState>(id));
    rows.insert(0, row(6, "Sixth"));
    frame(&ctx, &mut panel, &rows);
    assert_eq!(
        ScrollState::load(&ctx, id).unwrap().offset.y,
        0.0,
        "opening after stage close starts at the top"
    );
}
