//! Detectors · Handlers 서브탭을 egui 프레임으로 그려 보고 실제 클릭으로 조작하는 시험.

use super::*;
use std::sync::Arc;

/// 테스트에서는 번역 키가 그대로 그려져 글자가 길다. 행 글자가 Switch를 덮지 않도록 넓게 둔다.
const SCREEN: egui::Vec2 = egui::vec2(1600.0, 1200.0);

struct Painted {
    texts: Vec<(String, egui::Rect)>,
}

impl Painted {
    fn has(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _)| t == text)
    }

    /// 같은 글자가 여럿이면 가장 아래 것 — 머리의 Add와 카드 버튼 줄의 Add가 같은 글자다.
    fn lowest(&self, text: &str) -> egui::Pos2 {
        self.texts
            .iter()
            .filter(|(t, _)| t == text)
            .map(|(_, r)| r.center())
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap_or_else(|| panic!("{text} 글자가 그려지지 않았다"))
    }
}

fn painted(shapes: Vec<egui::epaint::ClippedShape>) -> Painted {
    let mut texts = Vec::new();
    let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
    while let Some(shape) = stack.pop() {
        match shape {
            egui::Shape::Vec(inner) => stack.extend(inner),
            egui::Shape::Text(text) => texts.push((
                text.galley.text().to_string(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => {}
        }
    }
    Painted { texts }
}

/// 같은 Context로 탭을 그린다. 이동·누름·뗌을 프레임마다 나눠 실제 입력처럼 클릭한다.
struct Harness {
    ctx: egui::Context,
}

impl Harness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
        }
    }

    fn frame(&self, events: Vec<egui::Event>, draw: &mut dyn FnMut(&mut egui::Ui)) -> Painted {
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| draw(ui));
            },
        );
        painted(output.shapes)
    }

    fn click(&self, at: egui::Pos2, draw: &mut dyn FnMut(&mut egui::Ui)) -> Painted {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![egui::Event::PointerMoved(at)], draw);
        self.frame(vec![button(true)], draw);
        self.frame(vec![button(false)], draw);
        // 클릭이 바꾼 초안을 다음 프레임이 그린다.
        self.frame(Vec::new(), draw)
    }
}

fn format_catalog() -> FileFormatRegistry {
    let reg = Arc::new(tasty_file_format::FileFormatRegistry::new());
    reg.install_host_defaults(
        "[[detector]]\nid = \"html\"\n[[detector.rule]]\nkind = \"extension\"\nvalues = [\"html\"]\n",
    );
    FileFormatRegistry::new(reg)
}

fn handler_catalog() -> FileHandlerRegistry {
    FileHandlerRegistry::new(Arc::new(tasty_file_handler::FileHandlerRegistry::new()))
}

/// 빈 카드는 입력 칸 placeholder를 그리고 Add는 눌러도 아무것도 하지 않는다.
/// id와 확장자를 넣으면 Add가 초안에 넣고 카드를 닫으며, 대기 행의 Undo가 그 추가를 되돌린다.
#[test]
fn detector_card_placeholders_disabled_add_and_undo() {
    let format = format_catalog();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: AddDetectorForm {
            open: true,
            ..AddDetectorForm::default()
        },
        ..FileHandlerEditDraft::default()
    };
    let h = Harness::new();
    let add = t("settings.file_handler.detectors.add");
    let undo = t("settings.file_handler.common.undo");

    let p = h.frame(Vec::new(), &mut |ui| draw_detectors(ui, &mut fh, &format));
    assert!(p.has("my-detector"), "id placeholder");
    assert!(p.has("md, mdx"), "extensions placeholder");
    assert!(p.has("Dockerfile"), "path glob placeholder");
    let at = p.lowest(add);
    h.click(at, &mut |ui| draw_detectors(ui, &mut fh, &format));
    assert!(
        fh.add_detector_form.open,
        "빈 카드의 Add는 disabled라 카드가 그대로다"
    );
    assert!(
        !fh.add_detector_form.tried,
        "disabled Add는 오류 표시도 켜지 않는다"
    );
    assert!(fh.add_detector.is_empty());

    fh.add_detector_form.id_input = "my-doc".into();
    fh.add_detector_form.extensions_input = "doc".into();
    let p = h.frame(Vec::new(), &mut |ui| draw_detectors(ui, &mut fh, &format));
    let at = p.lowest(add);
    let p = h.click(at, &mut |ui| draw_detectors(ui, &mut fh, &format));
    assert_eq!(fh.add_detector.len(), 1);
    assert!(!fh.add_detector_form.open, "Add에 성공하면 카드가 닫힌다");
    assert!(p.has("my-doc"), "새 행");
    let at = p.lowest(undo);
    let p = h.click(at, &mut |ui| draw_detectors(ui, &mut fh, &format));
    assert!(fh.add_detector.is_empty(), "Undo가 추가 대기를 지운다");
    assert!(!p.has(undo));
    assert!(!p.has("my-doc"));
}

/// Method 행은 동작 종류가 Ipc일 때만 있다. Ipc인데 method가 비면 Add가 disabled다.
#[test]
fn handler_card_shows_the_ipc_method_row_only_for_ipc() {
    let format = format_catalog();
    let handlers = handler_catalog();
    let method = t("settings.file_handler.handlers.field_ipc_method");
    let add = t("settings.file_handler.handlers.add");
    let mut fh = FileHandlerEditDraft {
        add_handler_form: AddHandlerForm {
            open: true,
            short_name_input: "my-viewer".into(),
            detector: Some("html".into()),
            action_kind: AddHandlerActionKind::System,
            ..AddHandlerForm::default()
        },
        ..FileHandlerEditDraft::default()
    };
    let h = Harness::new();
    let draw = |ui: &mut egui::Ui, fh: &mut FileHandlerEditDraft| {
        draw_handlers(ui, fh, &format, &handlers)
    };

    let p = h.frame(Vec::new(), &mut |ui| draw(ui, &mut fh));
    assert!(!p.has(method), "System에는 Method 행이 없다");
    assert!(p.has("100"), "priority placeholder");

    fh.add_handler_form.action_kind = AddHandlerActionKind::Ipc;
    let p = h.frame(Vec::new(), &mut |ui| draw(ui, &mut fh));
    assert!(p.has(method), "Ipc에는 Method 행이 있다");
    assert!(p.has("com.example.foo.open"), "method placeholder");
    let at = p.lowest(add);
    h.click(at, &mut |ui| draw(ui, &mut fh));
    assert!(fh.add_handler.is_empty(), "method가 비면 Add가 disabled다");
    assert!(!fh.add_handler_form.tried);

    fh.add_handler_form.action_ipc_method = "com.example.foo.open".into();
    let p = h.frame(Vec::new(), &mut |ui| draw(ui, &mut fh));
    let at = p.lowest(add);
    h.click(at, &mut |ui| draw(ui, &mut fh));
    assert_eq!(fh.add_handler.len(), 1);
    assert!(!fh.add_handler_form.open);
}

/// 추가 카드는 surface-raised다. 카드 안 Input·Select는 raised 상자(bg-panel)로 그린다.
/// panel 상자 색은 카드 배경과 같아 "쓰지 않음"은 채움만으로 구분할 수 없어 단언하지 않는다.
#[test]
fn card_fields_use_the_raised_box() {
    let format = format_catalog();
    let handlers = handler_catalog();
    let th = crate::theme::theme();
    let mut fh = FileHandlerEditDraft {
        add_handler_form: AddHandlerForm {
            open: true,
            ..AddHandlerForm::default()
        },
        ..FileHandlerEditDraft::default()
    };
    let h = Harness::new();
    let out = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| draw_handlers(ui, &mut fh, &format, &handlers));
        },
    );
    let mut fills = Vec::new();
    let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = stack.pop() {
        match s {
            egui::Shape::Vec(v) => stack.extend(v),
            egui::Shape::Rect(r) => fills.push(r.fill),
            _ => {}
        }
    }
    let raised_input = th.input_raised_bg().to_egui();
    let raised_select = th.select_raised_bg().to_egui();
    assert!(fills.contains(&raised_input), "Input raised 상자");
    assert!(fills.contains(&raised_select), "Select raised 상자");
    let panel = th.input_bg().to_egui();
    assert_ne!(
        panel, raised_input,
        "두 상자가 같으면 이 시험은 구분하지 못한다"
    );
}
