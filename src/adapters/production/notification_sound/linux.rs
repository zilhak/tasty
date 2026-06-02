#![cfg(target_os = "linux")]

//! Linux NotificationSoundPlayer impl.
//!
//! 외부 의존성 없이 `std::process::Command` 로 paplay → aplay → TTY `\a`
//! 3 단 폴백. detection 결과는 `OnceLock` 으로 캐시.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::ports::notification_sound::NotificationSoundPlayer;

#[derive(Clone, Copy)]
enum Strategy {
    Paplay,
    Aplay,
    TtyBell,
}

const PAPLAY_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/bell.oga";
const APLAY_SOUND: &str = "/usr/share/sounds/alsa/Front_Center.wav";

static STRATEGY: OnceLock<Strategy> = OnceLock::new();

fn cmd_available(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn detect_strategy() -> Strategy {
    if cmd_available("paplay") && Path::new(PAPLAY_SOUND).exists() {
        Strategy::Paplay
    } else if cmd_available("aplay") && Path::new(APLAY_SOUND).exists() {
        Strategy::Aplay
    } else {
        Strategy::TtyBell
    }
}

pub struct LinuxBeepPlayer;

impl NotificationSoundPlayer for LinuxBeepPlayer {
    fn play(&self) {
        let strat = *STRATEGY.get_or_init(detect_strategy);
        let result = match strat {
            Strategy::Paplay => Command::new("paplay")
                .arg(PAPLAY_SOUND)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ()),
            Strategy::Aplay => Command::new("aplay")
                .arg(APLAY_SOUND)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ()),
            Strategy::TtyBell => {
                use std::io::Write;
                let _ = std::io::stderr().write_all(b"\x07"); // 사운드 부재 = stderr 실패와 동등, notification 발화는 막지 않음.
                Ok(())
            }
        };
        if let Err(e) = result {
            tracing::warn!("notification sound playback failed: {e}");
        }
    }
}
