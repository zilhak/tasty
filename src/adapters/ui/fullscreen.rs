//! 전체화면 무대(fullscreen stage) — 창 전체를 독점하는 **독립 무대**.
//!
//! 무대는 기존 Workspace/Pane/Tab/Surface 트리와 **병렬로** 존재한다. 기존 요소를
//! 확대하는 것이 아니라, 창 전체 rect 를 자기 것으로 쓰는 별개의 표면이다. 그래서
//! 이 모듈은 레이아웃 계산(`surface_regions` / `compute_rects` / pane rect)을 하나도
//! 건드리지 않는다 — 근거는 `docs/adr/0082-fullscreen-independent-stage.md`.
//!
//! 구조는 popup 시스템과 대칭이다:
//!
//! | popup | 무대 |
//! |-------|------|
//! | [`PopupDef`](super::popup::PopupDef) + `defs::all_defs()` | [`StageDef`] + [`defs::all_defs`] |
//! | `PopupManager` (다중 + z-order) | [`StageState`] `Option` 하나 (무대는 창당 최대 1개) |
//! | `PopupManager::close` → `closed_queue` → `on_close` | [`AppState::close_fullscreen_stage`] → `stage_closed_queue` → [`drain_on_close_hooks`] |
//!
//! 정적 테이블인 이유: 선언하지 않은 것은 무대에 올라갈 수 없고(호출부가 draw 클로저를
//! 넘기는 방식이면 이 성질이 없다), debug IPC 가 가리킬 **id** 가 생긴다.
//!
//! 무대 콘텐츠의 자체 상태는 popup 관례를 그대로 따른다 — 무대 id 로 키를 만든 egui
//! temp memory 에 두고 [`StageDef::on_close`] 에서 지운다. [`StageState`] 자체에는
//! 무대 수명 그 자체에 속하는 것만 담는다.

pub mod defs;
pub(crate) mod notifications;

use crate::state::AppState;

/// 무대 식별자. `PopupId` 와 같이 정적 문자열이다 — debug IPC 가 이 id 로 무대를 지정한다.
pub use crate::fullscreen_stages::{StageId, StageMeta};

/// 무대 draw 함수의 프레임 결과. popup 의 `PopupAction` 과 대칭.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageAction {
    /// 아무 일도 없음 — 무대 유지.
    None,
    /// 이 프레임에 무대를 닫는다.
    Close,
}

/// 무대 하나의 정적 정의. 프로세스 수명 내내 살아있다.
pub struct StageDef {
    /// gui 무관 메타(id · 제목 키). **소유하지 않고 참조한다** — 정의가 메타를 복제하면
    /// 둘이 어긋날 수 있는데, 참조면 "정의는 있는데 메타가 없다" 가 타입으로 불가능해진다
    /// (반대 방향만 정합 테스트가 본다).
    pub meta: &'static StageMeta,
    /// 콘텐츠 렌더 함수. 무대가 활성인 매 프레임 호출된다. 셸(scrim + 제목)은
    /// [`draw_fullscreen_stage`] 가 이미 그린 뒤이고, 이 함수는 그 안쪽만 채운다.
    pub draw_fn: fn(&mut egui::Ui, &mut AppState, &mut crate::core::CoreState) -> StageAction,
    /// 닫힘 뒷정리 훅. 어떤 경로로 닫히든 정확히 1 회 발화한다
    /// ([`drain_on_close_hooks`], ADR-0063 과 같은 단일 수렴점 패턴).
    /// 그리는 게 없으므로 `&mut Ui` 가 아니라 `&egui::Context` 를 받는다 — 무대
    /// 콘텐츠 상태가 egui temp memory 에 있으면 여기서만 지울 수 있다.
    pub on_close: Option<fn(&egui::Context, &mut AppState, &mut crate::core::CoreState)>,
}

impl StageDef {
    /// 무대 식별자. 메타에서 온다.
    pub fn id(&self) -> StageId {
        self.meta.id
    }

    /// 제목 i18n 키. 메타에서 온다.
    pub fn title_key(&self) -> &'static str {
        self.meta.title_key
    }
}

/// 활성 무대의 런타임 상태. 창당 최대 하나이므로 `AppState` 에 `Option` 으로 산다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageState {
    /// 지금 올라와 있는 무대의 정의 id.
    pub id: StageId,
}

/// `on_close` drain 무한루프 방지 상한. popup 쪽과 같은 이유(훅이 다른 무대를 열고
/// 닫는 재진입)로 라운드를 센다.
const ON_CLOSE_DRAIN_MAX_ROUNDS: u32 = 8;

/// 닫힘 훅 drain — 무대가 닫힌 경로가 무엇이든 여기 한 곳으로 수렴한다.
///
/// 무대 프레임과 일반 프레임 **양쪽**에서 호출된다. 무대를 나오면 그 다음 프레임은
/// 일반 프레임이라 무대 경로가 돌지 않기 때문이다.
pub fn drain_on_close_hooks(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) {
    let mut round = 0u32;
    loop {
        let queue = std::mem::take(&mut state.stage_closed_queue);
        if queue.is_empty() {
            return;
        }
        round += 1;
        if round > ON_CLOSE_DRAIN_MAX_ROUNDS {
            tracing::warn!(
                "fullscreen stage on_close drain exceeded {ON_CLOSE_DRAIN_MAX_ROUNDS} rounds; \
                 dropping {} pending id(s)",
                queue.len()
            );
            return;
        }
        for id in queue {
            if let Some(hook) = defs::find(id).and_then(|def| def.on_close) {
                hook(ctx, state, engine);
            }
        }
    }
}

/// 무대 렌더 진입점. 활성 무대가 없으면 아무것도 그리지 않는다.
///
/// **레이어 선택 근거**(`docs/architecture/input-layer.md` §(c)/(d)): 미등록 raw
/// layer(= 그 tier 안에서 항상 최상단이 되는 함정)를 **의도적으로 쓰지 않고**
/// 등록된 `egui::Area`(`Order::Foreground`)로 그린다. 무대 프레임에는 경쟁할 다른
/// 레이어가 애초에 없다 — host chrome·popup·오버레이는 이 프레임에서 아예 그려지지
/// 않으므로(`Gpu::render` 의 무대 분기), "항상 최상단" 을 얻으려고 (c) 의 함정에
/// 기댈 이유가 없다. 등록해 두면 `Areas::order` 추적과 입력 라우팅이 정상 경로를
/// 타므로 이후 입력 라우팅 작업이 특수 케이스를 만들지 않아도 된다.
pub fn draw_fullscreen_stage(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) {
    drain_on_close_hooks(ctx, state, engine);
    let Some(id) = state.fullscreen_stage_id() else {
        return;
    };
    let Some(def) = defs::find(id) else {
        // 정의가 사라진 id 가 올라와 있으면(있을 수 없지만) 무대를 닫아 화면이
        // 영구히 빈 채로 남지 않게 한다.
        tracing::warn!("fullscreen stage '{id}' has no definition; closing");
        state.close_fullscreen_stage();
        return;
    };

    let th = crate::theme::theme();
    let screen = ctx.screen_rect();
    let mut action = StageAction::None;
    egui::Area::new(egui::Id::new("fullscreen_stage"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            // 셸: 창 전체 scrim + 제목. 뒤 콘텐츠는 이 프레임에 그려지지 않지만
            // scrim 은 무대가 "덮고 있다" 는 시각을 유지한다(마커 오버레이와 같은 토큰).
            ui.painter().rect_filled(screen, 0.0, th.scrim().to_egui());
            let title = crate::i18n::t(def.title_key());
            ui.painter().text(
                egui::pos2(screen.center().x, screen.top() + th.spacing_xl.value()),
                egui::Align2::CENTER_TOP,
                title,
                egui::FontId::proportional(th.font_size_heading.value()),
                th.text_primary().to_egui(),
            );
            if draw_exit_button(ui, screen) {
                action = StageAction::Close;
            }
            // 콘텐츠는 셸 chrome(제목·종료 버튼) **아래**로 묶인 child Ui 를 받는다 —
            // 콘텐츠가 chrome 위치를 알 필요도, 침범할 수도 없다.
            let mut content = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_rect(screen))
                    .id_salt(def.id()),
            );
            let content_action = (def.draw_fn)(&mut content, state, engine);
            if content_action == StageAction::Close {
                action = StageAction::Close;
            }
        });

    if action == StageAction::Close {
        state.close_fullscreen_stage();
        // 무대를 나온 화면(일반 프레임)을 그리려면 프레임이 한 번 더 필요하다 —
        // 진입 때와 같은 이유(`popup::frame::draw_popup_layer` 주석).
        ctx.request_repaint();
    }
}

/// 무대 콘텐츠에 주는 rect — 창 전체에서 셸 chrome(상단 제목 띠)과 바깥 여백을 뺀 것.
///
/// 종료 버튼도 이 띠 안(우측)에 있으므로 콘텐츠와 겹치지 않는다.
fn content_rect(screen: egui::Rect) -> egui::Rect {
    let th = crate::theme::theme();
    let pad = th.spacing_xl.value();
    let chrome_h = pad + th.font_size_heading.value() + th.spacing_lg.value();
    egui::Rect::from_min_max(
        egui::pos2(screen.left() + pad, screen.top() + chrome_h),
        egui::pos2(screen.right() - pad, screen.bottom() - pad),
    )
}

/// 무대 종료 버튼 — **셸이 공통 제공한다.** 콘텐츠 정의가 빠뜨릴 수 없어야 하기
/// 때문이다: 무대 프레임에는 CSD 타이틀바(창 닫기/최소화)조차 없으므로, 종료 수단이
/// 없는 무대는 창을 빠져나갈 방법이 없는 상태가 된다
/// (`docs/design/systems/fullscreen-stage.md` §아직 없는 것). `PopupManager` 가 X
/// 버튼을 중앙 관리하는 것과 같은 구조다.
///
/// 눌렸으면 `true`.
fn draw_exit_button(ui: &mut egui::Ui, screen: egui::Rect) -> bool {
    use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, top_right_inset_square};

    let th = crate::theme::theme();
    let pad = th.spacing_xl.value();
    let side = ControlSize::Md.height(&th);
    // 위치 공식은 공유한다 — 갤러리 specimen 이 같은 자리를 그리므로 여기 식을 손으로
    // 되풀이하면 갈라져도 화면 말고는 신호가 없다.
    let rect = top_right_inset_square(screen, pad, side);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .size(ControlSize::Md)
            .show(ui, &th, &|ui, rect, c| {
                crate::adapters::ui::icons::CLOSE
                    .image(rect.height(), c)
                    .paint_at(ui, rect);
            })
            .on_hover_text(crate::i18n::t("fullscreen.stage.exit_tooltip"))
            .clicked()
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_def_has_a_unique_id() {
        let mut ids: Vec<StageId> = defs::all_defs().iter().map(|d| d.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(
            before,
            ids.len(),
            "무대 id 는 정적 테이블 안에서 고유해야 한다"
        );
    }

    /// popup 이 선언한 무대 id 는 반드시 무대 테이블에 있어야 한다 — 두 정적 테이블이
    /// 서로 다른 파일이라 한쪽만 고치면 "버튼은 있는데 갈 곳이 없는" popup 이 된다.
    /// 아울러 headless popup(타이틀바가 없어 버튼을 그릴 자리가 없다)은 무대를 선언하지
    /// 않아야 한다.
    #[test]
    fn popup_declared_stages_exist_and_are_not_headless() {
        for def in crate::adapters::ui::popup::defs::all_defs() {
            let Some(stage) = def.fullscreen_stage else {
                continue;
            };
            assert!(
                defs::find(stage).is_some(),
                "popup '{}' 이 선언한 무대 '{stage}' 가 무대 테이블에 없다",
                def.id
            );
            assert!(
                !def.headless,
                "headless popup '{}' 은 타이틀바가 없어 전체화면 버튼을 그릴 자리가 없다",
                def.id
            );
        }
    }

    #[test]
    fn find_rejects_unknown_ids() {
        assert!(defs::find("no-such-stage").is_none());
        for def in defs::all_defs() {
            assert!(defs::find(def.id()).is_some());
        }
    }
}
