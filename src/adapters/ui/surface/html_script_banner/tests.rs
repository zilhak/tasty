use super::*;

const FADE: f64 = 0.12;

fn phase_after(phase: BannerPhase, memo: FadeMemo, now: f64) -> (Option<Shown>, FadeMemo) {
    resolve(phase, memo, now, FADE)
}

#[test]
fn blocked_and_reloading_show_at_full_opacity() {
    let (shown, _) = phase_after(BannerPhase::Blocked, FadeMemo::default(), 0.0);
    assert_eq!(
        shown,
        Some(Shown {
            state: HtmlScriptBannerState::Blocked,
            alpha: 1.0
        })
    );
    let (shown, memo) = phase_after(BannerPhase::Reloading, FadeMemo::default(), 0.0);
    assert_eq!(
        shown.map(|s| s.state),
        Some(HtmlScriptBannerState::Reloading)
    );
    assert!(memo.showing_reload);
}

#[test]
fn loading_shows_the_disabled_allow_at_full_opacity() {
    let (shown, memo) = phase_after(BannerPhase::Loading, FadeMemo::default(), 0.0);
    assert_eq!(
        shown,
        Some(Shown {
            state: HtmlScriptBannerState::Loading,
            alpha: 1.0
        })
    );
    assert_eq!(memo, FadeMemo::default());
}

#[test]
fn a_committed_reload_fades_out_over_the_banner_fade() {
    let (_, memo) = phase_after(BannerPhase::Reloading, FadeMemo::default(), 1.0);
    let (shown, memo) = phase_after(BannerPhase::Hidden, memo, 2.0);
    let first = shown.expect("fade starts");
    assert_eq!(first.state, HtmlScriptBannerState::Reloading);
    assert_eq!(first.alpha, 1.0);
    let (shown, memo) = phase_after(BannerPhase::Hidden, memo, 2.0 + FADE / 2.0);
    let half = shown.expect("still fading").alpha;
    assert!((half - 0.5).abs() < 1e-3, "{half}");
    let (shown, memo) = phase_after(BannerPhase::Hidden, memo, 2.0 + FADE);
    assert_eq!(shown, None);
    assert_eq!(memo, FadeMemo::default());
}

#[test]
fn dismissing_or_a_new_document_hides_without_a_fade() {
    let (_, memo) = phase_after(BannerPhase::Blocked, FadeMemo::default(), 1.0);
    let (shown, _) = phase_after(BannerPhase::Hidden, memo, 1.01);
    assert_eq!(shown, None);
}
