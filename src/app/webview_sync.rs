//! App synchronizes native WebView effects against the originating runtime resources.
//! Native windows, geometry caches and input handles remain owned by MainView.
use crate::plugin::PluginManager;
use crate::runtime::engine_access::EngineMut;
use crate::runtime::engine_access::EngineRef;
use crate::view::MainView;
use crate::view::main::redraw::{
    MAX_WEBVIEW_CREATE_ATTEMPTS, REVEAL_PENDING_WARN_AFTER, describe_webview_url,
    host_window_has_os_focus, next_webview_attempts, should_attempt_webview,
    take_hidden_webview_focus_targets, webview_release_candidate_is_live,
};
use crate::view::ui::View;
use tasty_model::html_script::BannerPhase;

pub(crate) fn synchronize(
    view: &mut MainView,
    engine: &mut EngineMut<'_>,
    plugins: Option<&PluginManager>,
    proofs: &super::html_runtime::NavigationProofs,
) {
    let stale: Vec<_> = view
        .webviews
        .keys()
        .copied()
        .filter(|surface| {
            !view
                .webview_runtime
                .get(surface)
                .is_some_and(|binding| binding.current(&engine.as_ref()))
        })
        .collect();
    for surface in stale {
        view.webviews.remove(&surface);
        view.webview_runtime.remove(&surface);
        view.webview_loaded_urls.remove(&surface);
        view.webview_applied_settings.remove(&surface);
        view.webview_create_attempts.remove(&surface);
        proofs.invalidate(&view.state.webview_identity, surface);
    }
    sync_webviews(view, &engine.as_ref(), plugins, proofs);
    if view.base.state.dirty {
        view.base.winit.request_redraw();
    }
}

/// `collect_html_surfaces` 의 결과. 활성 surface 는 세 맵에 같은 키로 들어 있다.
struct HtmlSurfaces {
    /// 플랫폼에 넘길 논리 좌표.
    bounds: std::collections::HashMap<u32, crate::webview::WebViewBounds>,
    /// egui 툴팁 배치에 쓸 물리 사각형.
    physical: std::collections::HashMap<u32, crate::model::PhysicalRect>,
    /// WebView 입력에서 뺄 host 입력 영역(WebView 기준 물리 px).
    holes: std::collections::HashMap<u32, Vec<crate::model::PhysicalRect>>,
    all_ids: Vec<u32>,
}

/// HTML surface 전체와 활성 surface의 영역을 수집한다. native 호출은 하지 않는다.
/// 활성 surface의 영역은 플랫폼에 넘길 논리 좌표와 egui 툴팁 배치에 쓸 물리 사각형을 함께 돌려준다.
fn collect_html_surfaces(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    scale_factor: f64,
) -> HtmlSurfaces {
    let terminal_rect = view.compute_terminal_rect();

    // Collect all Html surface IDs and their visibility/bounds
    let active_ws = view.state.active_workspace_index(engine);
    let mut active_html: std::collections::HashMap<u32, crate::webview::WebViewBounds> =
        std::collections::HashMap::new();
    let mut active_physical: std::collections::HashMap<u32, crate::model::PhysicalRect> =
        std::collections::HashMap::new();
    let mut active_holes: std::collections::HashMap<u32, Vec<crate::model::PhysicalRect>> =
        std::collections::HashMap::new();
    let mut all_html_ids: Vec<u32> = Vec::new();
    let window = view.base.gpu.size();
    let resize_band = view.window_resize_band();

    for (ws_idx, ws) in engine.workspaces().into_iter().enumerate() {
        let pane_rects = view
            .state
            .pane_rects(engine, ws, terminal_rect, scale_factor as f32);
        let pane_dividers =
            view.state
                .pane_dividers(engine, ws, terminal_rect, scale_factor as f32);
        for (pane_id, pane_rect) in &pane_rects {
            if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                // tab bar 아래 콘텐츠 영역 — 탭 내부 분할(SurfaceGroup)의 leaf rect
                // 는 이 영역을 기준으로 계산한다(`impl_pty` / `focus` 와 동일 유도).
                let content_rect = crate::model::PhysicalRect {
                    x: pane_rect.x,
                    y: pane_rect.y + view.state.tab_bar_height,
                    width: pane_rect.width,
                    height: (pane_rect.height - view.state.tab_bar_height)
                        .max(crate::model::PhysicalPx(1.0)),
                };
                for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                    let Some(_) = tab.layout_if_initialized() else {
                        continue;
                    };
                    // Only visible if: active workspace AND active tab
                    let is_visible =
                        ws_idx == active_ws && tab_idx == view.state.navigation.tab_index(pane);
                    // native WebView가 가리면 안 되는 host 입력 영역: pane·surface 분할선 hit 띠와
                    // 창 가장자리 리사이즈 밴드. 활성 탭에서만 쓴다.
                    let input_zones = if is_visible {
                        let mut dividers = pane_dividers.clone();
                        dividers.extend(view.state.surface_dividers(
                            engine,
                            tab,
                            content_rect,
                            scale_factor as f32,
                        ));
                        crate::state::webview_edges::host_input_zones(
                            &dividers,
                            crate::model::PhysicalPx(window.width as f32),
                            crate::model::PhysicalPx(window.height as f32),
                            resize_band,
                            scale_factor as f32,
                        )
                    } else {
                        Vec::new()
                    };
                    // 비포커스 leaf에도 native WebView가 필요하므로 탭 전체를 순회한다.
                    for region in view.state.tab_surface_regions(
                        engine,
                        tab,
                        content_rect,
                        scale_factor as f32,
                    ) {
                        let sid = region.id;
                        let leaf_rect = region.rect;
                        let Some(surface) = engine.find_surface_by_id(region.id) else {
                            continue;
                        };
                        if surface.webview_url().is_none() {
                            continue;
                        }
                        all_html_ids.push(sid);
                        if is_visible {
                            // 패널 바깥쪽 변에는 divider 드래그 영역만큼 여백을 둔다.
                            // egui chrome도 같은 여백 안쪽으로 내용을 자른다.
                            let [left, right, bottom] =
                                crate::state::webview_edges::webview_edge_inset(
                                    leaf_rect,
                                    content_rect,
                                    scale_factor as f32,
                                )
                                .map(|v| v.value() as f64);
                            // 물리 사각형을 만든 뒤 플랫폼 API에 맞는 논리 좌표로 변환한다.
                            let top = view.html_script_banner_top(sid, scale_factor);
                            let physical = crate::webview::PhysicalWebViewBounds {
                                x: leaf_rect.x.value() as f64 + left,
                                y: leaf_rect.y.value() as f64 + top,
                                width: (leaf_rect.width.value() as f64 - left - right).max(1.0),
                                height: (leaf_rect.height.value() as f64 - bottom - top).max(1.0),
                            };
                            let bounds = crate::webview::WebViewBounds::from_physical(
                                physical,
                                scale_factor,
                            );
                            active_html.insert(sid, bounds);
                            let webview_rect = crate::model::PhysicalRect {
                                x: crate::model::PhysicalPx(physical.x as f32),
                                y: crate::model::PhysicalPx(physical.y as f32),
                                width: crate::model::PhysicalPx(physical.width as f32),
                                height: crate::model::PhysicalPx(physical.height as f32),
                            };
                            active_holes.insert(
                                sid,
                                crate::state::webview_edges::webview_input_holes(
                                    webview_rect,
                                    &input_zones,
                                ),
                            );
                            active_physical.insert(sid, webview_rect);
                        }
                    }
                }
            }
        }
    }

    HtmlSurfaces {
        bounds: active_html,
        physical: active_physical,
        holes: active_holes,
        all_ids: all_html_ids,
    }
}

/// 필요한 설정을 읽은 뒤 HTML surface의 native WebView를 만들고 페이지를 연다.
fn create_missing_webviews(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    all_html_ids: &[u32],
    active_html: &std::collections::HashMap<u32, crate::webview::WebViewBounds>,
    scale_factor: f64,
) {
    // Create new webviews for Html panels that don't have one yet
    for &sid in all_html_ids {
        if !view.webviews.contains_key(&sid) {
            // 재시도 상한에 도달한 surface는 다시 만들지 않는다.
            let attempts = view.webview_create_attempts.get(&sid).copied().unwrap_or(0);
            if !should_attempt_webview(attempts) {
                continue;
            }
            // Find the URL for this surface
            let url = find_webview_url(view, engine, sid);
            // 생성 시 적용할 plugin 설정(부재 시 default) 을 미리 해석한다.
            let settings = resolve_webview_settings(view, engine, sid);
            match crate::webview::PlatformWebView::new(
                view.base.winit.as_ref(),
                active_html
                    .get(&sid)
                    .copied()
                    .unwrap_or(crate::webview::WebViewBounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    }),
                scale_factor,
                sid,
                view.webview_key_bridge.clone(),
            ) {
                Ok(wv) => {
                    let Some(binding) =
                        super::html_runtime::NativeWebviewBinding::capture(engine, sid)
                    else {
                        continue;
                    };
                    let bounds = active_html.get(&sid);
                    tracing::debug!(
                        "WebView surface {sid}: created (visible={}, bounds={:?}, url={})",
                        bounds.is_some(),
                        bounds,
                        describe_webview_url(url.as_ref())
                    );
                    attach_script_gate(view, engine, sid, &wv, &settings);
                    load_initial_url(view, engine, sid, &wv, url.as_ref());
                    // 생성 직후 HTML viewer 설정(zoom/JS/scheme/remote) 적용 + 기록.
                    settings.apply(&wv);
                    // Start hidden if not active
                    if !active_html.contains_key(&sid) {
                        wv.set_visible(false);
                    }
                    view.webviews.insert(sid, wv);
                    view.webview_runtime.insert(sid, binding);
                    view.webview_applied_settings.insert(sid, settings);
                    view.webview_create_attempts.remove(&sid);
                }
                Err(e) => record_webview_failure(view, sid, attempts, &e),
            }
        }
    }
}

/// html surface에 문서 단위 스크립트 허용을 붙인다(ADR-0053). 첫 로드 전에 sandbox 값을 맞춘다.
fn attach_script_gate(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    sid: u32,
    wv: &crate::webview::PlatformWebView,
    settings: &crate::webview::HtmlWebViewSettings,
) {
    let Some(rs) = find_remote_surface(view, engine, sid).filter(|rs| rs.kind_static == "html")
    else {
        return;
    };
    let gate =
        crate::webview::script_gate::ScriptGate::new(sid, std::sync::Arc::clone(&rs.html_script));
    gate.set_sandbox(!settings.javascript_enabled);
    wv.attach_script_gate(gate);
}

/// 허용 직후의 재로드와 debug 탐색 조작을 native webview에 전달한다.
fn apply_webview_requests(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
) {
    for (sid, wv) in &view.webviews {
        let reload = find_remote_surface(view, engine, *sid)
            .is_some_and(|rs| rs.with_html_script(|st| st.take_reload_request()));
        if reload {
            tracing::debug!("WebView surface {sid}: reloading after the script allowance");
            wv.reload();
        }
    }
    #[cfg(debug_assertions)]
    for (sid, action) in std::mem::take(&mut view.state.debug_webview_history) {
        match view.webviews.get(&sid) {
            Some(wv) => wv.debug_history(action),
            None => tracing::warn!("debug webview {action:?}: surface {sid} has no webview"),
        }
    }
}

/// 생성 실패 횟수를 갱신하고 첫 실패와 재시도 중단 시에만 경고한다.
/// 영구 실패는 즉시 상한으로, 일시 실패는 한 번씩 올린다.
fn record_webview_failure(
    view: &mut MainView,
    sid: u32,
    attempts: u32,
    err: &crate::webview::WebViewCreateError,
) {
    let next = next_webview_attempts(attempts, err);
    view.webview_create_attempts.insert(sid, next);
    if attempts == 0 {
        tracing::warn!("Failed to create WebView for surface {sid}: {err}");
    }
    if next >= MAX_WEBVIEW_CREATE_ATTEMPTS {
        // 영구 실패는 한 번에 상한이 되므로 로그에는 실제 시도 횟수를 쓴다.
        let made = attempts + 1;
        tracing::warn!(
            concat!(
                "Giving up on the WebView for surface {} after {} attempt(s): ",
                "{} — 이 surface 는 다시 열 때까지 비어 있다"
            ),
            sid,
            made,
            err
        );
    }
}

/// URL scheme이 있으면 페이지를 열고, 없으면 HTML 본문으로 로드한다.
fn load_initial_url(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    sid: u32,
    wv: &crate::webview::PlatformWebView,
    url: Option<&String>,
) {
    let Some(url) = url else {
        // 수집 단계(`collect_html_surfaces`)가 URL 이 있는 surface 만 담으므로
        // 여기까지 와서 None 이면 그 사이에 사라진 것이다. 로드가 없으니 nav 는
        // Idle 에 머물고 화면은 비어 보인다.
        tracing::warn!("WebView surface {sid}: created without a URL; nothing will be loaded");
        return;
    };
    note_host_webview_load(view, engine, sid);
    if crate::webview::is_navigable_url(url) {
        wv.load_url(url);
    } else {
        wv.load_html(url);
    }
    view.webview_loaded_urls.insert(sid, url.clone());
}

/// 페이지가 계속 숨겨져 있으면 surface별로 한 번 경고한다.
/// 상태 변경이나 재로드는 하지 않는다.
fn note_reveal_pending(view: &mut MainView, pending: &[(u32, crate::webview::NavState)]) {
    let now = std::time::Instant::now();
    for &(sid, nav) in pending {
        let entry = view
            .webview_reveal_pending
            .entry(sid)
            .or_insert((now, false));
        if !entry.1 && now.duration_since(entry.0) >= REVEAL_PENDING_WARN_AFTER {
            entry.1 = true;
            tracing::warn!(
                "WebView surface {sid}: still hidden {:?} after it became active \
                 (nav_state={nav:?}); the pane shows host chrome, not the page",
                REVEAL_PENDING_WARN_AFTER
            );
        }
    }
    // 드러났거나 사라진 surface 는 추적에서 뺀다 — 다시 보류되면 시계가 새로 시작한다.
    view.webview_reveal_pending
        .retain(|sid, _| pending.iter().any(|(p, _)| p == sid));
}

fn resync_webview_urls(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    all_html_ids: &[u32],
) {
    for &sid in all_html_ids {
        let Some(url) = find_webview_url(view, engine, sid) else {
            continue;
        };
        if view.webview_loaded_urls.get(&sid) == Some(&url) {
            continue;
        }
        if let Some(wv) = view.webviews.get(&sid) {
            note_host_webview_load(view, engine, sid);
            if crate::webview::is_navigable_url(&url) {
                wv.load_url(&url);
            } else {
                wv.load_html(&url);
            }
            view.webview_loaded_urls.insert(sid, url);
        }
    }
}

/// 드러난 WebView 영역을 기록한다. 탭 스트립 툴팁은 다음 프레임에 이 영역을 피해 배치한다.
/// HashMap 순회 순서가 바뀌어도 같은 값으로 보도록 surface id 순으로 정렬한다.
fn publish_native_content_rects(
    view: &mut MainView,
    active: &std::collections::HashMap<u32, crate::model::PhysicalRect>,
    mut revealed: Vec<u32>,
) {
    revealed.sort_unstable();
    let rects: Vec<_> = revealed
        .iter()
        .filter_map(|sid| active.get(sid).copied())
        .collect();
    let current = &view.state.native_content_rects;
    let unchanged =
        current.len() == rects.len() && current.iter().zip(&rects).all(|(a, b)| a.approx_eq(b));
    if !unchanged {
        view.state.native_content_rects = rects;
        view.mark_dirty();
    }
}

/// host 입력 영역을 native WebView 입력에서 뺀다. Linux는 X input shape 으로 빼고 WebView 를
/// surface 에 꽉 채운다. 다른 OS는 아직 방법이 없어 `webview_edge_inset` 여백으로 피한다.
#[cfg(target_os = "linux")]
fn apply_input_holes(wv: &crate::webview::PlatformWebView, holes: &[crate::model::PhysicalRect]) {
    let holes: Vec<[i32; 4]> = holes
        .iter()
        .map(|r| {
            let x0 = r.x.value().floor() as i32;
            let y0 = r.y.value().floor() as i32;
            let x1 = (r.x + r.width).value().ceil() as i32;
            let y1 = (r.y + r.height).value().ceil() as i32;
            [x0, y0, x1 - x0, y1 - y0]
        })
        .collect();
    wv.set_input_holes(&holes);
}

/// 입력 영역을 빼는 방법이 아직 없는 OS.
#[cfg(not(target_os = "linux"))]
fn apply_input_holes(_wv: &crate::webview::PlatformWebView, _holes: &[crate::model::PhysicalRect]) {
}

/// Synchronize native WebView instances with the current state.
/// Creates webviews for new Html panels, destroys removed ones,
/// updates bounds and visibility based on active workspace/tab.
fn sync_webviews(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    plugin_manager: Option<&PluginManager>,
    proofs: &super::html_runtime::NavigationProofs,
) {
    let scale_factor = view.base.gpu.scale_factor() as f64;
    let HtmlSurfaces {
        bounds: active_html,
        physical: active_physical,
        holes: active_holes,
        all_ids: all_html_ids,
    } = collect_html_surfaces(view, engine, scale_factor);
    create_missing_webviews(view, engine, &all_html_ids, &active_html, scale_factor);
    resync_webview_urls(view, engine, &all_html_ids);
    update_html_script_banners(view, engine, &all_html_ids);

    // When any egui overlay (context menu, popup, dialog) is open,
    // hide all WebViews so they don't cover the overlay.
    // Native views are always above the wgpu render surface in OS z-order.
    let overlay_open = view.state.has_egui_overlay_open();

    // 호스트 키 설정 또는 plugin 명령·override의 revision이 바뀔 때만
    // native 콜백이 사용할 단축키 스냅샷을 다시 만든다.
    let plugin_epoch =
        plugin_manager.map(|m| (m.command_registry.revision(), m.config.shortcut_revision()));
    if view.webview_policy_src.as_ref() != Some(&engine.runtime.settings.keybindings)
        || view.webview_policy_plugin_epoch != plugin_epoch
    {
        let kb = &engine.runtime.settings.keybindings;
        let plugin_combos = plugin_manager
            .map(|m| crate::plugin_bridge::key_dispatch::all_command_bindings(m, kb))
            .unwrap_or_default();
        view.webview_key_bridge.set_policy(
            crate::adapters::ui::input::shortcuts::webview_shortcut_policy(kb, plugin_combos),
        );
        view.webview_policy_src = Some(kb.clone());
        view.webview_policy_plugin_epoch = plugin_epoch;
    }

    // overlay를 열면 WebView 키보드 포커스를 호스트 창으로 돌린다. 숨기기만으로는
    // 포커스가 해제되지 않는다. 닫을 때 자동으로 native 자식에 포커스를 돌려주지는 않는다.
    // 다른 앱의 포커스를 바꾸지 않도록 이 창이 OS 포커스를 가질 때만 처리한다.
    if overlay_open {
        if !view.webview_overlay_focus_released
            && host_window_has_os_focus(view.base.state.focused, || {
                view.webviews.values().any(|wv| wv.holds_keyboard_focus())
            })
        {
            for wv in view.webviews.values() {
                wv.release_keyboard_focus();
            }
            view.webview_overlay_focus_released = true;
        }
    } else {
        view.webview_overlay_focus_released = false;
    }

    // 탭·workspace 전환으로 활성 탭에서 빠진 webview도 숨기기 전에 포커스를 회수한다.
    // Linux·Windows의 webview는 별도 OS 창이라 숨겨도 포커스가 부모로 돌아오지 않을 수 있다.
    // 숨긴 뒤에는 backend가 포커스가 이미 밖에 있다고 판정할 수 있어 순서가 중요하다.
    let now_active: std::collections::HashSet<u32> = active_html.keys().copied().collect();
    let release = take_hidden_webview_focus_targets(
        &view.webview_prev_active,
        &now_active,
        |sid| {
            webview_release_candidate_is_live(sid, &all_html_ids, |s| {
                view.webviews.contains_key(&s)
            })
        },
        &mut view.webview_focus_release_pending,
        || {
            host_window_has_os_focus(view.base.state.focused, || {
                view.webviews.values().any(|wv| wv.holds_keyboard_focus())
            })
        },
    );
    for sid in release {
        if let Some(wv) = view.webviews.get(&sid) {
            wv.release_keyboard_focus();
        }
    }
    view.webview_prev_active = now_active;

    // navigation이 Done일 때만 native 페이지를 표시한다. 그 전에는 egui의
    // 로딩·오류 표시가 native 페이지에 가려지지 않도록 숨긴다.
    let mut any_visible = false;
    let mut revealed = Vec::new();
    let mut reveal_pending: Vec<(u32, crate::webview::NavState)> = Vec::new();
    for (sid, wv) in &view.webviews {
        // active 면 bounds 는 숨겨져 있어도 갱신(다음 reveal 대비).
        if let Some(bounds) = active_html.get(sid) {
            wv.set_bounds(*bounds, scale_factor);
            apply_input_holes(wv, active_holes.get(sid).map_or(&[][..], Vec::as_slice));
        }
        let nav = wv.nav_state();
        // 드러나야 할 자리에 있는데(활성 tab · overlay 없음) nav 가 Done 이 아니면
        // 그 프레임의 그 surface 는 "만들어졌지만 안 보이는" 상태다.
        let wants_reveal = !overlay_open && active_html.contains_key(sid);
        let reveal = wants_reveal && nav == crate::webview::NavState::Done;
        if wants_reveal && !reveal {
            reveal_pending.push((*sid, nav));
        }
        wv.set_visible(reveal);
        any_visible |= reveal;
        revealed.extend(reveal.then_some(*sid));
    }
    publish_native_content_rects(view, &active_physical, revealed);
    note_reveal_pending(view, &reveal_pending);
    // 키 폴링 tick 의 게이트(`app::webview_keys`) — 드러난 webview 가 없으면
    // 키가 그리로 갈 수 없으므로 폴링을 세우지 않는다.
    view.webview_any_visible = any_visible;

    // Remove webviews for closed Html surfaces
    view.webviews.retain(|sid, _| all_html_ids.contains(sid));
    view.webview_runtime
        .retain(|sid, _| all_html_ids.contains(sid));
    view.webview_applied_settings
        .retain(|sid, _| all_html_ids.contains(sid));
    view.webview_loaded_urls
        .retain(|sid, _| all_html_ids.contains(sid));
    // 시도 횟수도 같이 지운다 — surface 를 닫았다 열면 예산이 새로 생긴다.
    view.webview_create_attempts
        .retain(|sid, _| all_html_ids.contains(sid));

    // 설정 변경 재적용 — 살아있는 webview 마다 현재 설정을 해석해, 마지막 적용값과
    // 다를 때만 backend 에 재적용한다(변경 없으면 backend 호출 0 — 매 프레임 호출 회피).
    let live_sids: Vec<u32> = view.webviews.keys().copied().collect();
    for sid in live_sids {
        let resolved = resolve_webview_settings(view, engine, sid);
        if view.webview_applied_settings.get(&sid) != Some(&resolved) {
            if let Some(wv) = view.webviews.get(&sid) {
                resolved.apply(wv);
            }
            view.webview_applied_settings.insert(sid, resolved);
        }
    }
    apply_webview_requests(view, engine);

    // native nav_state 를 RemoteSurface 로 mirror — egui 렌더 경로(egui_panels →
    // webview_chrome)가 다음 프레임에 읽어 loading/error chrome 을 그린다. borrow 충돌
    // 회피를 위해 (sid, nav) 를 먼저 수집한 뒤 기록한다. 전이가 있으면 mark_dirty 로
    // 한 프레임 더 그려 chrome 을 갱신한다(가시성 전환 자체는 위에서 native 즉시 적용).
    let navs: Vec<(u32, crate::webview::NavState)> = view
        .webviews
        .iter()
        .map(|(s, w)| (*s, w.nav_state()))
        .collect();
    let mut nav_changed = false;
    for (sid, nav) in navs {
        if let Some(rs) = find_remote_surface(view, engine, sid)
            && rs.nav_state() != nav
        {
            rs.set_nav_state(nav);
            nav_changed = true;
        }
    }
    if nav_changed {
        view.mark_dirty();
    }

    // navigation 시도를 소유 plugin에 알린다. 원격 콘텐츠 차단 여부와 별개이며
    // plugin이 없어도 큐를 비운다. 사용자 제스처와 페이지 작성자가 확인된 경우에만
    // 사용자 탐색 기록을 남긴다. plugin 응답보다 먼저 기록해야 한다(ADR-0031).
    let identity = view.state.webview_identity.clone();
    for (sid, wv) in &view.webviews {
        for nav in wv.take_pending_navigations() {
            if let Some(manager) = plugin_manager {
                let remote = find_remote_surface(view, engine, *sid);
                let owner =
                    remote.map(
                        |remote| crate::plugin_bridge::user_navigation::NavigationOwner {
                            plugin_id: remote.plugin_id.clone(),
                            wrote_page: remote.webview_page_by_owner(),
                        },
                    );
                proofs.record(
                    &identity,
                    *sid,
                    owner.as_ref(),
                    remote.map(|remote| &remote.webview_url),
                    &nav,
                );
                if let Some(owner) = owner {
                    manager.send_webview_navigation_attempt(
                        &owner.plugin_id,
                        &tasty_plugin_protocol::WebviewNavigationAttemptParams {
                            surface_id: *sid,
                            url: nav.url,
                        },
                    );
                }
            }
        }
    }
    let live: Vec<_> = view.webviews.keys().copied().collect();
    let takeovers: Vec<_> = live
        .iter()
        .map(|&sid| {
            (
                sid,
                find_remote_surface(view, engine, sid)
                    .is_some_and(|remote| remote.take_webview_owner_takeover()),
            )
        })
        .collect();
    proofs.settle_frame(&identity, &live, &takeovers);
}

/// surface_id 로 surface 를 전 workspace 에서 찾는다. 탭당 1 개만 보는
/// `Tab::surface()`(포커스 leaf) 가 아니라 각 탭의 `SurfaceLayout` 트리 전체를
/// 훑으므로, 탭 내부 분할(SurfaceGroup)의 비포커스 leaf 도 도달한다.
fn find_surface_anywhere<'e>(
    _view: &MainView,
    engine: &EngineRef<'e>,
    surface_id: u32,
) -> Option<&'e dyn crate::model::Surface> {
    engine.find_surface_by_id(surface_id)
}

/// surface_id 로 RemoteSurface 를 찾아 반환. nav_state mirror 기록에 쓴다.
fn find_remote_surface<'e>(
    view: &MainView,
    engine: &EngineRef<'e>,
    surface_id: u32,
) -> Option<&'e crate::plugin_bridge::remote_surface::RemoteSurface> {
    find_surface_anywhere(view, engine, surface_id)?
        .as_any()
        .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
}

/// webview surface 의 kind.
fn webview_surface_kind(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    surface_id: u32,
) -> Option<&'static str> {
    find_surface_anywhere(view, engine, surface_id).map(|s| s.kind())
}

/// surface 소유 plugin의 WebView 설정을 읽는다. 저장된 값이 없으면 기본값을 쓴다.
fn resolve_webview_settings(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    surface_id: u32,
) -> crate::webview::HtmlWebViewSettings {
    use crate::settings::PluginSettingValue;
    use crate::webview::{ColorScheme, HtmlWebViewSettings};

    let plugin_id = match webview_surface_kind(view, engine, surface_id)
        .and_then(crate::webview::webview_settings_plugin_id)
    {
        Some(id) => id,
        None => return HtmlWebViewSettings::default(),
    };
    let s = &engine.runtime.settings;
    let zoom_percent = match s.plugin_setting(plugin_id, "zoom") {
        Some(PluginSettingValue::Number(n)) => *n,
        _ => 100.0,
    };
    // HTML plugin은 임의 콘텐츠를 열 수 있어 기본적으로 JS를 막는다.
    // markdown은 정화한 문서의 탐색 스크립트를 실행해야 하므로 기본값이 다르다.
    // 사용자가 저장한 설정은 이 기본값보다 우선한다.
    // html은 이 값을 전역 sandbox로 받고 문서 단위 허용은 backend의 게이트가 적용한다(ADR-0053).
    let sandbox_default = plugin_id != "com.tasty.markdown";
    let sandbox = match s.plugin_setting(plugin_id, "sandbox_scripts") {
        Some(PluginSettingValue::Bool(b)) => *b,
        _ => sandbox_default,
    };
    let allow_remote_content = match s.plugin_setting(plugin_id, "allow_remote_content") {
        Some(PluginSettingValue::Bool(b)) => *b,
        _ => false,
    };
    let color_scheme = match s.plugin_setting(plugin_id, "color_scheme") {
        Some(PluginSettingValue::Text(t)) => match t.as_str() {
            "light" => ColorScheme::Light,
            "dark" => ColorScheme::Dark,
            _ => ColorScheme::Follow,
        },
        _ => ColorScheme::Follow,
    };
    HtmlWebViewSettings {
        zoom_percent,
        javascript_enabled: !sandbox, // "Sandbox scripts" on(기본) → JS off
        allow_remote_content,
        color_scheme,
    }
}

/// Find the URL for an Html panel by surface ID.
fn find_webview_url(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    surface_id: u32,
) -> Option<String> {
    find_surface_anywhere(view, engine, surface_id)?
        .webview_url()
        .map(|u| u.to_string())
}

fn note_host_webview_load(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    sid: u32,
) {
    if let Some(rs) = find_remote_surface(view, engine, sid) {
        rs.with_html_script(|st| st.on_host_load_requested());
    }
}

fn update_html_script_banners(
    view: &mut MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    all_html_ids: &[u32],
) {
    note_user_selection(view, engine);
    let mut phases = std::collections::HashMap::new();
    for &sid in all_html_ids {
        let Some(phase) = update_html_script_banner(view, engine, sid) else {
            continue;
        };
        if phase != BannerPhase::Hidden {
            phases.insert(sid, phase);
        }
    }
    if phases != view.html_script_phases {
        view.base.state.dirty = true;
    }
    view.html_script_phases = phases;
}

fn note_user_selection(view: &mut MainView, engine: &crate::runtime::engine_access::EngineRef<'_>) {
    let focused = view.state.focused_surface_id(engine);
    let selected = match view.html_script_seen_focus.replace(focused) {
        Some(prev) if prev != focused => focused,
        _ => None,
    };
    if let Some(sid) = selected
        && let Some(rs) = find_remote_surface(view, engine, sid)
        && rs.kind_static == "html"
    {
        tracing::debug!("html script banner: surface {sid} selected by the user");
        rs.with_html_script(|st| st.on_user_view());
    }
}

fn update_html_script_banner(
    view: &MainView,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    sid: u32,
) -> Option<BannerPhase> {
    let rs = find_remote_surface(view, engine, sid).filter(|rs| rs.kind_static == "html")?;
    let phase = rs.with_html_script(|st| st.update_banner());
    let before = view
        .html_script_phases
        .get(&sid)
        .copied()
        .unwrap_or(BannerPhase::Hidden);
    if phase != before {
        tracing::debug!(
            "html script banner: surface {sid} {} -> {}",
            before.as_str(),
            phase.as_str()
        );
    }
    Some(phase)
}
