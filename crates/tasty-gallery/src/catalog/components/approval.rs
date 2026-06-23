//! Approval popup view 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/approval.rs::draw_approval_view` 와 동일한 시각
//! layout 을 로컬 mock 으로 재현. AppState/CoreState/`tasty_approval` 비의존
//! 이라는 props 분리 성과를 가시화한다.
//!
//! 본체 의존: 0. 본체 view 변경 시 시각 동기화는 수동 검증 (gallery 가
//! binary crate `tasty` 에 의존 불가하므로 enum/struct/상수 로컬 복제).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

use crate::catalog::popup_frame::{self, CONTENT_MARGIN, ContentInset, TITLE_BAR_HEIGHT};

/// 본체 `tasty_approval::Severity` 와 동등한 로컬 mock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockSeverity {
    Info,
    Warn,
    Danger,
}

/// 본체 `ApprovalChoiceView` 와 동등.
#[derive(Debug, Clone)]
struct MockChoiceView {
    label: String,
    destructive: bool,
}

/// 본체 `ApprovalProps` 와 동등. `comment_buffer` 는 mock 에서 매 frame 새로
/// 만들어지므로 `RefCell` 로 감싸 동일 demo 안에서도 갱신 가능.
struct MockApprovalProps<'a> {
    id: String,
    severity: MockSeverity,
    severity_label: &'a str,
    comment_label: &'a str,
    comment_hint: &'a str,
    body: Option<&'a str>,
    choices: Vec<MockChoiceView>,
    comment_buffer: &'a RefCell<String>,
}

/// 본체 `draw_approval_view` 와 동등한 시각.
///
/// Gallery 는 action 을 무시 (단독 시각 검증 목적). 키보드 단축키 동작 mirroring
/// 은 생략 — gallery container 가 popup 이 아니라 일반 ui 영역이라 키 입력이
/// 다른 panel 로 가는 게 자연스러움.
fn draw_mock_approval_view(ui: &mut egui::Ui, theme: &Theme, props: &MockApprovalProps<'_>) {
    let sev_color: egui::Color32 = match props.severity {
        MockSeverity::Info => theme.subtext0.into(),
        MockSeverity::Warn => theme.accent_warning().into(),
        MockSeverity::Danger => theme.accent_danger().into(),
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(props.severity_label)
                .size(theme.font_size_caption.value())
                .color(sev_color),
        );
        ui.label(
            egui::RichText::new(format!("· {}", props.id))
                .size(theme.font_size_caption.value())
                .color(egui::Color32::from(theme.subtext0)),
        );
    });
    ui.add_space(6.0);

    if let Some(body) = props.body {
        ui.label(
            egui::RichText::new(body)
                .color(egui::Color32::from(theme.text))
                .size(theme.font_size_body.value()),
        );
        ui.add_space(8.0);
    }

    ui.label(
        egui::RichText::new(props.comment_label)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
    let mut buf = props.comment_buffer.borrow_mut();
    ui.add(
        egui::TextEdit::singleline(&mut *buf)
            .desired_width(ui.available_width())
            .hint_text(props.comment_hint),
    );
    drop(buf);
    ui.add_space(8.0);

    ui.horizontal_wrapped(|ui| {
        for (idx, choice) in props.choices.iter().enumerate() {
            let label_text = if idx < 9 {
                format!("{} ({})", choice.label, idx + 1)
            } else {
                choice.label.clone()
            };
            let mut btn =
                egui::Button::new(egui::RichText::new(label_text).size(theme.font_size_body.value()));
            if choice.destructive {
                let red: egui::Color32 = theme.accent_danger().into();
                btn = btn.fill(red.linear_multiply(0.18));
            }
            let _ = ui.add(btn); // 갤러리는 클릭 핸들 없음 — 시각만.
        }
    });
}

/// "Popup frame" 처럼 보이도록 surface0 배경 + border 카드를 두르는 헬퍼.
fn with_popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    width: f32,
    body_h: f32,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let total_h = TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + body_h;
    popup_frame::draw(ui, theme, title, width, total_h, ContentInset::INSET, paint);
}

/// 대표 상태 5 종:
/// 1. 정상 — Info severity, approve/deny 2 선택지
/// 2. Warn severity + 추가 선택지 (3 개)
/// 3. Danger severity — destructive 강조, 긴 body wrap
/// 4. body 없음 — title 만으로 충분한 단순 confirm
/// 5. 단축키 9 개 한계 — 다중 선택지 wrap
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "ApprovalProps + draw_approval_view — AppState/CoreState 비의존 view 함수.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    const POPUP_W: f32 = 460.0;
    let buf1 = RefCell::new(String::new());
    let buf2 = RefCell::new(String::new());
    let buf3 = RefCell::new(String::new());
    let buf4 = RefCell::new(String::new());
    let buf5 = RefCell::new(String::new());

    // 1. 정상 (Info)
    ui.label(
        egui::RichText::new("① Info severity — 표준 approve/deny:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockApprovalProps {
        id: "abc123".to_string(),
        severity: MockSeverity::Info,
        severity_label: "INFO",
        comment_label: "Comment (optional)",
        comment_hint: "Reason or context",
        body: Some("Plugin 'foo' wants to read clipboard."),
        choices: vec![
            MockChoiceView {
                label: "Approve".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Deny".to_string(),
                destructive: true,
            },
        ],
        comment_buffer: &buf1,
    };
    with_popup_frame(ui, theme, "Approval needed", POPUP_W, 160.0, |ui| {
        draw_mock_approval_view(ui, theme, &props);
    });

    ui.add_space(16.0);

    // 2. Warn + 3 선택지
    ui.label(
        egui::RichText::new("② Warn severity — 3 선택지 (Allow once / Allow always / Deny):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockApprovalProps {
        id: "ws-7-42".to_string(),
        severity: MockSeverity::Warn,
        severity_label: "WARN",
        comment_label: "Comment (optional)",
        comment_hint: "Reason or context",
        body: Some("Agent wants to execute: `rm -rf node_modules/`"),
        choices: vec![
            MockChoiceView {
                label: "Allow once".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Allow always".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Deny".to_string(),
                destructive: true,
            },
        ],
        comment_buffer: &buf2,
    };
    with_popup_frame(ui, theme, "Approval needed", POPUP_W, 170.0, |ui| {
        draw_mock_approval_view(ui, theme, &props);
    });

    ui.add_space(16.0);

    // 3. Danger + 긴 body
    ui.label(
        egui::RichText::new("③ Danger severity — destructive 강조, 긴 body wrap:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockApprovalProps {
        id: "danger-deploy-99".to_string(),
        severity: MockSeverity::Danger,
        severity_label: "DANGER",
        comment_label: "Comment (optional)",
        comment_hint: "Reason or context",
        body: Some(
            "Plugin 'deployer' requests permission to push to the production branch \
             (origin/main). This will trigger a release build and deploy to all customers. \
             Confirm only if you reviewed the diff against last release.",
        ),
        choices: vec![
            MockChoiceView {
                label: "Push to production".to_string(),
                destructive: true,
            },
            MockChoiceView {
                label: "Cancel".to_string(),
                destructive: false,
            },
        ],
        comment_buffer: &buf3,
    };
    with_popup_frame(ui, theme, "Production push", POPUP_W, 220.0, |ui| {
        draw_mock_approval_view(ui, theme, &props);
    });

    ui.add_space(16.0);

    // 4. body 없음
    ui.label(
        egui::RichText::new("④ Edge — body 없음 (단순 confirm):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockApprovalProps {
        id: "simple-ok".to_string(),
        severity: MockSeverity::Info,
        severity_label: "INFO",
        comment_label: "Comment (optional)",
        comment_hint: "Reason or context",
        body: None,
        choices: vec![
            MockChoiceView {
                label: "OK".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Cancel".to_string(),
                destructive: false,
            },
        ],
        comment_buffer: &buf4,
    };
    with_popup_frame(ui, theme, "Confirm", POPUP_W, 130.0, |ui| {
        draw_mock_approval_view(ui, theme, &props);
    });

    ui.add_space(16.0);

    // 5. 다중 선택지 wrap (단축키 1..=9 한계 시연)
    ui.label(
        egui::RichText::new("⑤ 다중 선택지 wrap — 5 개 (각각 1..=5 단축키):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockApprovalProps {
        id: "multi-choice-1".to_string(),
        severity: MockSeverity::Info,
        severity_label: "INFO",
        comment_label: "Comment (optional)",
        comment_hint: "Reason or context",
        body: Some("Pick the merge strategy for this PR."),
        choices: vec![
            MockChoiceView {
                label: "Squash".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Rebase".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Merge commit".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Fast-forward".to_string(),
                destructive: false,
            },
            MockChoiceView {
                label: "Abort".to_string(),
                destructive: true,
            },
        ],
        comment_buffer: &buf5,
    };
    with_popup_frame(ui, theme, "Merge strategy", POPUP_W, 180.0, |ui| {
        draw_mock_approval_view(ui, theme, &props);
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 와 시각 동기화. 1..=9 숫자 단축키 처리는 view 내부 — 갤러리는 시각만.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
