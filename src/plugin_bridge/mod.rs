//! Plugin manager 가 본 바이너리 도메인 (engine / file / shortcuts / model 등)
//! 과 결합한 코드를 모아 두는 bin-side glue.
//!
//! tasty-host-plugin (manager crate) 가 본 바이너리를 역참조할 수 없으므로,
//! 본 모듈이 *protocol port impl* 의 본 바이너리 잔존 지점 역할을 한다.

#[cfg(feature = "gui")]
pub mod banner_render;
pub mod egui_mesh_surface;
#[cfg(feature = "gui")]
pub mod key_dispatch;
#[cfg(feature = "gui")]
pub mod manifest_validate;
pub mod mesh_forward;
#[cfg(feature = "gui")]
pub mod popup_render;
#[cfg(feature = "gui")]
pub mod remote_kind;
pub mod remote_surface;
#[cfg(feature = "gui")]
pub mod wire_scroll;

// host_cmd / host_actions 는 tasty-host-plugin crate 가 owning (manager 가 채널
// 송신자). 본 바이너리에서는 그대로 같은 경로로 노출하기 위해 re-export.
// host_actions 는 gui-only (keybindings_tab/plugins), host_cmd 는 headless 도
// 사용 (remote_surface).
#[cfg(feature = "gui")]
pub use tasty_host_plugin::host_actions;
pub use tasty_host_plugin::host_cmd;

/// egui 가 준 논리 사각형을 mesh 합성용 물리 사각형으로 올린다.
///
/// plugin mesh(배너·popup)는 egui 좌표로 배치되고 GPU 합성은 물리 픽셀로 하므로 매
/// 프레임 이 경계를 넘는다. 네 변에 각각 `× ppp` 를 곱하던 자리를 한 번의
/// `LogicalRect::to_physical` 로 모은다 — 곱셈이 네 번이면 하나를 빠뜨려도 컴파일이
/// 통과하고, 그 결과는 mesh 가 화면의 엉뚱한 자리에 붙는 형태로만 드러난다.
#[cfg(feature = "gui")]
pub(crate) fn mesh_region_of(
    content_rect: egui::Rect,
    pixels_per_point: f32,
) -> crate::model::PhysicalRect {
    use crate::model::{LogicalPx, LogicalRect};
    LogicalRect {
        x: LogicalPx(content_rect.min.x),
        y: LogicalPx(content_rect.min.y),
        width: LogicalPx(content_rect.width()),
        height: LogicalPx(content_rect.height()),
    }
    .to_physical(pixels_per_point)
}

/// egui-mesh forward 한 벌이 **세 채널에서 똑같이** 들고 다니는 상태 — surface(A1)·
/// popup(A2)·banner(A3).
///
/// 세 채널은 각자 다른 것을 그리지만 "지금 `set_context` 를 다시 보내야 하는가" 를
/// 판정하는 방식이 같다: 마지막으로 보낸 geom/Theme 과 지금 값을 견주고, 아직 paint
/// frame 을 못 받았으면 bootstrap 을 1회만 보내고, 렌더 prepare 가 textures_delta 체인
/// 단절을 알렸으면 다음 송신에 full 을 실어 보낸다.
///
/// **그래서 칸도 판정도 여기 한 벌만 있다.** 예전에는 이 넷이 세 벌로 흩어져 있었고
/// (`AppState` 의 평행 `HashMap` 두 무리 + surface 구조체 한 벌), 필드 doc 이 서로를
/// "같은 모양" 이라고 가리켰지만 그 평행을 지키는 것이 아무것도 없었다 — 실제로 세
/// 자리가 갈려 있었다: manager 부재 시 popup 은 `bootstrap_sent` 를 안 비웠고, 인스턴스
/// 정리에서 popup 은 `last_theme` 을 안 걸렀으며(죽은 인스턴스의 테마가 남았다), 세
/// 갈래의 좌변 갱신 순서도 제각각이었다. 한 타입으로 모으면 그 자리들이 사라진다 —
/// 갈렸는지 재는 장치가 아니라 갈릴 자리가 없는 것이 답이다.
///
/// 채널 고유의 칸은 여기 넣지 않는다. surface 의 focus 추적·입력 누적은 사본이 아니라
/// 그 채널 하나만의 것이라 각자 자리에 남는다. 무입력 강제 repaint 도 여기서 합치지
/// 않았는데, 이유는 "세 채널이 갖는가" 가 아니라 아래 세 가지다(첫째는 차이가
/// 아니라는 확인이고, 나머지 둘이 실제로 갈리는 축이다).
///
/// - **채우는 self-repaint 는 셋이 같다.** plugin SDK 의 `schedule_self_repaint` 는
///   surface·popup·banner 세 판이 `arm_self_repaint_timer`(`crates/tasty-plugin-sdk/src/egui_surface.rs`)
///   하나를 공유하고, 실어 보내는 variant(`SurfaceInvalidated`·`PopupInvalidated`·
///   `BannerInvalidated`)만 다르다. 그러니 이 사건은 차이가 아니다.
/// - **담는 자리가 다르다.** surface 는 자기 구조체의 `bool` 한 칸
///   (`MeshForwardState::invalidated`, `src/view/main/egui_mesh.rs`), popup·banner 는
///   `AppState` 의 `HashSet<u64>` 두 개(`plugin_mesh_popup_pending_repaint`·
///   `plugin_mesh_banner_pending_repaint`)다 — **대상 상태 구조체 안의 칸** vs
///   **`AppState` 의 별도 집합**. 대상 하나당 한 칸이라는 점은 셋이 같고, 다른 것은
///   그 칸이 어디에 사는가다.
/// - **추가 진입로도 채널마다 다르다.** surface 는 파일 변경 통지가 같은
///   `SurfaceInvalidated` 를 타고 와 `mark_surface_invalidated` 로 그 칸을 세우고,
///   popup 은 ADR-0056 의 비동기 host→plugin push 결과가 같은 칸을 세우며
///   (git-viewer 원격 조회 결과 뒤의 강제 repaint, `src/app/attach_client.rs` 두
///   자리), banner 는 self-repaint 하나뿐이다.
#[derive(Default)]
pub(crate) struct MeshForwardCommon {
    /// 마지막으로 보낸 `(width_px, height_px, ppp.to_bits())`. 변경 감지의 좌변.
    pub(crate) last_geom: Option<(u32, u32, u32)>,
    /// 마지막으로 보낸 Theme 스냅샷. 크기·입력이 무변이어도 테마가 바뀌면 재forward.
    pub(crate) last_theme: Option<tasty_plugin_protocol::ThemeWire>,
    /// paint frame 을 아직 못 받은 동안 bootstrap `set_context` 를 1회만 보내기 위한
    /// 래치. frame 이 보이면 풀려, crash 로 frame 이 사라지면 재bootstrap 된다.
    /// 핵심: 첫 frame(폰트 atlas 동봉)을 host 가 반드시 decode 하도록 스팸하지 않는다.
    pub(crate) bootstrap_sent: bool,
    /// 렌더 prepare 가 textures_delta 체인 단절을 감지했다 — 다음 `set_context` 에
    /// `need_full_textures` 를 실어 보낸다(송신 시 소거).
    pub(crate) pending_full: bool,
    /// bootstrap `set_context` 를 보낸 시각. 이후에도 frame 이 오지 않으면
    /// [`BLANK_MESH_GRACE`] 경과 시점에 1회 경고한다 — `blank_warned` 참조.
    bootstrap_at: Option<std::time::Instant>,
    /// "빈 화면" 경고를 이미 냈다 — 매 frame 반복 로그를 막는 래치.
    /// frame 이 한 번이라도 도착하면 해제되어, 이후 plugin crash 로 다시 비면 재경고한다.
    blank_warned: bool,
}

/// bootstrap `set_context` 를 보낸 뒤 이 시간이 지나도록 plugin 이 frame 을 하나도
/// 보내지 않으면 그 채널은 사실상 빈 화면으로 멈춘 것으로 본다.
///
/// 정상 경로에서 첫 paint 는 수십 ms 안에 온다(plugin 프로세스는 이미 기동·handshake
/// 완료 상태이고 남은 일은 콘텐츠 적재 + tessellate 뿐). 3초는 느린 디스크의 대용량
/// 파일 적재까지 흡수하면서 실제 고장을 놓치지 않는 선.
const BLANK_MESH_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

impl MeshForwardCommon {
    /// paint frame 유무를 bootstrap 워치독에 반영한다 — 세 채널이 매 프레임 부른다.
    ///
    /// frame 이 보이는 동안은 bootstrap 무장을 풀어 둔다(이후 plugin crash 로 frame 이
    /// 사라지면 다음 프레임이 다시 bootstrap 한다). 반대로 bootstrap 을 보냈는데
    /// [`BLANK_MESH_GRACE`] 가 지나도록 frame 이 하나도 오지 않았으면 = 사용자에게는 빈
    /// 화면이다. plugin 쪽 실패(paint 에러/hang/crash)는 plugin 자체 로그에만 남고 host
    /// 의 forward 루프는 frame 없는 채널을 조용히 건너뛰므로, host stderr 만 보는
    /// 사람에게는 아무 징후도 없다. 그 침묵을 여기서 깬다 — 래치(`blank_warned`)가 이
    /// 구조체 한 벌에 있으니 **대상 하나당 1회**다(surface 는 surface 당, popup·banner 는
    /// 인스턴스 당). 채널 종류당이 아니다 — 같은 채널의 다른 인스턴스는 각자 한 번씩
    /// 경고한다.
    ///
    /// 원인은 여기서 알 수 없다(host 는 실패 통지를 받지 않는다) — plugin 로그 경로를
    /// 함께 찍어 다음 확인처를 명시한다.
    ///
    /// `channel` 은 경고문이 대상을 지목하는 구절이다(예: `surface 12 (kind 'markdown',
    /// plugin 'com.tasty.image')`). [`std::format_args!`] 로 넘기면 경고가 안 나는
    /// 프레임에서는 아무것도 할당하지 않는다.
    pub(crate) fn watch_blank(
        &mut self,
        has_frame: bool,
        channel: std::fmt::Arguments<'_>,
        plugin_id: &str,
    ) {
        if has_frame {
            self.bootstrap_sent = false;
            self.bootstrap_at = None;
            self.blank_warned = false;
            return;
        }
        if self.blank_warned || !self.bootstrap_sent {
            return;
        }
        if !self
            .bootstrap_at
            .is_some_and(|t| t.elapsed() >= BLANK_MESH_GRACE)
        {
            return;
        }
        tracing::error!(
            "egui-mesh {channel} has received no frame {:.0}s after bootstrap — it is blank on \
             screen. Check the plugin's own log: `tasty plugin logs {plugin_id}`",
            BLANK_MESH_GRACE.as_secs_f32(),
        );
        self.blank_warned = true;
    }

    pub(crate) fn geom_changed(&self, geom: (u32, u32, u32)) -> bool {
        self.last_geom != Some(geom)
    }

    pub(crate) fn theme_changed(&self, theme: &tasty_plugin_protocol::ThemeWire) -> bool {
        self.last_theme.as_ref() != Some(theme)
    }

    pub(crate) fn need_bootstrap(&self, has_frame: bool) -> bool {
        !has_frame && !self.bootstrap_sent
    }

    /// full 재전송 요청을 1회 소비한다.
    pub(crate) fn take_pending_full(&mut self) -> bool {
        std::mem::take(&mut self.pending_full)
    }

    /// `set_context` 를 보낸 직후의 좌변 갱신 — 다음 프레임의 변경 감지 기준이 된다.
    pub(crate) fn record_sent(
        &mut self,
        geom: (u32, u32, u32),
        theme: &tasty_plugin_protocol::ThemeWire,
        has_frame: bool,
    ) {
        self.last_geom = Some(geom);
        self.last_theme = Some(theme.clone());
        if !has_frame {
            self.bootstrap_sent = true;
            // 첫 bootstrap 시각만 기록 — 이후 입력/리사이즈로 재forward 될 때마다
            // 갱신하면 grace 가 계속 밀려 빈 화면을 영영 못 잡는다.
            self.bootstrap_at
                .get_or_insert_with(std::time::Instant::now);
        }
    }
}
