//! 배너(Banner) 시스템 — Modal / Popup / Toast 에 이은 **4번째 오버레이 개념**.
//!
//! 설계 문서: `docs/design/systems/banner.md`, 결정 근거:
//! `docs/adr/0024-banner-fourth-overlay-concept.md`. 분류 enum 은
//! [`tasty_model::banner_kind`] (model 잔류, GUI 비의존).
//!
//! 배너는 스코프(View / Workspace / Pane / Tab / Surface) 콘텐츠 영역 최상단(탭바
//! 바로 아래)에 떠 있는 focus-less 공지다. Toast 와 달리 **자기 마우스를 소비**하고
//! (뒤로 전파 X) 내부 action 버튼을 가질 수 있다. Popup 과 달리 **키보드 포커스를
//! 받지 않으며**(클릭해도 포커스 이동 X) 타이틀바·드래그·자유이동이 없다.
//!
//! ## Split: manager(상태) / view(시각)
//!
//! [`BannerManager`] 가 *상태 관리* — 스코프당 1 표시 + 최대 5 큐, 동일 id
//! 리셋/무시, TTL 카운트다운·정지/재개, 계층 z-order·디밍 — 를 담당한다. 큐/TTL
//! 로직은 egui 비의존 순수 함수라 [`mod tests`] 에서 결정론적으로 검증된다. 시각
//! draw([`BannerManager::draw`])는 미리 계산된 상태 + scope rect 만 받아 그린다.
//!
//! ## 발화 정책 (불가침)
//!
//! 배너는 **사용자 직접 조작에서만** 발사된다. IPC/release cascade 는 배너를 띄울 수
//! 없다 (Toast/Popup 과 동일, identity 원칙 1). debug 빌드에서만 직접 띄워 시각
//! 검증한다 ([`crate::adapters::ipc::handler::debug`] 의 `banner.*`).

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use crate::adapters::ui::icons;
use crate::theme::Theme;

use super::layout_context::LayoutContext;

pub use tasty_model::banner_kind::{BannerId, BannerScope};

/// 배너 내부 콘텐츠를 그리는 함수. 셸(프레임/패딩/그림자)은 매니저가 그리고,
/// 이 함수는 셸 내부 child `Ui` 에 icon/제목/본문/action 을 그린다. id 가 곧 kind 라
/// 심각도 표현(글리프 색 등)은 각 함수가 자체 처리한다.
pub type BannerContentFn = fn(&mut egui::Ui, &Theme);

/// 스코프당 큐 최대 대기 개수 (표시 중 1 + 대기 5 = 총 6).
const MAX_QUEUED: usize = 5;

/// 단일 배너 정적 정의 — id(=kind), TTL 유무, 콘텐츠 draw 함수.
///
/// popup 의 `PopupDef` 와 동일한 데이터 지향 정의. `defs::find` 로 id 조회한다.
#[derive(Clone, Copy)]
pub struct BannerDef {
    /// 고유 id = kind. 같은 id 는 한 스코프에 하나만 존재한다.
    pub id: BannerId,
    /// TTL(초). `None` 이면 사용자 닫기까지 유지, `Some` 이면 카운트다운 후 자동 소멸.
    // 이유: 선언용 필드 — 현재 모든 `BannerDef`(BANNER_MOUSE_CAPTURE,
    // BANNER_SHELL_INTEGRATION_MISSING)는 `BannerState::persistent`로 직접 열려
    // 이 필드를 거치지 않는다. def→state 변환 지점이 생기면 여기서 읽어야 한다
    // (판단필요 — conductor 검토).
    #[allow(dead_code)]
    pub ttl_seconds: Option<u32>,
    /// 셸 내부 콘텐츠 draw 함수.
    pub content_fn: BannerContentFn,
}

/// 배너 콘텐츠의 출처(D2). 큐/TTL/z-order/위치 등 **생명주기는 host 소유 단일 지점**
/// ([`BannerManager`])이 관리하고, **content 만** 이 enum 으로 분기한다.
///
/// - [`BannerContentSource::Host`]: host 정의 배너. `defs::find(id)` 로 `content_fn` 을
///   조회해 host 프로세스의 egui 로 셸 내부를 그린다(기존 배너).
/// - [`BannerContentSource::PluginMesh`]: plugin egui-mesh 배너(A3). plugin 이 자기
///   프로세스에서 content_rect 를 tessellate 하고 host 는 셸(컨테이너/border/close X/
///   카운트다운)만 그린 뒤 content_rect 에 plugin mesh 를 합성한다(popup chrome 과 동형).
///
/// (B5 substrate: 이후 다른 egui-mesh banner 소비자도 `PluginMesh` 를 재사용한다.)
#[derive(Clone, Debug, PartialEq)]
pub enum BannerContentSource {
    /// host 프로세스가 `content_fn` 으로 셸 내부를 그린다.
    Host,
    /// plugin 이 egui-mesh 로 content_rect 를 그린다.
    PluginMesh {
        /// 소유 plugin id — set_context forward / mesh frame lookup 에 필요.
        plugin_id: String,
        /// host 발급 banner 인스턴스 식별자.
        instance_id: u64,
        /// 셸 높이(LogicalPx). 너비는 스코프 폭 도킹이라 높이만 host 가 고정한다
        /// (manifest `size_hint.height`, 없으면 기본값).
        height: f32,
    },
}

/// 배너 큐/dedup/카드 rect 의 identity 키. host 배너는 정적 id, plugin 배너는
/// 인스턴스 id 로 구분한다 — `BannerId`(&'static str)로는 동적 plugin 배너를 키잉할 수
/// 없으므로(popup 이 별도 instance map 을 둔 것과 같은 이유), content 원천별로 나눈다.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum BannerKey {
    Host(BannerId),
    Plugin(u64),
}

/// 단일 배너 인스턴스 상태. 큐/TTL 의 단위.
#[derive(Clone, Debug, PartialEq)]
pub struct BannerState {
    /// 고유 id = kind. plugin 배너는 미사용(빈 문자열) — identity 는 `content` 로.
    pub id: BannerId,
    /// 대상 스코프.
    pub scope: BannerScope,
    /// 총 TTL(ms). `None` = TTL 없음(사용자 닫기까지 유지).
    pub ttl_ms: Option<f32>,
    /// 남은 시간(ms). `ttl_ms` 가 `None` 이면 의미 없음.
    pub remaining_ms: f32,
    /// 콘텐츠 출처(host fn / plugin mesh). 생명주기는 공유, content 만 분기(D2).
    pub content: BannerContentSource,
}

impl BannerState {
    /// TTL 없는 배너 — 사용자가 닫을 때까지 유지.
    pub fn persistent(id: BannerId, scope: BannerScope) -> Self {
        Self {
            id,
            scope,
            ttl_ms: None,
            remaining_ms: 0.0,
            content: BannerContentSource::Host,
        }
    }

    /// TTL(초) 배너 — 카운트다운 0 에서 자동 소멸.
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)와 `#[cfg(test)]` 뿐이라
    // release+non-test 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn with_ttl(id: BannerId, scope: BannerScope, seconds: u32) -> Self {
        let ms = seconds as f32 * 1000.0;
        Self {
            id,
            scope,
            ttl_ms: Some(ms),
            remaining_ms: ms,
            content: BannerContentSource::Host,
        }
    }

    /// plugin egui-mesh 배너(A3). `instance_id` 는 host 발급, `ttl_seconds` 가 `None` 이면
    /// persistent(close X 로만 닫힘). `height` 는 셸 높이(LogicalPx).
    pub fn plugin_mesh(
        scope: BannerScope,
        plugin_id: String,
        instance_id: u64,
        ttl_seconds: Option<u32>,
        height: f32,
    ) -> Self {
        let ttl_ms = ttl_seconds.map(|s| s as f32 * 1000.0);
        Self {
            id: "",
            scope,
            ttl_ms,
            remaining_ms: ttl_ms.unwrap_or(0.0),
            content: BannerContentSource::PluginMesh {
                plugin_id,
                instance_id,
                height,
            },
        }
    }

    /// 큐/dedup/카드 rect identity 키.
    pub fn key(&self) -> BannerKey {
        match &self.content {
            BannerContentSource::Host => BannerKey::Host(self.id),
            BannerContentSource::PluginMesh { instance_id, .. } => BannerKey::Plugin(*instance_id),
        }
    }

    /// 우상단에 표시할 남은 초 (올림). TTL 없으면 `None`.
    pub fn remaining_seconds(&self) -> Option<u32> {
        self.ttl_ms
            .map(|_| (self.remaining_ms / 1000.0).ceil().max(0.0) as u32)
    }

    /// 카운트다운을 총 TTL 로 리셋 (동일 id 재발화 시).
    fn reset_countdown(&mut self) {
        if let Some(total) = self.ttl_ms {
            self.remaining_ms = total;
        }
    }
}

/// `push` 결과 — debug/테스트 가시성용. 어떤 일이 일어났는지 알려준다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerPushOutcome {
    /// 빈 스코프라 즉시 표시됨.
    Shown,
    /// 표시 중인 배너가 있어 큐에 대기됨.
    Queued,
    /// 동일 id 가 표시 중 → 카운트다운만 리셋됨 (새로 안 생김).
    ResetCountdown,
    /// 무시됨 (큐의 동일 id 중복 / 큐 가득 참 / TTL 없는 표시 중 동일 id).
    Ignored,
}

/// plugin egui-mesh 배너가 host 측 생명주기에서 닫힌 사유(D3). host-plugin manager 의
/// `BannerCloseReason` 로 매핑되어 plugin 에 `banner.closed` 로 통지된다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginBannerCloseKind {
    /// TTL 카운트다운 만료.
    Ttl,
    /// 사용자가 셸 우상단 close X 를 눌렀음.
    UserClose,
}

/// plugin 배너 mesh 합성 슬롯 — draw 가 그린 셸의 content_rect(논리 좌표)를 실어
/// host banner 합성기(`draw_plugin_banners`)에 전달한다. plugin mesh 가 이 rect 에 얹힌다.
#[derive(Clone)]
pub struct PluginBannerMeshSlot {
    pub plugin_id: String,
    pub instance_id: u64,
    pub content_rect: egui::Rect,
}

/// 한 스코프의 표시 슬롯 + 대기 큐.
#[derive(Default, Clone)]
struct ScopeLane {
    shown: Option<BannerState>,
    queue: VecDeque<BannerState>,
}

impl ScopeLane {
    fn is_empty(&self) -> bool {
        self.shown.is_none() && self.queue.is_empty()
    }
}

/// 배너 큐·TTL·계층을 중앙 관리하는 매니저. Toast/Popup 과 별도.
#[derive(Default)]
pub struct BannerManager {
    scopes: HashMap<BannerScope, ScopeLane>,
    /// 마지막 `draw` 시각 — TTL 의 실시간 dt 계산용. 테스트는 `advance` 직접 호출.
    last_tick: Option<Instant>,
    /// 직전 프레임에 **실제로 그려진** 배너 카드 rect — hover/소비 판정의 기준.
    /// 배치용 placeholder(`banner_zone`, scope 전체 rect)와 입력 zone 을 분리한다:
    /// scope 전체를 소비하면 이미 focus 된 캡쳐 surface 본문 클릭까지 삼켜 마우스
    /// 리포트가 막히므로, hover 는 카드(콘텐츠 높이) rect 로만 판정한다. egui
    /// immediate-mode 라 카드 rect 는 그린 *후* 에야 알 수 있어 1프레임 지연 측정한다
    /// (persistent 배너는 정적이라 비가시).
    card_rects: HashMap<(BannerScope, BannerKey), egui::Rect>,
    /// 이번 draw 에서 그려진 plugin egui-mesh 배너 슬롯(A3). 매 draw 재수집한다.
    /// host banner 합성기(`draw_plugin_banners`)가 draw 직후 take 해 set_context
    /// forward + mesh 합성 영역 적재에 쓴다.
    plugin_mesh_slots: Vec<PluginBannerMeshSlot>,
    /// 이번 프레임에 host 측 생명주기(TTL/close X)로 닫힌 plugin 배너 (instance_id, 사유).
    /// 합성기가 drain 해 host-plugin manager 로 close 전파(→ `banner.closed`).
    closed_plugin_banners: Vec<(u64, PluginBannerCloseKind)>,
}

impl BannerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 배너 발화. **사용자 행동에서만** 호출되어야 한다 (발화 정책 §불가침).
    ///
    /// - 표시 중 동일 id → TTL 있으면 카운트다운 리셋, 없으면 무시.
    /// - 큐의 동일 id → 무시.
    /// - 빈 스코프 → 즉시 표시. 표시 중이면 큐(<5)에 대기, 가득 차면 무시.
    pub fn push(&mut self, banner: BannerState) -> BannerPushOutcome {
        let lane = self.scopes.entry(banner.scope.clone()).or_default();

        if let Some(shown) = lane.shown.as_mut()
            && shown.key() == banner.key()
        {
            return if shown.ttl_ms.is_some() {
                shown.reset_countdown();
                BannerPushOutcome::ResetCountdown
            } else {
                BannerPushOutcome::Ignored
            };
        }

        if lane.queue.iter().any(|b| b.key() == banner.key()) {
            return BannerPushOutcome::Ignored;
        }

        if lane.shown.is_none() {
            lane.shown = Some(banner);
            BannerPushOutcome::Shown
        } else if lane.queue.len() < MAX_QUEUED {
            lane.queue.push_back(banner);
            BannerPushOutcome::Queued
        } else {
            BannerPushOutcome::Ignored
        }
    }

    /// 스코프의 표시 중 배너를 닫고 큐 head 를 승격한다 (X 버튼 / TTL 만료).
    /// 닫힌 배너 상태를 돌려준다 (없으면 `None`) — 호출자가 plugin 배너면 close 사유를
    /// 부여할 수 있게.
    pub fn close_shown(&mut self, scope: &BannerScope) -> Option<BannerState> {
        let lane = self.scopes.get_mut(scope)?;
        let removed = lane.shown.take()?;
        lane.shown = lane.queue.pop_front();
        if lane.is_empty() {
            self.scopes.remove(scope);
        }
        Some(removed)
    }

    /// plugin egui-mesh 배너를 instance_id 로 닫는다 (plugin 요청 `banner.close` /
    /// plugin 종료 시 host 측 UI 슬롯 정리). 표시 중이면 닫고 승격, 큐에 있으면 제거.
    /// 닫힌(또는 제거된) 게 있으면 `true`. host-plugin manager 로의 통지는 호출자 책임.
    pub fn close_by_instance(&mut self, instance_id: u64) -> bool {
        let target = BannerKey::Plugin(instance_id);
        let mut target_scope: Option<BannerScope> = None;
        for (scope, lane) in self.scopes.iter_mut() {
            if lane.shown.as_ref().is_some_and(|b| b.key() == target) {
                target_scope = Some(scope.clone());
                break;
            }
            if let Some(pos) = lane.queue.iter().position(|b| b.key() == target) {
                lane.queue.remove(pos);
                return true;
            }
        }
        if let Some(scope) = target_scope {
            return self.close_shown(&scope).is_some();
        }
        false
    }

    /// 현재 host 측 UI 에 살아있는 plugin 배너 instance_id 목록 (표시 + 큐).
    /// 합성기가 host-plugin manager 와 reconcile 하는 데 쓴다.
    pub fn plugin_instances(&self) -> impl Iterator<Item = u64> + '_ {
        self.scopes.values().flat_map(|lane| {
            lane.shown
                .iter()
                .chain(lane.queue.iter())
                .filter_map(|b| match &b.content {
                    BannerContentSource::PluginMesh { instance_id, .. } => Some(*instance_id),
                    BannerContentSource::Host => None,
                })
        })
    }

    /// 이번 draw 에서 그려진 plugin 배너 mesh 슬롯을 가져간다(소유권 이전). 합성기가
    /// draw 직후 1회 호출한다.
    pub fn take_plugin_mesh_slots(&mut self) -> Vec<PluginBannerMeshSlot> {
        std::mem::take(&mut self.plugin_mesh_slots)
    }

    /// 이번 프레임에 host 측 생명주기로 닫힌 plugin 배너 목록을 drain 한다. 합성기가
    /// host-plugin manager 에 close 를 전파(→ `banner.closed`)하는 데 쓴다.
    pub fn drain_closed_plugin_banners(&mut self) -> Vec<(u64, PluginBannerCloseKind)> {
        std::mem::take(&mut self.closed_plugin_banners)
    }

    /// id 로 닫는다 (debug). 표시 중이면 닫고 승격, 큐에 있으면 큐에서 제거.
    /// 닫힌(또는 제거된) 게 있으면 `true`.
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)와 `#[cfg(test)]` 뿐이라
    // release+non-test 빌드에서 미사용으로 잡힌다 (debug-ipc.md 격리 정책의 정상 형태).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn close_by_id(&mut self, id: BannerId) -> bool {
        let mut target_scope: Option<BannerScope> = None;
        for (scope, lane) in self.scopes.iter_mut() {
            if lane.shown.as_ref().is_some_and(|b| b.id == id) {
                target_scope = Some(scope.clone());
                break;
            }
            if let Some(pos) = lane.queue.iter().position(|b| b.id == id) {
                lane.queue.remove(pos);
                return true;
            }
        }
        if let Some(scope) = target_scope {
            return self.close_shown(&scope).is_some();
        }
        false
    }

    /// TTL 카운트다운 진행. `is_paused(scope, key)` 가 `true` 인 배너는 정지(hover 중
    /// 또는 백그라운드). 0 이하로 떨어지면 자동 소멸 + 큐 승격. plugin 배너가 만료되면
    /// `closed_plugin_banners` 에 `Ttl` 로 기록된다(합성기가 `banner.closed` 로 전파).
    ///
    /// 순수 함수(Instant 비의존) — 테스트가 dt 를 직접 넣어 검증한다.
    pub fn advance(&mut self, dt_ms: f32, is_paused: impl Fn(&BannerScope, &BannerKey) -> bool) {
        let mut expired: Vec<BannerScope> = Vec::new();
        for (scope, lane) in self.scopes.iter_mut() {
            if let Some(shown) = lane.shown.as_mut()
                && shown.ttl_ms.is_some()
                && !is_paused(scope, &shown.key())
            {
                shown.remaining_ms -= dt_ms;
                if shown.remaining_ms <= 0.0 {
                    expired.push(scope.clone());
                }
            }
        }
        for scope in expired {
            if let Some(removed) = self.close_shown(&scope)
                && let BannerContentSource::PluginMesh { instance_id, .. } = removed.content
            {
                self.closed_plugin_banners
                    .push((instance_id, PluginBannerCloseKind::Ttl));
            }
        }
    }

    /// 표시 중 배너의 카운트다운을 강제 설정한다 (debug). TTL 배너에만 적용.
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)와 `#[cfg(test)]` 뿐이라
    // release+non-test 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn set_countdown(&mut self, scope: &BannerScope, seconds: u32) -> bool {
        if let Some(lane) = self.scopes.get_mut(scope)
            && let Some(shown) = lane.shown.as_mut()
            && shown.ttl_ms.is_some()
        {
            let ms = seconds as f32 * 1000.0;
            shown.ttl_ms = Some(ms);
            shown.remaining_ms = ms;
            return true;
        }
        false
    }

    /// 현재 표시 중인 배너들 (스코프당 0~1). draw·debug 가 순회.
    pub fn shown_banners(&self) -> impl Iterator<Item = &BannerState> {
        self.scopes.values().filter_map(|lane| lane.shown.as_ref())
    }

    /// 스코프의 대기 큐 (표시 중 제외).
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)뿐이라 release 빌드에서
    // 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn queued_banners(&self, scope: &BannerScope) -> impl Iterator<Item = &BannerState> {
        self.scopes
            .get(scope)
            .into_iter()
            .flat_map(|lane| lane.queue.iter())
    }

    /// 전체 스코프 대기 큐 합 (debug 요약).
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)와 `#[cfg(test)]` 뿐이라
    // release+non-test 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn total_queued(&self) -> usize {
        self.scopes.values().map(|lane| lane.queue.len()).sum()
    }

    /// 어떤 배너든 떠 있거나 대기 중인지.
    pub fn has_any(&self) -> bool {
        !self.scopes.is_empty()
    }

    /// 표시 중 배너가 상위 스코프 배너 뒤로 디밍(recessed)되어야 하는지 판정.
    /// 자기 priority 가 현재 가시 배너 중 최고 priority 보다 낮으면 recessed.
    fn is_recessed(&self, scope: &BannerScope, visible_max_priority: u8) -> bool {
        scope.priority() < visible_max_priority
    }
}

// ── 시각 draw ────────────────────────────────────────────────────────────────

/// `draw` 결과 — 입력 레이어 배선용. `hovered` 가 true 면 마우스가 배너 위라
/// 하위 레이어(터미널/divider)로 전파를 막아야 한다(`AppState.banner_hovered`).
#[derive(Default, Clone, Copy)]
pub struct BannerDrawResult {
    pub hovered: bool,
}

/// 배너 셸 한 장을 그린다 — surface-raised fill + 1px border-strong + radius-8 +
/// popover shadow. `opacity` < 1 이면 색을 곱해 디밍(recessed). 내부 콘텐츠는
/// `content` 가 child `Ui` 에 그린다. 반환값은 **실제로 그려진 카드 rect**(콘텐츠
/// 높이만큼) — 입력 hover/소비 zone 판정에 쓴다(배치 placeholder 와 분리).
fn draw_shell(
    ui: &mut egui::Ui,
    theme: &Theme,
    opacity: f32,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    let dim = |c: egui::Color32| c.gamma_multiply(opacity);
    let mut shadow = theme.shadow_popover().to_egui();
    shadow.color = shadow.color.gamma_multiply(opacity);
    egui::Frame::new()
        .fill(dim(theme.banner_bg().to_egui()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            dim(theme.banner_border().to_egui()),
        ))
        .corner_radius(theme.corner_radius_lg.value())
        .shadow(shadow)
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        })
        .response
        .rect
}

impl BannerManager {
    /// 매 프레임 호출. TTL 진행 + 표시 중 배너 draw + hover/close 처리.
    ///
    /// - `view_placeholder`: 현재 View 가 지정한 View-스코프 배너 위치(없으면 View
    ///   배너 미표시). Modal 포함 모든 View 가 자기 플레이스홀더를 제공.
    /// - 계층 z-order: 가시 표시 배너를 priority 오름차순으로 그려 상위가 앞에 오게
    ///   하고, 하위(자기보다 높은 가시 배너 존재)는 recessed opacity 로 디밍한다.
    ///
    /// 반환 `hovered` 는 입력 레이어(마우스 비전파)에 쓰인다.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        draw_ctx: &LayoutContext,
        theme: &Theme,
        view_placeholder: Option<egui::Rect>,
        _reduced_motion: bool,
    ) -> BannerDrawResult {
        // 1) TTL dt 계산 (Instant 기반). 정지 조건은 draw 단계에서 hover/가시성으로
        //    판정하므로, 여기서는 dt 만 구하고 advance 는 hover 결정 후 호출한다.
        let now = Instant::now();
        let dt_ms = self
            .last_tick
            .map(|prev| now.duration_since(prev).as_secs_f32() * 1000.0)
            .unwrap_or(0.0)
            .min(1000.0); // 프레임 폭주/디버그 정지 후 점프 방지(최대 1s)
        self.last_tick = Some(now);

        // plugin mesh 슬롯은 매 draw 재수집한다 (이전 프레임 잔재 방지).
        self.plugin_mesh_slots.clear();

        // 2) 표시 중 배너의 scope rect 해석 (가시 = rect 있음).
        struct Slot {
            scope: BannerScope,
            key: BannerKey,
            /// host 배너 content_fn 조회용 id (plugin 배너면 `""`).
            id: BannerId,
            /// plugin egui-mesh 배너면 (plugin_id, instance_id, 셸 높이). host 면 None.
            mesh: Option<(String, u64, f32)>,
            zone: egui::Rect,
            remaining_seconds: Option<u32>,
        }
        let mut slots: Vec<Slot> = Vec::new();
        for banner in self.shown_banners() {
            let Some(zone) =
                Self::banner_zone(&banner.scope, draw_ctx, ctx, view_placeholder, theme)
            else {
                continue; // 백그라운드(스코프 비가시) — draw 안 함, TTL 정지(아래).
            };
            let mesh = match &banner.content {
                BannerContentSource::PluginMesh {
                    plugin_id,
                    instance_id,
                    height,
                } => Some((plugin_id.clone(), *instance_id, *height)),
                BannerContentSource::Host => None,
            };
            slots.push(Slot {
                scope: banner.scope.clone(),
                key: banner.key(),
                id: banner.id,
                mesh,
                zone,
                remaining_seconds: banner.remaining_seconds(),
            });
        }

        // 3) 계층 — 가시 배너 중 최고 priority.
        let visible_max_priority = slots.iter().map(|s| s.scope.priority()).max().unwrap_or(0);

        // priority 오름차순 정렬 → 하위 먼저(뒤), 상위 나중(앞).
        slots.sort_by_key(|s| s.scope.priority());

        // 4) draw. hover 판정 → 입력 레이어 + TTL 정지.
        let pointer = ctx.pointer_hover_pos();
        let mut hovered_any = false;
        let mut hovered_ids: Vec<(BannerScope, BannerKey)> = Vec::new();
        let mut close_requests: Vec<BannerScope> = Vec::new();
        // plugin 배너가 이번 프레임 그린 (plugin_id, instance_id, content_rect).
        let mut mesh_drawn: Vec<PluginBannerMeshSlot> = Vec::new();
        // 이번 프레임 실측 카드 rect — 루프 종료 후 `self.card_rects` 를 이 값으로
        // 교체한다(사라진 배너 키는 자동 정리). hover 는 *직전* 프레임 카드 rect 로 판정.
        let mut next_card_rects: HashMap<(BannerScope, BannerKey), egui::Rect> = HashMap::new();

        let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("banner_layer"));

        for slot in &slots {
            let recessed = self.is_recessed(&slot.scope, visible_max_priority);
            let opacity = if recessed {
                theme.opacity_recessed()
            } else {
                1.0
            };
            // hover/소비 판정은 scope 전체(`slot.zone`, 배치 placeholder)가 아니라 직전
            // 프레임에 실제 그려진 카드 rect 로 한정한다 — scope 전역을 소비하면 이미
            // focus 된 캡쳐 surface 본문 클릭까지 삼켜 마우스 리포트가 막힌다. 카드 rect
            // 가 아직 없는 첫 프레임엔 hover=false(자연스러움).
            let card_key = (slot.scope.clone(), slot.key.clone());
            let banner_hovered = pointer.is_some_and(|p| {
                self.card_rects
                    .get(&card_key)
                    .is_some_and(|r| r.contains(p))
            });
            if banner_hovered {
                hovered_any = true;
                hovered_ids.push(card_key.clone());
            }

            // plugin mesh 배너면 content_rect 를 여기로 실어낸다(셸 내부, 우측 affordance 제외).
            let mut mesh_content_rect: Option<egui::Rect> = None;
            let mut child = ui_at(ctx, layer_id, slot.zone);
            let card_rect = draw_shell(&mut child, theme, opacity, |ui| {
                if let Some((_, _, height)) = &slot.mesh {
                    // plugin egui-mesh 배너: 셸 내부를 (가용폭 - affordance) × height 로
                    // 예약하고 그 rect 를 content_rect 로 실어낸다(콘텐츠 자체는 GPU 가 host
                    // egui pass 후 합성). affordance 열은 비워 두고 아래에서 host 가 그린다.
                    let aff = theme.item_height_interactive.value();
                    let w = (ui.available_width() - aff).max(1.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(w, *height), egui::Sense::hover());
                    mesh_content_rect = Some(rect);
                } else if let Some(def) = defs::find(slot.id) {
                    // host 배너 콘텐츠 (id → def 조회). 없으면 id 텍스트만(누락 정의 가시화).
                    (def.content_fn)(ui, theme);
                } else {
                    ui.label(slot.id);
                }
                // 우상단 affordance — TTL 카운트다운 ↔ hover 시 X. 같은 자리.
                let avail = ui.max_rect();
                let corner = egui::Rect::from_min_max(
                    egui::pos2(
                        avail.right() - theme.item_height_interactive.value(),
                        avail.top(),
                    ),
                    egui::pos2(
                        avail.right(),
                        avail.top() + theme.item_height_interactive.value(),
                    ),
                );
                let mut corner_ui = ui.new_child(egui::UiBuilder::new().max_rect(corner));
                corner_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if banner_hovered {
                        // 닫기 affordance — 갤러리 specimen `dismiss_x()` 와 동일한
                        // Ghost/Sm IconButton + icons::CLOSE(SVG). raw `"✕"`(U+2715)는
                        // UI 폰트에 글리프가 없어 tofu(□)로 렌더되던 것을 고친다(gallery
                        // parity). 색은 IconButton 의 해소색(ghost: text-secondary →
                        // hover text-primary)을 따른다.
                        if IconButton::new()
                            .variant(IconButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, theme, &|ui, rect, c| {
                                icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                            })
                            .clicked()
                        {
                            close_requests.push(slot.scope.clone());
                        }
                    } else if let Some(secs) = slot.remaining_seconds {
                        ui.label(
                            egui::RichText::new(secs.to_string())
                                .monospace()
                                .size(theme.font_size_micro.value())
                                .color(theme.banner_countdown_fg().to_egui()),
                        );
                    }
                });
            });
            // plugin mesh 배너면 실측 content_rect 를 합성 슬롯으로 적재.
            if let (Some((plugin_id, instance_id, _)), Some(content_rect)) =
                (&slot.mesh, mesh_content_rect)
            {
                mesh_drawn.push(PluginBannerMeshSlot {
                    plugin_id: plugin_id.clone(),
                    instance_id: *instance_id,
                    content_rect,
                });
            }
            next_card_rects.insert(card_key, card_rect);
        }

        // 직전 프레임 카드 rect 교체 — 더는 표시되지 않는 배너 키는 자동으로 빠진다.
        self.card_rects = next_card_rects;
        // 이번 프레임 그려진 plugin mesh 슬롯 확정 — 합성기가 draw 직후 take.
        self.plugin_mesh_slots = mesh_drawn;

        // 5) TTL 진행 — hover 중이거나 백그라운드(가시 슬롯 없음)면 정지.
        let visible_ids: std::collections::HashSet<(BannerScope, BannerKey)> = slots
            .iter()
            .map(|s| (s.scope.clone(), s.key.clone()))
            .collect();
        let hovered_set: std::collections::HashSet<(BannerScope, BannerKey)> =
            hovered_ids.into_iter().collect();
        // TTL 진행 — hover 중이거나 백그라운드(가시 슬롯 없음)면 정지. reduced_motion 은
        // 페이드 모션에만 영향이고 카운트다운 진행 자체는 항상 동작한다.
        self.advance(dt_ms, |scope, key| {
            let k = (scope.clone(), key.clone());
            hovered_set.contains(&k) || !visible_ids.contains(&k)
        });

        // 6) 사용자 X 클릭 닫기. plugin 배너면 UserClose 로 기록(→ `banner.closed`).
        for scope in close_requests {
            if let Some(removed) = self.close_shown(&scope)
                && let BannerContentSource::PluginMesh { instance_id, .. } = removed.content
            {
                self.closed_plugin_banners
                    .push((instance_id, PluginBannerCloseKind::UserClose));
            }
        }

        // 살아있는 동안 매 프레임 repaint (TTL 카운트다운/페이드 갱신).
        if self.has_any() {
            ctx.request_repaint();
        }

        BannerDrawResult {
            hovered: hovered_any,
        }
    }

    /// 스코프 배너 존(zone) rect — 셸이 그려질 영역. 탭바 바로 아래 8px,
    /// 양옆 8px margin, 하단 margin 없음. 스코프 비가시면 `None`.
    fn banner_zone(
        scope: &BannerScope,
        draw_ctx: &LayoutContext,
        ctx: &egui::Context,
        view_placeholder: Option<egui::Rect>,
        theme: &Theme,
    ) -> Option<egui::Rect> {
        let margin = theme.spacing_sm.value();
        let base = match scope {
            // View/Modal: 각 View 가 지정한 플레이스홀더. 워크스페이스 비종속.
            BannerScope::View => view_placeholder?,
            // Workspace: 활성 워크스페이스의 화면 전체 폭, 탭바 아래.
            BannerScope::Workspace(ws_idx) => {
                if *ws_idx != draw_ctx.active_workspace {
                    return None;
                }
                let screen = ctx.screen_rect();
                egui::Rect::from_min_max(
                    egui::pos2(screen.left(), screen.top() + theme.tab_bar_height.value()),
                    screen.max,
                )
            }
            // Pane: pane 영역 100%, 탭바 아래.
            BannerScope::Pane(pane_id) => {
                let rect = draw_ctx
                    .pane_rects
                    .iter()
                    .find(|(id, _)| id == pane_id)
                    .map(|(_, r)| *r)?;
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.top() + theme.tab_bar_height.value()),
                    rect.max,
                )
            }
            // Tab: 해당 pane 이 그 탭을 활성으로 둘 때만. pane 영역 100%.
            BannerScope::Tab(pane_id, tab_idx) => {
                let active = draw_ctx
                    .active_tabs
                    .iter()
                    .any(|(pid, idx)| pid == pane_id && idx == tab_idx);
                if !active {
                    return None;
                }
                let rect = draw_ctx
                    .pane_rects
                    .iter()
                    .find(|(id, _)| id == pane_id)
                    .map(|(_, r)| *r)?;
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.top() + theme.tab_bar_height.value()),
                    rect.max,
                )
            }
            // Surface: surface 영역 100%. surface rect 최상단이 곧 탭바 아래.
            BannerScope::Surface(surface_id) => draw_ctx
                .surface_rects
                .iter()
                .find(|(id, _)| id == surface_id)
                .map(|(_, r)| *r)?,
        };
        // margin 적용: 상·좌·우 8px, 하단 없음. 높이는 가변이므로 max.y 는 base 하단.
        let zone = egui::Rect::from_min_max(
            egui::pos2(base.left() + margin, base.top() + margin),
            egui::pos2(base.right() - margin, base.bottom()),
        );
        if zone.width() <= 0.0 {
            return None;
        }
        Some(zone)
    }
}

/// 주어진 rect 에 layer painter child Ui 를 만든다 (배너는 다른 UI 위 Foreground).
fn ui_at(ctx: &egui::Context, layer_id: egui::LayerId, rect: egui::Rect) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        egui::Id::new(("banner_zone", rect.left() as i32, rect.top() as i32)),
        egui::UiBuilder::new().layer_id(layer_id).max_rect(rect),
    )
}

/// 배너 정적 정의 레지스트리. id → `BannerDef`. 첫 용도(마우스 캡쳐 안내)와
/// debug 발화가 여기서 정의를 찾는다.
pub mod defs {
    use super::{BannerDef, BannerId};
    use crate::adapters::ui::icons;
    use crate::i18n::t;
    use crate::theme::Theme;

    /// 예시 배너: TUI 마우스 캡쳐 안내 (첫 용도). 사용자가 마우스 캡쳐 surface 에서
    /// 드래그 선택을 시도했을 때 표시 — 안내 + action.
    pub const BANNER_MOUSE_CAPTURE: BannerId = "mouse-capture";

    fn content_mouse_capture(ui: &mut egui::Ui, theme: &Theme) {
        // 가로 레이아웃 — 선두 mouse 글리프 + 제목/본문(세로). 디자인
        // `MouseCaptureBannerG`(overlays-shared.jsx) 전사: glyph(banner-icon-fg) +
        // flex 본문, 우측엔 우상단 ×(매니저가 그림) 자리를 남긴다.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            let glyph = theme.icon_glyph_size_md.value();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
            icons::MOUSE
                .image(glyph, theme.banner_icon_fg().to_egui())
                .paint_at(ui, rect);
            ui.vertical(|ui| {
                // 우상단 × / 카운트다운 affordance 자리를 본문에서 비워둔다.
                ui.set_max_width(
                    (ui.available_width() - theme.item_height_interactive.value()).max(0.0),
                );
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                ui.label(
                    egui::RichText::new(t("banner.mouse_capture.title"))
                        .size(theme.font_size_body.value())
                        .strong()
                        .color(theme.banner_fg().to_egui()),
                );
                ui.label(
                    egui::RichText::new(t("banner.mouse_capture.body"))
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        });
    }

    /// OSC 133 셸 통합 미설치 안내. surface 가 출력을 내고 있는데도 일정
    /// 시간이 지나도록 `PromptBoundary` 를 한 번도 못 받았을 때 표시 — 설명만
    /// 하고 자동 조치는 없다(mouse-capture 배너와 동일 형태).
    pub const BANNER_SHELL_INTEGRATION_MISSING: BannerId = "shell-integration-missing";

    fn content_shell_integration_missing(ui: &mut egui::Ui, theme: &Theme) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            let glyph = theme.icon_glyph_size_md.value();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
            icons::TERM
                .image(glyph, theme.banner_icon_fg().to_egui())
                .paint_at(ui, rect);
            ui.vertical(|ui| {
                ui.set_max_width(
                    (ui.available_width() - theme.item_height_interactive.value()).max(0.0),
                );
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                ui.label(
                    egui::RichText::new(t("banner.shell_integration_missing.title"))
                        .size(theme.font_size_body.value())
                        .strong()
                        .color(theme.banner_fg().to_egui()),
                );
                ui.label(
                    egui::RichText::new(t("banner.shell_integration_missing.body"))
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        });
    }

    /// 전체 빌트인 배너 정의.
    // 이유: 호출부가 debug.rs(`#![cfg(debug_assertions)]`)뿐이라 release 빌드에서
    // 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn all_defs() -> &'static [BannerDef] {
        &DEFS
    }

    static DEFS: [BannerDef; 2] = [
        BannerDef {
            id: BANNER_MOUSE_CAPTURE,
            ttl_seconds: None,
            content_fn: content_mouse_capture,
        },
        BannerDef {
            id: BANNER_SHELL_INTEGRATION_MISSING,
            ttl_seconds: None,
            content_fn: content_shell_integration_missing,
        },
    ];

    /// id 로 정의 조회. 런타임 문자열(IPC/CLI 입력)도 조회할 수 있도록 `&str` 을
    /// 받는다 — 매칭된 def 의 `id` 는 여전히 `&'static str` 이다.
    pub fn find(id: &str) -> Option<&'static BannerDef> {
        DEFS.iter().find(|d| d.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persistent(id: BannerId, scope: BannerScope) -> BannerState {
        BannerState::persistent(id, scope)
    }

    #[test]
    fn push_into_empty_scope_shows_immediately() {
        let mut mgr = BannerManager::new();
        let out = mgr.push(persistent("a", BannerScope::Surface(1)));
        assert_eq!(out, BannerPushOutcome::Shown);
        assert_eq!(mgr.shown_banners().count(), 1);
        assert_eq!(mgr.total_queued(), 0);
    }

    #[test]
    fn queue_caps_at_one_shown_plus_five_queued_and_drops_seventh() {
        // 같은 스코프에 7개 push → 표시 1 + 큐 5(가득), 7번째 무시.
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(1);
        let ids = ["a", "b", "c", "d", "e", "f", "g"];
        let outcomes: Vec<_> = ids
            .iter()
            .map(|id| mgr.push(persistent(id, scope.clone())))
            .collect();
        assert_eq!(outcomes[0], BannerPushOutcome::Shown);
        for o in &outcomes[1..6] {
            assert_eq!(*o, BannerPushOutcome::Queued);
        }
        assert_eq!(outcomes[6], BannerPushOutcome::Ignored);
        assert_eq!(mgr.shown_banners().count(), 1);
        assert_eq!(mgr.total_queued(), 5);
    }

    #[test]
    fn same_id_shown_with_ttl_resets_countdown() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Pane(7);
        mgr.push(BannerState::with_ttl("ttl", scope.clone(), 6));
        // 카운트다운 일부 진행.
        mgr.advance(3000.0, |_, _| false);
        let before = mgr.shown_banners().next().unwrap().remaining_ms;
        assert!(before < 6000.0);
        // 동일 id 재발화 → 리셋, 새로 안 생김.
        let out = mgr.push(BannerState::with_ttl("ttl", scope.clone(), 6));
        assert_eq!(out, BannerPushOutcome::ResetCountdown);
        assert_eq!(mgr.shown_banners().count(), 1);
        assert_eq!(mgr.total_queued(), 0);
        assert_eq!(mgr.shown_banners().next().unwrap().remaining_ms, 6000.0);
    }

    #[test]
    fn same_id_shown_without_ttl_is_ignored() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::View;
        mgr.push(persistent("x", scope.clone()));
        let out = mgr.push(persistent("x", scope));
        assert_eq!(out, BannerPushOutcome::Ignored);
        assert_eq!(mgr.shown_banners().count(), 1);
    }

    #[test]
    fn same_id_in_queue_is_ignored() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(1);
        mgr.push(persistent("a", scope.clone())); // shown
        mgr.push(persistent("b", scope.clone())); // queued
        let out = mgr.push(persistent("b", scope.clone())); // dup queued → ignore
        assert_eq!(out, BannerPushOutcome::Ignored);
        assert_eq!(mgr.total_queued(), 1);
    }

    #[test]
    fn close_promotes_queue_head() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(1);
        mgr.push(persistent("a", scope.clone()));
        mgr.push(persistent("b", scope.clone()));
        assert_eq!(mgr.shown_banners().next().unwrap().id, "a");
        assert!(mgr.close_shown(&scope).is_some());
        assert_eq!(mgr.shown_banners().next().unwrap().id, "b");
        assert_eq!(mgr.total_queued(), 0);
    }

    #[test]
    fn close_last_removes_scope_lane() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(1);
        mgr.push(persistent("a", scope.clone()));
        assert!(mgr.close_shown(&scope).is_some());
        assert!(!mgr.has_any());
        assert!(mgr.close_shown(&scope).is_none()); // 더 닫을 것 없음
    }

    #[test]
    fn ttl_counts_down_and_expires_to_promote() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Pane(1);
        mgr.push(BannerState::with_ttl("ttl", scope.clone(), 2)); // 2000ms
        mgr.push(persistent("next", scope.clone())); // queued
        mgr.advance(1500.0, |_, _| false);
        assert_eq!(mgr.shown_banners().next().unwrap().id, "ttl");
        mgr.advance(600.0, |_, _| false); // 2100ms 누적 → 만료
        assert_eq!(mgr.shown_banners().next().unwrap().id, "next");
    }

    #[test]
    fn ttl_pauses_when_flagged() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Pane(1);
        mgr.push(BannerState::with_ttl("ttl", scope.clone(), 5));
        // 정지 플래그(hover/백그라운드) → 감소 안 함.
        mgr.advance(3000.0, |_, _| true);
        assert_eq!(mgr.shown_banners().next().unwrap().remaining_ms, 5000.0);
        // 정지 해제 → 감소.
        mgr.advance(1000.0, |_, _| false);
        assert_eq!(mgr.shown_banners().next().unwrap().remaining_ms, 4000.0);
    }

    #[test]
    fn ttl_does_not_affect_persistent_banner() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::View;
        mgr.push(persistent("p", scope.clone()));
        mgr.advance(100_000.0, |_, _| false);
        assert_eq!(mgr.shown_banners().count(), 1);
    }

    #[test]
    fn cross_scope_recessed_lower_priority() {
        // View(상위) + Surface(하위) 동시 표시 → Surface 가 recessed.
        let mut mgr = BannerManager::new();
        mgr.push(persistent("v", BannerScope::View));
        mgr.push(persistent("s", BannerScope::Surface(1)));
        let view_max = BannerScope::View.priority();
        assert!(!mgr.is_recessed(&BannerScope::View, view_max));
        assert!(mgr.is_recessed(&BannerScope::Surface(1), view_max));
    }

    #[test]
    fn remaining_seconds_ceils() {
        let b = BannerState::with_ttl("t", BannerScope::View, 6);
        assert_eq!(b.remaining_seconds(), Some(6));
        let mut b2 = b.clone();
        b2.remaining_ms = 5400.0;
        assert_eq!(b2.remaining_seconds(), Some(6)); // ceil
        b2.remaining_ms = 4001.0;
        assert_eq!(b2.remaining_seconds(), Some(5));
        let p = BannerState::persistent("p", BannerScope::View);
        assert_eq!(p.remaining_seconds(), None);
    }

    #[test]
    fn close_by_id_removes_from_queue_or_shown() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(1);
        mgr.push(persistent("a", scope.clone()));
        mgr.push(persistent("b", scope.clone()));
        // 큐의 b 제거.
        assert!(mgr.close_by_id("b"));
        assert_eq!(mgr.total_queued(), 0);
        assert_eq!(mgr.shown_banners().next().unwrap().id, "a");
        // 표시 중 a 제거.
        assert!(mgr.close_by_id("a"));
        assert!(!mgr.has_any());
    }

    #[test]
    fn set_countdown_overrides_remaining() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Pane(1);
        mgr.push(BannerState::with_ttl("t", scope.clone(), 10));
        assert!(mgr.set_countdown(&scope, 2));
        assert_eq!(mgr.shown_banners().next().unwrap().remaining_ms, 2000.0);
    }

    #[test]
    fn independent_scopes_each_show_one() {
        let mut mgr = BannerManager::new();
        mgr.push(persistent("a", BannerScope::Surface(1)));
        mgr.push(persistent("b", BannerScope::Surface(2)));
        assert_eq!(mgr.shown_banners().count(), 2);
    }

    // ── plugin egui-mesh 배너(A3) ──

    fn plugin_mesh(instance_id: u64, scope: BannerScope, ttl: Option<u32>) -> BannerState {
        BannerState::plugin_mesh(scope, "com.example.p".to_string(), instance_id, ttl, 64.0)
    }

    #[test]
    fn plugin_mesh_key_uses_instance_id() {
        let a = plugin_mesh(1, BannerScope::Surface(9), None);
        let b = plugin_mesh(2, BannerScope::Surface(9), None);
        assert_eq!(a.key(), BannerKey::Plugin(1));
        assert_ne!(a.key(), b.key());
        // host 배너와도 절대 충돌하지 않는다.
        assert_ne!(
            a.key(),
            BannerState::persistent("x", BannerScope::Surface(9)).key()
        );
    }

    #[test]
    fn distinct_plugin_instances_queue_not_dedup() {
        // 같은 스코프에 서로 다른 instance 두 개 → 표시 1 + 큐 1 (id sentinel 로 dedup 안 됨).
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(9);
        assert_eq!(
            mgr.push(plugin_mesh(1, scope.clone(), None)),
            BannerPushOutcome::Shown
        );
        assert_eq!(
            mgr.push(plugin_mesh(2, scope.clone(), None)),
            BannerPushOutcome::Queued
        );
        assert_eq!(mgr.shown_banners().count(), 1);
        assert_eq!(mgr.total_queued(), 1);
        assert_eq!(mgr.plugin_instances().count(), 2);
    }

    #[test]
    fn plugin_ttl_expiry_records_close_reason() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(9);
        mgr.push(plugin_mesh(7, scope.clone(), Some(2))); // 2000ms
        mgr.advance(2100.0, |_, _| false); // 만료
        let closed = mgr.drain_closed_plugin_banners();
        assert_eq!(closed, vec![(7, PluginBannerCloseKind::Ttl)]);
        assert!(!mgr.has_any());
        // drain 후 비워짐.
        assert!(mgr.drain_closed_plugin_banners().is_empty());
    }

    #[test]
    fn close_by_instance_removes_plugin_banner() {
        let mut mgr = BannerManager::new();
        let scope = BannerScope::Surface(9);
        mgr.push(plugin_mesh(1, scope.clone(), None)); // shown
        mgr.push(plugin_mesh(2, scope.clone(), None)); // queued
        // 큐의 2 제거.
        assert!(mgr.close_by_instance(2));
        assert_eq!(mgr.total_queued(), 0);
        // 표시 중 1 제거 → 큐 없어 lane 사라짐.
        assert!(mgr.close_by_instance(1));
        assert!(!mgr.has_any());
        assert!(!mgr.close_by_instance(1)); // 더 없음
    }
}
