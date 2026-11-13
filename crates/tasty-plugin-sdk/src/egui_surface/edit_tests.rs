//! 편집 요청 회신 시험. 와이어 요청을 직접 만들어 surface 에 넣고 회신을 읽는다.

use super::*;
use crate::egui_surface::EguiMeshSurface;
use tasty_plugin_protocol::SurfaceSetContextParams;

const SALT: &str = "edit_field";

#[derive(Clone, Copy, Default)]
struct Field {
    read_only: bool,
    password: bool,
    /// 어댑터 대신 egui `TextEdit` 을 직접 그린다.
    plain: bool,
    /// `plain` 입력칸을 [`EditProbe`] 로 감싼다.
    probe: bool,
}

fn params(events: Vec<RawInputEventWire>) -> SurfaceSetContextParams {
    SurfaceSetContextParams {
        surface_id: 1,
        width_px: 400,
        height_px: 100,
        pixels_per_point: 1.0,
        raw_input: RawInputWire {
            events,
            ..Default::default()
        },
        theme: None,
        need_full_textures: false,
    }
}

/// 포커스를 가진 입력칸 하나를 그리고, `select` 가 있으면 그 선택을 매 패스 뒤에 다시 넣는다.
fn field_ui(
    buf: &mut String,
    field: Field,
    select: Option<(usize, usize)>,
) -> impl FnMut(&Context) + '_ {
    move |ctx: &Context| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut output = if field.plain {
                let id = ui.make_persistent_id(SALT);
                let probe = field
                    .probe
                    .then(|| EditProbe::begin(ui.ctx(), id, buf, field.read_only));
                let output = if field.read_only {
                    let mut view: &str = buf.as_str();
                    TextEdit::singleline(&mut view).id(id).show(ui)
                } else {
                    TextEdit::singleline(&mut *buf).id(id).show(ui)
                };
                if let Some(probe) = probe {
                    probe.finish(ui.ctx(), buf);
                }
                output
            } else {
                EditField::singleline(&mut *buf)
                    .id_salt(SALT)
                    .read_only(field.read_only)
                    .password(field.password)
                    .show(ui, |t| t)
            };
            let id = output.response.id;
            ui.ctx().data_mut(|d| d.insert_temp(field_id_key(), id));
            ui.memory_mut(|m| m.request_focus(id));
            if let Some((start, end)) = select {
                let range = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(end),
                );
                output.state.cursor.set_char_range(Some(range));
                output.state.store(ui.ctx(), id);
            }
        });
    }
}

fn field_id_key() -> Id {
    Id::new("edit_tests.field_id")
}

/// 입력칸의 선택을 (secondary, primary)로 읽는다. `field_ui`의 `select`와 같은 순서다.
fn selection(surface: &EguiMeshSurface) -> Option<(usize, usize)> {
    let ctx = surface.context();
    let id = ctx.data(|d| d.get_temp::<Id>(field_id_key()))?;
    let range = TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    Some((range.secondary.index, range.primary.index))
}

fn request(edit_id: u64, edit: EditRequestWire) -> RawInputEventWire {
    RawInputEventWire::EditRequest { edit_id, edit }
}

/// 입력칸에 포커스와 선택을 준 뒤 `events` 한 프레임을 넣고 회신을 꺼낸다. 선택은 요청 프레임에는 다시 넣지 않는다.
fn run(
    buf: &mut String,
    field: Field,
    select: Option<(usize, usize)>,
    events: Vec<RawInputEventWire>,
) -> (EguiMeshSurface, Vec<(u64, EditResultWire, Option<String>)>) {
    let mut surface = EguiMeshSurface::new(1);
    for _ in 0..2 {
        surface.run_frame(&params(Vec::new()), field_ui(buf, field, select));
    }
    surface.run_frame(&params(events), field_ui(buf, field, None));
    let outcomes = surface.take_edit_outcomes();
    (surface, outcomes)
}

fn acted(edit_id: u64, copied: Option<&str>) -> (u64, EditResultWire, Option<String>) {
    (edit_id, EditResultWire::Acted, copied.map(str::to_string))
}

fn not_acted(edit_id: u64) -> (u64, EditResultWire, Option<String>) {
    (edit_id, EditResultWire::NotActed, None)
}

/// 선택이 있으면 복사는 실행이고 복사 글자가 회신에 실린다. 그 복사는 요청과 무관한 복사로 따로 나가지 않는다.
#[test]
fn a_copy_with_a_selection_acts_and_carries_the_text() {
    let mut buf = "hello world".to_string();
    let events = vec![request(1, EditRequestWire::Copy)];
    let (mut surface, outcomes) = run(&mut buf, Field::default(), Some((0, 5)), events);
    assert_eq!(outcomes, vec![acted(1, Some("hello"))]);
    assert_eq!(surface.take_copied_text(), None);
}

/// 선택이 없는 복사는 실행 안 함이다. 호스트는 원래 키를 넘긴다.
#[test]
fn a_copy_without_a_selection_does_not_act() {
    let mut buf = "hello world".to_string();
    let events = vec![request(2, EditRequestWire::Copy)];
    let (_, outcomes) = run(&mut buf, Field::default(), None, events);
    assert_eq!(outcomes, vec![not_acted(2)]);
}

/// 편집 가능한 칸의 잘라내기는 선택을 지우고 실행이며 지운 글자를 싣는다.
#[test]
fn a_cut_in_an_editable_field_removes_the_selection() {
    let mut buf = "hello world".to_string();
    let events = vec![request(3, EditRequestWire::Cut)];
    let (_, outcomes) = run(&mut buf, Field::default(), Some((0, 5)), events);
    assert_eq!(outcomes, vec![acted(3, Some("hello"))]);
    assert_eq!(buf, " world");
}

/// 읽기 전용 칸의 잘라내기는 입력칸에 가지 않는다. 실행 안 함이고 복사도 나지 않는다.
#[test]
fn a_cut_in_a_read_only_field_does_not_act_or_copy() {
    let mut buf = "hello world".to_string();
    let field = Field {
        read_only: true,
        ..Field::default()
    };
    let events = vec![request(4, EditRequestWire::Cut)];
    let (mut surface, outcomes) = run(&mut buf, field, Some((0, 5)), events);
    assert_eq!(outcomes, vec![not_acted(4)]);
    assert_eq!(surface.take_copied_text(), None, "egui 복사도 나지 않는다");
    assert_eq!(buf, "hello world");
}

/// 비밀번호 칸의 잘라내기는 지우기만 하고 실행이다. 복사는 egui 가 막고, 복사도 실행 안 함이다.
#[test]
fn a_password_field_cuts_without_copying_and_does_not_copy() {
    let field = Field {
        password: true,
        ..Field::default()
    };
    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        field,
        Some((0, 5)),
        vec![request(5, EditRequestWire::Cut)],
    );
    assert_eq!(outcomes, vec![acted(5, None)]);
    assert_eq!(buf, " world");

    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        field,
        Some((0, 5)),
        vec![request(6, EditRequestWire::Copy)],
    );
    assert_eq!(outcomes, vec![not_acted(6)]);
}

/// 붙여넣기는 버퍼가 바뀌면 실행이다. 같은 글자로 덮어써 버퍼가 같아도 커서가 뒤로 가므로 실행이다.
/// 읽기 전용 칸에는 들어가지 않아 실행 안 함이다.
#[test]
fn a_paste_acts_when_the_field_takes_it() {
    let paste = |text: &str| EditRequestWire::PasteText { text: text.into() };
    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        Field::default(),
        Some((0, 5)),
        vec![request(7, paste("bye"))],
    );
    assert_eq!(outcomes, vec![acted(7, None)]);
    assert_eq!(buf, "bye world");

    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        Field::default(),
        Some((0, 5)),
        vec![request(8, paste("hello"))],
    );
    assert_eq!(outcomes, vec![acted(8, None)]);
    assert_eq!(buf, "hello world");

    let mut buf = "hello world".to_string();
    let field = Field {
        read_only: true,
        ..Field::default()
    };
    let (_, outcomes) = run(&mut buf, field, Some((0, 5)), vec![request(9, paste("x"))]);
    assert_eq!(outcomes, vec![not_acted(9)]);
    assert_eq!(buf, "hello world");
}

/// 같은 프레임의 앞뒤 글자 입력은 요청 패스 밖에서 처리되어, 그 변화가 붙여넣기의 결과로 세어지지 않는다.
/// 내용 없는 형식 붙여넣기는 입력칸에 효과가 없으므로 실행 안 함이고, 글자는 그대로 들어간다.
#[test]
fn text_typed_around_a_paste_is_not_counted_as_its_result() {
    let text = |t: &str| RawInputEventWire::Text { text: t.into() };
    let mut buf = String::new();
    let events = vec![text("a"), request(10, EditRequestWire::Paste), text("b")];
    let (_, outcomes) = run(&mut buf, Field::default(), None, events);
    assert_eq!(outcomes, vec![not_acted(10)]);
    assert_eq!(buf, "ab");
}

/// 어댑터를 쓰지 않은 입력칸의 잘라내기는 모름이다. 복사는 일어났으므로 회신에 실린다.
#[test]
fn a_cut_outside_the_adapter_is_unknown() {
    let mut buf = "hello world".to_string();
    let field = Field {
        plain: true,
        ..Field::default()
    };
    let events = vec![request(11, EditRequestWire::Cut)];
    let (_, outcomes) = run(&mut buf, field, Some((0, 5)), events);
    assert_eq!(
        outcomes,
        vec![(11, EditResultWire::Unknown, Some("hello".into()))]
    );
}

/// 모르는 동작은 실행하지 않고 실행 안 함으로 회신한다.
#[test]
fn an_unknown_edit_does_not_act() {
    let mut buf = "hello world".to_string();
    let events = vec![request(12, EditRequestWire::Unknown)];
    let (_, outcomes) = run(&mut buf, Field::default(), Some((0, 5)), events);
    assert_eq!(outcomes, vec![not_acted(12)]);
}

/// 한 프레임의 요청마다 회신이 받은 순서대로 하나씩 나간다.
#[test]
fn each_request_in_a_frame_gets_one_reply() {
    let mut buf = "hello world".to_string();
    let events = vec![
        request(13, EditRequestWire::Copy),
        request(14, EditRequestWire::Cut),
    ];
    let (mut surface, outcomes) = run(&mut buf, Field::default(), Some((0, 5)), events);
    assert_eq!(
        outcomes,
        vec![acted(13, Some("hello")), acted(14, Some("hello"))]
    );
    assert!(surface.take_edit_outcomes().is_empty(), "한 번만 나간다");
}

/// 패스 밖에서 처리한 붙여넣기는 알린 값으로, 패스 안에서 처리한 것은 `report_edit` 값으로 회신한다.
#[test]
fn a_reported_result_answers_the_request() {
    let mut surface = EguiMeshSurface::new(1);
    let input = params(vec![request(15, EditRequestWire::Paste)]);
    let ids: Vec<u64> = edit_requests(&input.raw_input).map(|(id, _)| id).collect();
    assert_eq!(ids, [15]);
    surface.report_edit_outcome(15, true);
    surface.run_frame(&input, |_| {});
    assert_eq!(surface.take_edit_outcomes(), vec![acted(15, None)]);

    let input = params(vec![request(16, EditRequestWire::Paste)]);
    let mut seen = None;
    surface.run_frame(&input, |ctx| {
        if let Some(req) = edit_request(ctx) {
            seen = Some(req.edit_id);
            report_edit(ctx, true);
        }
    });
    assert_eq!(seen, Some(16));
    assert_eq!(surface.take_edit_outcomes(), vec![acted(16, None)]);
    let after = surface.context().clone();
    assert_eq!(edit_request(&after), None, "요청 패스가 끝나면 지워진다");
}

/// 어댑터 밖 붙여넣기는 모름이다. 지난 렌더에 알린 값이 다음 요청에 남지 않는다.
#[test]
fn a_report_for_another_frame_is_not_reused() {
    let mut surface = EguiMeshSurface::new(1);
    surface.report_edit_outcome(20, true);
    surface.run_frame(&params(Vec::new()), |_| {});
    surface.run_frame(&params(vec![request(20, EditRequestWire::Paste)]), |_| {});
    assert_eq!(
        surface.take_edit_outcomes(),
        vec![(20, EditResultWire::Unknown, None)]
    );
}

/// 다른 입력칸 위젯을 [`EditProbe`] 로 감싸면 어댑터와 같이 잘라내기·붙여넣기 결과를 정한다.
#[test]
fn a_probe_judges_a_field_drawn_by_another_widget() {
    let field = Field {
        plain: true,
        probe: true,
        ..Field::default()
    };
    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        field,
        Some((0, 5)),
        vec![request(21, EditRequestWire::Cut)],
    );
    assert_eq!(outcomes, vec![acted(21, Some("hello"))]);

    let mut buf = "hello world".to_string();
    let paste = EditRequestWire::PasteText {
        text: "hello".into(),
    };
    let (_, outcomes) = run(&mut buf, field, Some((0, 5)), vec![request(22, paste)]);
    assert_eq!(
        outcomes,
        vec![acted(22, None)],
        "같은 글자로 덮어써도 커서가 움직인다"
    );

    let mut buf = "hello world".to_string();
    let (_, outcomes) = run(
        &mut buf,
        field,
        None,
        vec![request(23, EditRequestWire::Paste)],
    );
    assert_eq!(outcomes, vec![not_acted(23)]);
}

/// 내용 없는 붙여넣기(형식 붙여넣기, 빈 글자)는 egui 가 무시하므로 선택 방향과 관계없이 실행 안 함이다.
/// 정방향 선택은 커서(primary)가 선택 시작보다 뒤에 있어도 그것이 붙여넣기의 결과는 아니다.
#[test]
fn an_empty_paste_does_not_act_whatever_the_selection_direction() {
    let probe = Field {
        plain: true,
        probe: true,
        ..Field::default()
    };
    let empty = EditRequestWire::PasteText {
        text: String::new(),
    };
    let mut edit_id = 30;
    for field in [Field::default(), probe] {
        for select in [(0, 5), (5, 0)] {
            for edit in [EditRequestWire::Paste, empty.clone()] {
                edit_id += 1;
                let mut buf = "hello world".to_string();
                let (surface, outcomes) =
                    run(&mut buf, field, Some(select), vec![request(edit_id, edit)]);
                assert_eq!(
                    outcomes,
                    vec![not_acted(edit_id)],
                    "probe={} select={select:?}",
                    field.probe
                );
                assert_eq!(buf, "hello world");
                assert_eq!(selection(&surface), Some(select), "선택도 그대로다");
            }
        }
    }
}

/// 읽기 전용 칸의 잘라내기는 위젯에 가기 전에 걷어내므로 선택이 남는다. 호스트가 넘긴 원래 키가 다시 와도
/// 복사할 수 있다. 요청이 없거나 복사 요청이면 선택이 그대로인 것과 같다.
#[test]
fn a_cut_in_a_read_only_field_keeps_the_selection() {
    let adapter = Field {
        read_only: true,
        ..Field::default()
    };
    let probe = Field {
        read_only: true,
        plain: true,
        probe: true,
        ..Field::default()
    };
    for field in [adapter, probe] {
        let mut buf = "hello world".to_string();
        let (surface, outcomes) = run(&mut buf, field, Some((0, 5)), Vec::new());
        assert!(outcomes.is_empty());
        assert_eq!(selection(&surface), Some((0, 5)), "대조군: 요청 없음");

        let (surface, outcomes) = run(
            &mut buf,
            field,
            Some((0, 5)),
            vec![request(40, EditRequestWire::Copy)],
        );
        assert_eq!(outcomes, vec![acted(40, Some("hello"))]);
        assert_eq!(selection(&surface), Some((0, 5)), "대조군: 복사");

        let (mut surface, outcomes) = run(
            &mut buf,
            field,
            Some((0, 5)),
            vec![request(41, EditRequestWire::Cut)],
        );
        assert_eq!(outcomes, vec![not_acted(41)], "probe={}", field.probe);
        assert_eq!(surface.take_copied_text(), None);
        assert_eq!(buf, "hello world");
        assert_eq!(
            selection(&surface),
            Some((0, 5)),
            "probe={}: 잘라내기 뒤에도 선택이 남는다",
            field.probe
        );
    }
}
